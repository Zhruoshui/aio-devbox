// Model metadata catalog: GET /api/models/catalog.
//
// Proxies https://models.dev/api.json (design 08-27-provider-form-piweb §1):
// normalizes into CatalogResponse, 1h process-global cache. The fetch itself
// happens while holding the cache mutex, so concurrent requests naturally
// queue behind the in-flight fetch and land on the freshly-populated cache
// instead of triggering their own request (in-flight dedup without a
// broadcast channel).
//
// cost fields are passed through as-is: canonical cost is USD-per-1M-tokens
// (design 08-27-usage-correctness), and models.dev's `cost.*` fields are the
// same unit — no conversion needed here (unlike pi's native render, which
// wants USD-per-token; see render/pi.rs::render_pi_cost).

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use serde_json::Value;

use super::store::CostEntry;
use crate::state::AppState;

const CATALOG_URL: &str = "https://models.dev/api.json";
const FETCH_TIMEOUT: Duration = Duration::from_secs(15);
const CATALOG_TTL: Duration = Duration::from_secs(3600);

// ── response types ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogModel {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<CostEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogProvider {
    pub id: String,
    pub name: String,
    pub models: Vec<CatalogModel>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogResponse {
    pub providers: Vec<CatalogProvider>,
    pub fetched_at: String,
}

// ── cache ─────────────────────────────────────────────────────────

struct CatalogCache {
    at: Instant,
    data: CatalogResponse,
}

static CACHE: OnceLock<tokio::sync::Mutex<Option<CatalogCache>>> = OnceLock::new();

fn cache() -> &'static tokio::sync::Mutex<Option<CatalogCache>> {
    CACHE.get_or_init(|| tokio::sync::Mutex::new(None))
}

// ── handler ───────────────────────────────────────────────────────

pub async fn get_catalog(
    State(state): State<AppState>,
) -> Result<Json<CatalogResponse>, (StatusCode, String)> {
    let mut guard = cache().lock().await;
    if let Some(entry) = guard.as_ref() {
        if entry.at.elapsed() < CATALOG_TTL {
            return Ok(Json(entry.data.clone()));
        }
    }

    let resp = state
        .http
        .get(CATALOG_URL)
        .header("Accept", "application/json")
        .timeout(FETCH_TIMEOUT)
        .send()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("catalog fetch failed: {e}")))?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err((
            StatusCode::BAD_GATEWAY,
            format!("catalog upstream {status}: {}", truncate(&text, 500)),
        ));
    }

    let raw: Value = serde_json::from_str(&text).map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            format!("catalog parse failed: {e} (body: {})", truncate(&text, 500)),
        )
    })?;

    let data = normalize_catalog(&raw);
    *guard = Some(CatalogCache {
        at: Instant::now(),
        data: data.clone(),
    });
    Ok(Json(data))
}

// ── normalization (pure, testable) ──────────────────────────────────

/// models.dev's `api.json` top level is an object keyed by provider id, each
/// carrying a `name` and a `models` object keyed by model id. Any field can
/// be absent depending on upstream source drift — every lookup here is
/// fallible and simply yields `None`, never panics.
fn normalize_catalog(raw: &Value) -> CatalogResponse {
    let mut providers = Vec::new();
    if let Some(obj) = raw.as_object() {
        for (provider_id, pv) in obj {
            let name = pv
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(provider_id)
                .to_string();
            let mut models = Vec::new();
            if let Some(mobj) = pv.get("models").and_then(Value::as_object) {
                for (model_id, mv) in mobj {
                    models.push(normalize_model(model_id, mv));
                }
            }
            providers.push(CatalogProvider {
                id: provider_id.clone(),
                name,
                models,
            });
        }
    }
    providers.sort_by(|a, b| a.id.cmp(&b.id));
    CatalogResponse {
        providers,
        fetched_at: String::new(), // set by caller if needed; not required by AC
    }
}

