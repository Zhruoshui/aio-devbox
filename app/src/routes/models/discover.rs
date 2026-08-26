// Model discovery: POST /api/models/discover.
//
// Resolves /v1/models (or protocol-equivalent) from a provider endpoint and
// returns a deduped, sorted list of model ids. The URL derivation, multi-
// candidate fallback, header construction and multi-shape response parsing
// follow design §5 and research/pi-web §2 + cc-switch §4.
//
// All pure helpers (URL candidates, header construction, response parsing) are
// unit-tested here; live HTTP is exercised by the functional test later.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::state::AppState;
use super::store::{read_config, StoreError};

/// Timeout per candidate request (design §5: 20s).
const PER_CANDIDATE_TIMEOUT: Duration = Duration::from_secs(20);
/// Total budget across all candidates for one discover call (design §5: 20s).
const TOTAL_BUDGET: Duration = Duration::from_secs(20);

// ── request / response types ──────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum DiscoverRequest {
    /// Resolve everything from the canonical store by provider id.
    ById { providerId: String },
    /// Literal fields (transient provider being edited, or ad-hoc endpoint).
    Literal {
        baseUrl: String,
        #[serde(default = "default_api")]
        api: String,
        apiKey: Option<String>,
    },
}

fn default_api() -> String {
    "openai-completions".to_string()
}

#[derive(Debug, Serialize)]
pub struct DiscoveredModel {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DiscoverResponse {
    pub models: Vec<DiscoveredModel>,
    pub endpoint: String,
}

// ── handler ───────────────────────────────────────────────────────

pub async fn discover(
    State(state): State<AppState>,
    Json(req): Json<DiscoverRequest>,
) -> Result<Json<DiscoverResponse>, (StatusCode, String)> {
    // Resolve the effective provider fields (baseUrl/api/apiKey/headers).
    let resolved = resolve_provider(&state, &req).await?;

    // Candidate URLs in priority order (design §5 + cc-switch §4).
    let candidates = candidate_urls(&resolved.base_url, &resolved.api);

    // Build headers once (identical across candidates for the same protocol).
    let headers = build_headers(&resolved.api, resolved.api_key.as_deref(), &resolved.headers);

    // Total deadline across all candidates so a slow first host can't blow the
    // whole budget on retries that should go to the next candidate.
    let deadline = Instant::now() + TOTAL_BUDGET;

    let mut tried: Vec<String> = Vec::new();
    let mut first_auth_failure: Option<(StatusCode, String)> = None;

    for (idx, url) in candidates.iter().enumerate() {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .unwrap_or(Duration::ZERO);
        if remaining.is_zero() {
            tried.push(url.clone());
            break;
        }
        let per = remaining.min(PER_CANDIDATE_TIMEOUT);
        tried.push(url.clone());

        let mut req_builder = state.http.get(url.as_str()).header("Accept", "application/json");
        for (k, v) in &headers {
            req_builder = req_builder.header(k, v);
        }
        req_builder = req_builder.timeout(per);

        let resp = req_builder.send().await;
        match resp {
            Err(e) => {
                // connect/timeout/transport error -> next candidate
                tracing::debug!(target: "models::discover", "candidate {url} transport err: {e}");
                continue;
            }
            Ok(r) => {
                let status = r.status();
                if status.is_success() {
                    let text = r.text().await.unwrap_or_default();
                    let models = parse_discovered_models(&text);
                    if !models.is_empty() {
                        return Ok(Json(DiscoverResponse {
                            models,
                            endpoint: url.clone(),
                        }));
                    }
                    // 2xx but no parseable models -> treat as exhaustion.
                    return Err((
                        StatusCode::BAD_GATEWAY,
                        format!(
                            "no models parsed from {url} (body truncated: {})",
                            truncate(&text, 500)
                        ),
                    ));
                }
                // 401/403 from the FIRST candidate short-circuits: the key is
                // wrong, fallback URLs would 401 too (design §5). From later
                // candidates it's just a skip - a gateway may expose the list
                // endpoint at a different path.
                if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
                    let body = r.text().await.unwrap_or_default();
                    let err = format!(
                        "{} {} :: {}",
                        status.as_u16(),
                        url,
                        truncate(&body, 500)
                    );
                    if idx == 0 {
                        return Err((status, err));
                    }
                    if first_auth_failure.is_none() {
                        first_auth_failure = Some((status, err));
                    }
                    continue;
                }
                // 5xx/404/other -> next candidate
                tracing::debug!(target: "models::discover", "candidate {url} status {status}");
                continue;
            }
        }
    }

