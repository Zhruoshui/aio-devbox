// Usage fan-out: GET /api/usage?window=today|7d|all (sandbox-mgr Phase 4a,
// design §3.7 — implementation adjusted, see below).
//
// For every sandbox the DB says is running, fetch that sandbox app's
// /api/models/usage and return all results side by side:
//
//   {"sandboxes": [{"name": "...", "error": null|"...", "usage": {…app shape…}}]}
//
// `usage` is the app response JSON passed through VERBATIM (rows +
// generatedAt, usage.rs in the app) — mgr does not re-aggregate across
// sandboxes (the mgr-web usage view concatenates rows with a sandbox
// column; Phase 4c).
//
// Fan-out details:
//   - URL is `http://sbx-<name>-piweb:8088/...`: `sbx-<name>-piweb` is the
//     alias composegen gives the app service on aio-mgr-net (the sandbox's
//     own gateway carries the sibling alias `sbx-<name>`, which proxies the
//     workbench - mgr talks to the app DIRECTLY, not through the sandbox
//     gateway, design §3.7). Port 8088 is the app's axum port. (design
//     §3.7 wrote `sbx-<name>:8089/8088` — that host/port does not match
//     what composegen actually aliases; composegen is the source of truth.)
//   - one shared reqwest client (AppState.http), 5s per-sandbox timeout;
//   - a sandbox that fails (down, restarting, slow) does NOT fail the
//     response: its entry carries `error` and the rest still return;
//   - adopted sandboxes are INCLUDED (Phase 5 adopt network-connects the
//     stack's app container onto aio-mgr-net under the same
//     `sbx-<name>-piweb` alias - routes.rs adopt_sandbox): the standard
//     repo stack answers /api/models/usage like any mgr sandbox, and a
//     foreign stack whose app lacks the endpoint degrades to an isolated
//     error entry, never a failed response.
//
// CACHING — deliberate deviation from design §3.7's "mgr polls every 60s":
// instead of a resident poller feeding /api/usage, requests fan out on
// demand and the per-sandbox results are cached for 30s keyed by
// `<name>:<window>` (AppState.usage_cache). mgr-web polling multiple
// tabs/views at a higher rate therefore does not multiply the load on the
// sandbox-side aggregation (which itself scans session logs). Semantics
// are equivalent (a poller + 60s cache answers with the same freshness
// envelope); this variant drops one piece of resident state. Errors are
// cached too — a down sandbox is not re-probed on every UI tick.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::db;
use crate::state::{AppState, CachedUsage};

/// Per-sandbox fetch timeout. Deliberately short: the fan-out runs inside a
/// request handler, and one slow sandbox must not hold the whole response.
const PER_SANDBOX_TIMEOUT: Duration = Duration::from_secs(5);
/// Cache TTL per (sandbox, window) — see module comment.
const CACHE_TTL: Duration = Duration::from_secs(30);
/// The app's axum port on aio-mgr-net (composegen alias `sbx-<name>-piweb`).
/// Shared with the sandbox proxy (proxy.rs) so the alias:port pairing has a
/// single owner.
pub const APP_PORT: u16 = 8088;

#[derive(Debug, serde::Deserialize)]
struct UsageQuery {
    #[serde(default = "default_window")]
    window: String,
}

fn default_window() -> String {
    "today".to_string()
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/api/usage", get(usage))
}

/// GET /api/usage — fan out to every running mgr-created sandbox.
async fn usage(State(state): State<Arc<AppState>>, Query(q): Query<UsageQuery>) -> Json<Value> {
    // Same window normalization as the app route: unknown -> today (usage
    // is read-only and harmless; a 400 here would break the whole page for
    // a typo'd query param).
    let window = match q.window.as_str() {
        "today" | "7d" | "all" => q.window.clone(),
        _ => "today".to_string(),
    };

    // Candidate sandboxes: DB status running, native AND adopted alike (the
    // adopt handler maintains the same status lifecycle). The status is
    // intent (a sandbox someone `docker compose down`ed behind mgr's back
    // still says running) — the per-sandbox probe turning that into an
    // `error` entry is exactly the honest reporting we want.
    let names: Vec<String> = {
        let conn = state.db.lock().unwrap();
        match db::list_sandboxes(&conn) {
            Ok(rows) => rows
                .into_iter()
                .filter(|r| r.status == "running")
                .map(|r| r.name)
                .collect(),
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "list sandboxes for usage fan-out");
                Vec::new()
            }
        }
    };

    let entries = fan_out(&state, &names, &window).await;
    // S4 (design §2): totals are derived from the entries we already have —
    // pure sum, never a re-scan. Error entries contribute zeros.
    let totals = assemble_totals(&entries);
    Json(json!({ "sandboxes": entries, "totals": totals }))
}