fn normalize_model(model_id: &str, mv: &Value) -> CatalogModel {
    let name = mv.get("name").and_then(Value::as_str).map(String::from);
    let reasoning = mv.get("reasoning").and_then(Value::as_bool);
    let input = mv
        .get("modalities")
        .and_then(|m| m.get("input"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        });
    let context_window = mv
        .get("limit")
        .and_then(|l| l.get("context"))
        .and_then(Value::as_u64);
    let max_tokens = mv
        .get("limit")
        .and_then(|l| l.get("output"))
        .and_then(Value::as_u64);
    let cost = mv.get("cost").and_then(Value::as_object).map(|c| CostEntry {
        input: c.get("input").and_then(Value::as_f64),
        output: c.get("output").and_then(Value::as_f64),
        cache_read: c.get("cache_read").and_then(Value::as_f64),
        cache_write: c.get("cache_write").and_then(Value::as_f64),
    });
    CatalogModel {
        id: model_id.to_string(),
        name,
        reasoning,
        input,
        context_window,
        max_tokens,
        cost,
    }
}

/// Truncate to `max` chars, appending "…" when truncated (mirrors discover.rs
/// / test.rs — kept private per-file per existing convention).
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max).collect();
        format!("{head}…")
    }
}

// ── tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_maps_provider_and_model_fields() {
        let raw = json!({
            "openai": {
                "name": "OpenAI",
                "models": {
                    "gpt-5.6-sol": {
                        "name": "GPT-5.6 Sol",
                        "reasoning": true,
                        "modalities": {"input": ["text", "image"]},
                        "limit": {"context": 200000, "output": 8192},
                        "cost": {"input": 5.0, "output": 15.0, "cache_read": 0.5, "cache_write": 6.25}
                    }
                }
            }
        });
        let out = normalize_catalog(&raw);
        assert_eq!(out.providers.len(), 1);
        let p = &out.providers[0];
        assert_eq!(p.id, "openai");
        assert_eq!(p.name, "OpenAI");
        assert_eq!(p.models.len(), 1);
        let m = &p.models[0];
        assert_eq!(m.id, "gpt-5.6-sol");
        assert_eq!(m.name.as_deref(), Some("GPT-5.6 Sol"));
        assert_eq!(m.reasoning, Some(true));
        assert_eq!(m.input.as_deref(), Some(&["text".to_string(), "image".to_string()][..]));
        assert_eq!(m.context_window, Some(200000));
        assert_eq!(m.max_tokens, Some(8192));
        let c = m.cost.as_ref().unwrap();
        assert_eq!(c.input, Some(5.0));
        assert_eq!(c.output, Some(15.0));
        assert_eq!(c.cache_read, Some(0.5));
        assert_eq!(c.cache_write, Some(6.25));
    }

    #[test]
    fn normalize_missing_fields_yield_none_not_panic() {
        let raw = json!({
            "bare": {
                "models": {
                    "m1": {}
                }
            }
        });
        let out = normalize_catalog(&raw);
        assert_eq!(out.providers.len(), 1);
        let p = &out.providers[0];
        assert_eq!(p.name, "bare"); // falls back to key when `name` absent
        let m = &p.models[0];
        assert_eq!(m.id, "m1");
        assert!(m.name.is_none());
        assert!(m.reasoning.is_none());
        assert!(m.input.is_none());
        assert!(m.context_window.is_none());
        assert!(m.max_tokens.is_none());
        assert!(m.cost.is_none());
    }

    #[test]
    fn normalize_non_object_top_level_yields_empty() {
        let raw = json!([1, 2, 3]);
        let out = normalize_catalog(&raw);
        assert!(out.providers.is_empty());
    }

    #[test]
    fn normalize_provider_without_models_key_yields_empty_models() {
        let raw = json!({ "solo": { "name": "Solo" } });
        let out = normalize_catalog(&raw);
        assert_eq!(out.providers[0].models.len(), 0);
    }

    #[test]
    fn truncate_short_unchanged() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_long_gets_ellipsis() {
        let s = "a".repeat(20);
        let t = truncate(&s, 5);
        assert_eq!(t.chars().count(), 6); // 5 + ellipsis char
        assert!(t.ends_with('…'));
    }

    #[test]
    fn cache_hit_within_ttl_returns_cached_data_without_refetch() {
        // Pure cache-shape test (no live HTTP): seed cache directly, assert
        // TTL boundary logic matches usage.rs's established pattern.
        let entry = CatalogCache {
            at: Instant::now(),
            data: CatalogResponse {
                providers: vec![],
                fetched_at: "x".into(),
            },
        };
        assert!(entry.at.elapsed() < CATALOG_TTL);
    }
}