    // All candidates exhausted.
    if let Some((status, msg)) = first_auth_failure {
        return Err((status, msg));
    }
    Err((
        StatusCode::BAD_GATEWAY,
        format!("all candidates failed; tried: {}", tried.join(", ")),
    ))
}

// ── provider resolution ───────────────────────────────────────────

struct ResolvedProvider {
    base_url: String,
    api: String,
    api_key: Option<String>,
    headers: BTreeMap<String, String>,
}

/// Resolve the effective provider fields for a discover request.
/// ById reads the REAL stored key (no mask semantics here — design §3).
/// Literal uses the provided fields verbatim (key optional for keyless gateways).
async fn resolve_provider(
    state: &AppState,
    req: &DiscoverRequest,
) -> Result<ResolvedProvider, (StatusCode, String)> {
    match req {
        DiscoverRequest::ById { providerId } => {
            let config = match read_config(&state.models_file) {
                Ok(c) => c,
                Err(StoreError::Io(e)) => {
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("read models.json: {e}"),
                    ));
                }
                Err(StoreError::Corrupt(e)) => {
                    return Err((
                        StatusCode::UNPROCESSABLE_ENTITY,
                        format!("models.json corrupt: {e}"),
                    ));
                }
            };
            let p = config.providers.get(providerId).ok_or_else(|| {
                (
                    StatusCode::NOT_FOUND,
                    format!("provider '{}' not found", providerId),
                )
            })?;
            Ok(ResolvedProvider {
                base_url: p.base_url.clone(),
                api: p.api.clone(),
                api_key: p.api_key.clone(),
                headers: p.headers.clone(),
            })
        }
        DiscoverRequest::Literal {
            baseUrl,
            api,
            apiKey,
        } => {
            if baseUrl.trim().is_empty() {
                return Err((StatusCode::BAD_REQUEST, "baseUrl is required".to_string()));
            }
            Ok(ResolvedProvider {
                base_url: baseUrl.clone(),
                api: api.clone(),
                api_key: apiKey.clone(),
                headers: BTreeMap::new(),
            })
        }
    }
}

// ── URL derivation (pure) ─────────────────────────────────────────

/// Strip a trailing path segment if it matches one of `suffixes`. Returns the
/// original string when the base does not end with the suffix.
fn strip_trailing<'a>(base: &'a str, suffixes: &[&str]) -> &'a str {
    for s in suffixes {
        if base.ends_with(s) {
            return &base[..base.len() - s.len()];
        }
    }
    base
}

/// The anthropic-style suffixes cc-switch strips before re-deriving a models
/// URL (cc-switch `model_fetch.rs:207-263`).
const ANTHROPIC_SUFFIXES: &[&str] = &["/anthropic", "/claude", "/api/coding"];

/// Derive the primary models URL for a base URL + protocol (pi-web
/// `buildModelsListUrl`, design §5).
///
/// - openai-completions / openai-responses -> `<base>/models` (when base
///   doesn't already end with `/models`)
/// - anthropic-messages -> insert `/v1` before any trailing path if missing,
///   then append `/models?limit=1000`
/// - google-generative-ai -> `/v1beta/models?pageSize=1000` (best-effort;
///   not exercised in the current gateway matrix)
fn primary_models_url(base: &str, api: &str) -> String {
    let base = base.trim_end_matches('/');
    if base.ends_with("/models") {
        return base.to_string();
    }
    match api {
        "anthropic-messages" => {
            // Insert /v1 if the base has no path beyond the host, OR if the
            // path doesn't already start with /v1. cc-switch inserts /v1
            // unconditionally; pi-web inserts it only when the path is empty.
            // We follow pi-web: if there's no path (or just a trailing slash
            // already trimmed), add /v1.
            let after_scheme = base
                .split_once("://")
                .map(|(_, rest)| rest)
                .unwrap_or(base);
            let path = after_scheme
                .split_once('/')
                .map(|(_, p)| p)
                .unwrap_or("");
            if path.is_empty() {
                format!("{base}/v1/models?limit=1000")
            } else {
                format!("{base}/models?limit=1000")
            }
        }
        "google-generative-ai" => format!("{base}/v1beta/models?pageSize=1000"),
        _ => format!("{base}/models"),
    }
}