/// Sum every non-error entry's `usage.rows` into a per-sandbox total
/// (S4 design §2, R1). Shape: `[{"name", "in", "out", "cost"}]`.
///
/// Rows carry optional `cost` (claude/codex log none): a missing cost sums
/// as 0 — the frontend still knows the sandbox has no cost data via the
/// absence of a per-day cost series (AC4). Error entries have no usage, so
/// their totals are all zeros.
fn assemble_totals(entries: &[Value]) -> Vec<Value> {
    entries
        .iter()
        .map(|entry| {
            let name = entry["name"].as_str().unwrap_or("").to_string();
            let mut r#in: u64 = 0;
            let mut out: u64 = 0;
            let mut cost: f64 = 0.0;
            if let Some(rows) = entry["usage"].get("rows").and_then(|r| r.as_array()) {
                for row in rows {
                    if let Some(v) = row["in"].as_u64() {
                        r#in += v;
                    }
                    if let Some(v) = row["out"].as_u64() {
                        out += v;
                    }
                    if let Some(c) = row["cost"].as_f64() {
                        cost += c;
                    }
                }
            }
            json!({ "name": name, "in": r#in, "out": out, "cost": cost })
        })
        .collect()
}

/// Fan out to every sandbox concurrently. Cache hits (within TTL) skip the
/// HTTP probe; misses are fetched in parallel via join_all and written back.
/// Pure assembly shape is covered by unit tests on `assemble_entry`.
async fn fan_out(state: &Arc<AppState>, names: &[String], window: &str) -> Vec<Value> {
    // Split into cached / to-fetch while holding the std Mutex only for the
    // map read (never across the awaits below — state.rs discipline).
    let mut results: Vec<Option<Value>> = Vec::with_capacity(names.len());
    let mut to_fetch: Vec<(usize, &String)> = Vec::new();
    {
        let cache = state.usage_cache.lock().unwrap();
        for name in names {
            let key = format!("{name}:{window}");
            match cache.get(&key) {
                Some(hit) if hit.at.elapsed() < CACHE_TTL => results.push(Some(hit.entry.clone())),
                _ => {
                    results.push(None);
                    to_fetch.push((results.len() - 1, name));
                }
            }
        }
    }

    if !to_fetch.is_empty() {
        let fetches = to_fetch
            .iter()
            .map(|(_, name)| fetch_one(state, name, window));
        let outcomes = futures_util::future::join_all(fetches).await;
        let mut cache = state.usage_cache.lock().unwrap();
        for ((slot, name), outcome) in to_fetch.iter().zip(outcomes) {
            let entry = assemble_entry(name, outcome);
            cache.insert(
                format!("{name}:{window}"),
                CachedUsage {
                    at: Instant::now(),
                    entry: entry.clone(),
                },
            );
            results[*slot] = Some(entry);
        }
    }

    results.into_iter().flatten().collect()
}

/// The outcome of probing one sandbox: the app's usage JSON on success, a
/// message on failure (timeout / non-2xx / transport).
type FetchOutcome = Result<Value, String>;

/// Fetch one sandbox's usage payload. 5s timeout; any failure becomes a
/// string (never propagates to the response status).
async fn fetch_one(state: &Arc<AppState>, name: &str, window: &str) -> FetchOutcome {
    let url = format!("http://sbx-{name}-piweb:{APP_PORT}/api/models/usage?window={window}");
    let resp = state
        .http
        .get(&url)
        .timeout(PER_SANDBOX_TIMEOUT)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "HTTP {status}: {}",
            crate::models::truncate(&text, 200)
        ));
    }
    serde_json::from_str(&text).map_err(|e| format!("invalid usage JSON: {e}"))
}

/// Assemble one response entry from a probe outcome. Shape:
/// `{"name", "error": null|"...", "usage": {...}|null}` — `usage` is null
/// when the sandbox errored (the frontend renders the error, not a
/// zero-filled table).
fn assemble_entry(name: &str, outcome: FetchOutcome) -> Value {
    match outcome {
        Ok(usage) => json!({ "name": name, "error": null, "usage": usage }),
        Err(err) => json!({ "name": name, "error": err, "usage": null }),
    }
}

// ── tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Test state wrapped the way fan_out expects it.
    fn test_state() -> Arc<AppState> {
        Arc::new(AppState::new_for_test())
    }

    #[test]
    fn assemble_entry_ok_shape() {
        let usage = json!({ "rows": [], "generatedAt": "2026-09-09T00:00:00Z" });
        let e = assemble_entry("sbx-a", Ok(usage.clone()));
        assert_eq!(e["name"], "sbx-a");
        assert!(e["error"].is_null());
        assert_eq!(e["usage"], usage, "app payload passed through verbatim");
    }

    #[test]
    fn assemble_entry_error_is_isolated() {
        // A failing sandbox must not poison the response shape: error
        // carries the message, usage is null (not zero rows).
        let e = assemble_entry("sbx-b", Err("request failed: timed out".into()));
        assert_eq!(e["name"], "sbx-b");
        assert_eq!(e["error"], "request failed: timed out");
        assert!(e["usage"].is_null());
    }

    // --- assemble_totals (S4, design §2) ---

    /// Totals sum each entry's rows; cost sums only where present; missing
    /// cost (claude/codex rows) contributes 0 — the sandbox with no cost
    /// data still gets a numeric total.
    #[test]
    fn assemble_totals_sums_rows_and_optional_cost() {
        let entries = vec![
            json!({
                "name": "sbx-a",
                "error": null,
                "usage": {
                    "rows": [
                        { "agent": "pi", "model": "m1", "in": 100, "out": 50, "cost": 0.01 },
                        { "agent": "claude", "model": "m2", "in": 10, "out": 5 } // no cost
                    ]
                }
            }),
            json!({
                "name": "sbx-b",
                "error": null,
                "usage": { "rows": [{ "agent": "pi", "model": "m3", "in": 7, "out": 8, "cost": 0.5 }] }
            }),
        ];
        let totals = assemble_totals(&entries);
        assert_eq!(totals.len(), 2);
        assert_eq!(totals[0]["name"], "sbx-a");
        assert_eq!(totals[0]["in"], 110);
        assert_eq!(totals[0]["out"], 55);
        assert!((totals[0]["cost"].as_f64().unwrap() - 0.01).abs() < 1e-9);
        assert_eq!(totals[1]["name"], "sbx-b");
        assert_eq!(totals[1]["in"], 7);
        assert!((totals[1]["cost"].as_f64().unwrap() - 0.5).abs() < 1e-9);
    }

    /// An error entry has no usage — its totals come back all zeros.
    #[test]
    fn assemble_totals_error_entry_is_zero() {
        let entries = vec![json!({
            "name": "down",
            "error": "request failed: timed out",
            "usage": null,
        })];
        let totals = assemble_totals(&entries);
        assert_eq!(totals[0]["name"], "down");
        assert_eq!(totals[0]["in"], 0);
        assert_eq!(totals[0]["out"], 0);
        assert_eq!(totals[0]["cost"].as_f64(), Some(0.0));
    }

    #[tokio::test]
    async fn fan_out_merges_errors_and_successes_in_order() {
        // Inject a fake cached result for one sandbox and assert the
        // assembly path: names come back in input order, cached + fresh
        // entries coexist, the fresh fetch is cached, and an unreachable
        // sandbox surfaces as an error entry (not a silent hole).
        let state = test_state();
        let cached_entry = json!({
            "name": "alpha",
            "error": null,
            "usage": { "rows": [], "generatedAt": "cached" },
        });
        state.usage_cache.lock().unwrap().insert(
            "alpha:today".into(),
            CachedUsage {
                at: Instant::now(),
                entry: cached_entry.clone(),
            },
        );

        // "beta" is not cached: fan_out will try to reach it over HTTP,
        // fail fast (no such host in a test process), and record the error.
        let entries = fan_out(&state, &["alpha".to_string(), "beta".to_string()], "today").await;
        assert_eq!(entries.len(), 2, "input order and count preserved");
        assert_eq!(entries[0], cached_entry, "cache hit returned verbatim");
        assert_eq!(entries[1]["name"], "beta");
        assert!(
            entries[1]["error"]
                .as_str()
                .map(|s| !s.is_empty())
                .unwrap_or(false),
            "unreachable sandbox carries an error message"
        );
        assert!(entries[1]["usage"].is_null());

        // The fresh result landed in the cache.
        let cache = state.usage_cache.lock().unwrap();
        assert!(cache.contains_key("beta:today"), "fresh fetch cached");
    }

    #[tokio::test]
    async fn fan_out_expired_cache_entry_is_refetched() {
        // A stale entry (at: Instant::now() - 2 * TTL) must NOT be served.
        let state = test_state();
        let stale = json!({
            "name": "gamma",
            "error": null,
            "usage": { "rows": [], "generatedAt": "stale" },
        });
        state.usage_cache.lock().unwrap().insert(
            "gamma:today".into(),
            CachedUsage {
                // checked_sub keeps the test robust if the clock is at the
                // epoch; a panic here would mean the system clock is broken.
                at: Instant::now()
                    .checked_sub(CACHE_TTL + CACHE_TTL)
                    .expect("test clock far past epoch"),
                entry: stale,
            },
        );
        let entries = fan_out(&state, &["gamma".to_string()], "today").await;
        assert_eq!(entries.len(), 1);
        assert_ne!(
            entries[0]["usage"]["generatedAt"], "stale",
            "expired entry refetched"
        );
    }

    #[tokio::test]
    async fn fan_out_empty_names_returns_empty() {
        let state = test_state();
        let entries = fan_out(&state, &[], "all").await;
        assert!(entries.is_empty());
    }
}