/// Build the ordered, deduped candidate URL list for a base + protocol
/// (cc-switch multi-candidate + pi-web primary derivation, design §5).
///
/// Order:
/// 1. primary derivation (api-specific)
/// 2. `<base>/v1/models` (cc-switch fallback)
/// 3. `<base>/models` (cc-switch fallback; may duplicate primary for openai)
/// 4. strip anthropic suffixes from base, then re-derive primary
///
/// Dedupe preserves order (first occurrence wins; the primary is always first
/// unless it duplicated an earlier entry).
pub fn candidate_urls(base: &str, api: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let primary = primary_models_url(base, api);
    out.push(primary.clone());

    let base_trim = base.trim_end_matches('/');

    let push_unique = |out: &mut Vec<String>, url: String| {
        if !out.iter().any(|u| u == &url) {
            out.push(url);
        }
    };

    // cc-switch fallbacks (protocol-agnostic).
    push_unique(&mut out, format!("{base_trim}/v1/models"));
    push_unique(&mut out, format!("{base_trim}/models"));

    // Strip anthropic-style suffixes and re-derive.
    let stripped = strip_trailing(base_trim, ANTHROPIC_SUFFIXES);
    if stripped != base_trim {
        let re_primary = primary_models_url(stripped, api);
        push_unique(&mut out, re_primary);
        push_unique(&mut out, format!("{stripped}/v1/models"));
        push_unique(&mut out, format!("{stripped}/models"));
    }

    out
}

// ── header construction (pure) ────────────────────────────────────

/// Build the per-request headers for a discover/test call (design §5).
///
/// - `Accept: application/json` always.
/// - anthropic-messages -> `x-api-key: <key>` + `anthropic-version: 2023-06-01`
/// - openai* (and anything else) -> `Authorization: Bearer <key>`
/// - When `key` is None/empty, the auth header is omitted (some gateways list
///   models keyless).
/// - Provider extra headers are merged on top.
pub fn build_headers(
    api: &str,
    key: Option<&str>,
    extra: &BTreeMap<String, String>,
) -> Vec<(String, String)> {
    let mut h: Vec<(String, String)> = Vec::new();
    h.push(("Accept".to_string(), "application/json".to_string()));

    let key = key.filter(|k| !k.is_empty());
    match api {
        "anthropic-messages" => {
            if let Some(k) = key {
                h.push(("x-api-key".to_string(), k.to_string()));
            }
            h.push(("anthropic-version".to_string(), "2023-06-01".to_string()));
        }
        _ => {
            if let Some(k) = key {
                h.push(("Authorization".to_string(), format!("Bearer {k}")));
            }
        }
    }

    // Provider extra headers (may override the above — e.g. custom x-api-key).
    for (k, v) in extra {
        // Replace any existing header with the same name (case-insensitive).
        let lower = k.to_ascii_lowercase();
        if let Some(slot) = h.iter_mut().find(|(n, _)| n.to_ascii_lowercase() == lower) {
            slot.1 = v.clone();
        } else {
            h.push((k.clone(), v.clone()));
        }
    }

    h
}

// ── response parsing (pure) ───────────────────────────────────────

/// Parse a /v1/models response body into a deduped, naturally-sorted list of
/// model ids (pi-web `parseDiscoveredModels`, design §5).
///
/// Accepted shapes:
/// - bare array: `[...]`
/// - object with array under `data` | `models` | `results` | `items`
/// - object-of-objects: `{ "<id>": {...} }`
///
/// Each item is either a string or an object with `id` | `model` | `name` and
/// an optional display name under `display_name` | `displayName` | `name`.
/// The `models/` prefix is stripped (Gemini). Duplicates removed; natural sort.
pub fn parse_discovered_models(body: &str) -> Vec<DiscoveredModel> {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let raw_items = collect_items(&v);
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut out: Vec<DiscoveredModel> = Vec::new();

    for item in raw_items {
        // Items are (optional key, value) pairs. For array shapes the key is
        // None; for object-of-objects the key is the model id (pi-web
        // `parseDiscoveredModels`).
        let (id_raw, name) = match item {
            (None, Value::String(s)) => (s, None),
            (key_override, Value::Object(o)) => {
                // Object-of-objects: the key IS the id (wins over any inner
                // field, matching pi-web).
                let id = key_override.or_else(|| {
                    o.get("id")
                        .or_else(|| o.get("model"))
                        .or_else(|| o.get("name"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                });
                let id = match id {
                    Some(i) => i,
                    None => continue,
                };
                let name = o
                    .get("display_name")
                    .or_else(|| o.get("displayName"))
                    .or_else(|| o.get("name"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                (id, name)
            }
            (Some(k), Value::String(s)) => {
                // Object-of-objects with a string value: key is id, value is name.
                (k, Some(s))
            }
            _ => continue,
        };

        // Strip leading "models/" (Gemini).
        let id = id_raw
            .strip_prefix("models/")
            .map(|s| s.to_string())
            .unwrap_or(id_raw);

        if id.is_empty() {
            continue;
        }
        if seen.insert(id.clone()) {
            out.push(DiscoveredModel { id, name });
        }
    }

    // Natural sort (case-insensitive, numeric-aware) for stable display.
    out.sort_by(|a, b| natcmp(&a.id, &b.id));
    out
}

/// Collect the list of (optional key, item value) pairs from a parsed
/// response. Array shapes yield (None, value); object-of-objects yields
/// (Some(key), value) so the parser can use the key as the id (pi-web
/// `parseDiscoveredModels`).
fn collect_items(v: &Value) -> Vec<(Option<String>, Value)> {
    match v {
        Value::Array(arr) => arr.iter().cloned().map(|x| (None, x)).collect(),
        Value::Object(o) => {
            for key in ["data", "models", "results", "items"] {
                if let Some(Value::Array(arr)) = o.get(key) {
                    return arr.iter().cloned().map(|x| (None, x)).collect();
                }
            }
            // Object-of-objects: every (key, value) is a model entry; the key
            // is the id.
            o.iter()
                .map(|(k, v)| (Some(k.clone()), v.clone()))
                .collect()
        }
        _ => Vec::new(),
    }
}

/// Truncate to `max` chars, appending "…" when truncated.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max).collect();
        format!("{head}…")
    }
}

/// Natural comparison: split into runs of digits and non-digits, compare digit
/// runs numerically, the rest byte-wise. Falls back to a plain string compare
/// when either side has no digits.
fn natcmp(a: &str, b: &str) -> std::cmp::Ordering {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek(), bi.peek()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(&ca), Some(&cb)) => {
                if ca.is_ascii_digit() && cb.is_ascii_digit() {
                    // Collect full digit runs.
                    let mut na: String = String::new();
                    let mut nb: String = String::new();
                    while let Some(&c) = ai.peek() {
                        if c.is_ascii_digit() {
                            na.push(c);
                            ai.next();
                        } else {
                            break;
                        }
                    }
                    while let Some(&c) = bi.peek() {
                        if c.is_ascii_digit() {
                            nb.push(c);
                            bi.next();
                        } else {
                            break;
                        }
                    }
                    // Compare numerically, ignoring leading zeros (so 01 == 1).
                    let va: u64 = na.trim_start_matches('0').parse().unwrap_or(0);
                    let vb: u64 = nb.trim_start_matches('0').parse().unwrap_or(0);
                    match va.cmp(&vb) {
                        std::cmp::Ordering::Equal => continue,
                        other => return other,
                    }
                } else {
                    match ca.cmp(&cb) {
                        std::cmp::Ordering::Equal => {
                            ai.next();
                            bi.next();
                        }
                        other => return other,
                    }
                }
            }
        }
    }
}

// ── tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // --- URL candidate derivation ---

    #[test]
    fn openai_base_without_v1_appends_models() {
        let urls = candidate_urls("https://api.openai.com", "openai-completions");
        assert_eq!(urls[0], "https://api.openai.com/models");
        // cc-switch fallbacks
        assert!(urls.iter().any(|u| u == "https://api.openai.com/v1/models"));
        assert!(urls.iter().any(|u| u == "https://api.openai.com/models"));
    }

    #[test]
    fn openai_base_with_v1_appends_models() {
        let urls = candidate_urls("https://api.openai.com/v1", "openai-completions");
        assert_eq!(urls[0], "https://api.openai.com/v1/models");
        // Primary already covers /v1/models; fallbacks deduped out.
        assert_eq!(urls.len(), 2); // primary (/v1/models) + bare /models
    }

    #[test]
    fn base_already_ending_models_is_not_doubled() {
        let urls = candidate_urls("https://api.example.com/v1/models", "openai-completions");
        assert_eq!(urls[0], "https://api.example.com/v1/models");
        // No duplicate of the primary.
        assert_eq!(
            urls.iter()
                .filter(|u| *u == "https://api.example.com/v1/models")
                .count(),
            1
        );
    }

    #[test]
    fn anthropic_base_no_path_inserts_v1() {
        let urls = candidate_urls("https://api.anthropic.com", "anthropic-messages");
        assert_eq!(
            urls[0],
            "https://api.anthropic.com/v1/models?limit=1000"
        );
    }

    #[test]
    fn anthropic_base_with_path_keeps_path() {
        let urls = candidate_urls(
            "https://ai.aruoshui.com/anthropic",
            "anthropic-messages",
        );
        // primary derives from the full base (no /v1 insertion since path exists).
        assert_eq!(
            urls[0],
            "https://ai.aruoshui.com/anthropic/models?limit=1000"
        );
        // stripped variant re-derives from the bare host.
        assert!(urls
            .iter()
            .any(|u| u == "https://ai.aruoshui.com/v1/models?limit=1000"));
    }

    #[test]
    fn anthropic_suffix_stripped_in_fallbacks() {
        let urls = candidate_urls(
            "https://ai.aruoshui.com/v1/anthropic",
            "openai-completions",
        );
        // primary
        assert!(urls
            .iter()
            .any(|u| u == "https://ai.aruoshui.com/v1/anthropic/models"));
        // stripped
        assert!(urls
            .iter()
            .any(|u| u == "https://ai.aruoshui.com/v1/models"));
    }

    #[test]
    fn candidate_urls_dedupe_preserving_order() {
        let urls = candidate_urls("https://api.example.com/v1", "openai-completions");
        let mut seen = std::collections::HashSet::new();
        for u in &urls {
            assert!(seen.insert(u.clone()), "duplicate: {u}");
        }
    }

    // --- header construction ---

    #[test]
    fn openai_bearer_header() {
        let h = build_headers(
            "openai-completions",
            Some("sk-test1234"),
            &BTreeMap::new(),
        );
        let auth = h.iter().find(|(n, _)| n.eq_ignore_ascii_case("authorization"));
        assert_eq!(auth.map(|(_, v)| v.as_str()), Some("Bearer sk-test1234"));
        assert!(h.iter().any(|(n, _)| n.eq_ignore_ascii_case("accept")));
    }

    #[test]
    fn anthropic_headers() {
        let h = build_headers(
            "anthropic-messages",
            Some("sk-ant-xx"),
            &BTreeMap::new(),
        );
        let key = h.iter().find(|(n, _)| n.eq_ignore_ascii_case("x-api-key"));
        assert_eq!(key.map(|(_, v)| v.as_str()), Some("sk-ant-xx"));
        let ver = h.iter().find(|(n, _)| n.eq_ignore_ascii_case("anthropic-version"));
        assert_eq!(ver.map(|(_, v)| v.as_str()), Some("2023-06-01"));
        // No bearer for anthropic.
        assert!(h
            .iter()
            .all(|(n, _)| !n.eq_ignore_ascii_case("authorization")));
    }

    #[test]
    fn no_key_omits_auth_header() {
        let h = build_headers("openai-completions", None, &BTreeMap::new());
        assert!(h
            .iter()
            .all(|(n, _)| !n.eq_ignore_ascii_case("authorization")));
        assert!(h.iter().any(|(n, _)| n.eq_ignore_ascii_case("accept")));
    }

    #[test]
    fn empty_key_omits_auth_header() {
        let h = build_headers(
            "openai-completions",
            Some(""),
            &BTreeMap::new(),
        );
        assert!(h
            .iter()
            .all(|(n, _)| !n.eq_ignore_ascii_case("authorization")));
    }

    #[test]
    fn extra_headers_override_defaults() {
        let mut extra = BTreeMap::new();
        extra.insert("x-custom".to_string(), "abc".to_string());
        // Override the Accept default.
        extra.insert("accept".to_string(), "text/plain".to_string());
        let h = build_headers("openai-completions", Some("sk-1"), &extra);
        let accept = h.iter().find(|(n, _)| n.eq_ignore_ascii_case("accept"));
        assert_eq!(accept.map(|(_, v)| v.as_str()), Some("text/plain"));
        assert!(h.iter().any(|(n, v)| n == "x-custom" && v == "abc"));
    }

    // --- response parsing ---

    #[test]
    fn parse_bare_array() {
        let body = r#"["gpt-4", "gpt-3.5-turbo"]"#;
        let m = parse_discovered_models(body);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].id, "gpt-3.5-turbo"); // natural sort
        assert_eq!(m[1].id, "gpt-4");
    }

    #[test]
    fn parse_openai_data_shape() {
        let body = r#"{"data":[{"id":"gpt-4"},{"id":"gpt-3.5-turbo"}]}"#;
        let m = parse_discovered_models(body);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].id, "gpt-3.5-turbo");
    }

    #[test]
    fn parse_models_key_shape() {
        let body = r#"{"models":[{"id":"m1","display_name":"M One"}]}"#;
        let m = parse_discovered_models(body);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].id, "m1");
        assert_eq!(m[0].name.as_deref(), Some("M One"));
    }

    #[test]
    fn parse_object_of_objects() {
        let body = r#"{"model-a":{"name":"A"},"model-b":{}}"#;
        let m = parse_discovered_models(body);
        assert_eq!(m.len(), 2);
        let ids: Vec<_> = m.iter().map(|x| x.id.as_str()).collect();
        assert!(ids.contains(&"model-a"));
        assert!(ids.contains(&"model-b"));
        assert_eq!(m.iter().find(|x| x.id == "model-a").unwrap().name.as_deref(), Some("A"));
    }

    #[test]
    fn parse_item_with_model_field_not_id() {
        let body = r#"{"data":[{"model":"alt-id"}]}"#;
        let m = parse_discovered_models(body);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].id, "alt-id");
    }

    #[test]
    fn parse_strips_models_prefix_gemini() {
        let body = r#"["models/gemini-1.5-pro","models/gemini-1.5-flash"]"#;
        let m = parse_discovered_models(body);
        assert_eq!(m.len(), 2);
        assert!(m.iter().all(|x| !x.id.starts_with("models/")));
        assert_eq!(m[0].id, "gemini-1.5-flash");
    }

    #[test]
    fn parse_dedupes() {
        let body = r#"["gpt-4","gpt-4",{"id":"gpt-4"}]"#;
        let m = parse_discovered_models(body);
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn parse_natural_sort() {
        let body = r#"["model-10","model-2","model-1"]"#;
        let m = parse_discovered_models(body);
        assert_eq!(
            m.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(),
            vec!["model-1", "model-2", "model-10"]
        );
    }

    #[test]
    fn parse_empty_array() {
        let m = parse_discovered_models("[]");
        assert!(m.is_empty());
    }

    #[test]
    fn parse_invalid_json_returns_empty() {
        let m = parse_discovered_models("not json");
        assert!(m.is_empty());
    }

    #[test]
    fn parse_unknown_shape_returns_empty() {
        // A bare string is not a valid models response.
        let m = parse_discovered_models(r#""hello""#);
        assert!(m.is_empty());
    }

    // --- truncate ---

    #[test]
    fn truncate_short_unchanged() {
        assert_eq!(truncate("abc", 10), "abc");
    }

    #[test]
    fn truncate_long_gets_ellipsis() {
        let s = "a".repeat(600);
        let t = truncate(&s, 500);
        assert!(t.ends_with('…'));
        assert_eq!(t.chars().count(), 501);
    }

    // --- natcmp ---

    #[test]
    fn natcmp_numeric_ordering() {
        assert_eq!(natcmp("m2", "m10"), std::cmp::Ordering::Less);
        assert_eq!(natcmp("m10", "m2"), std::cmp::Ordering::Greater);
        assert_eq!(natcmp("m1", "m1"), std::cmp::Ordering::Equal);
    }
}
