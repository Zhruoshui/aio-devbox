// Model-config routes — the mgr side of the config up-lift (sandbox-mgr
// Phase 4a, prd D6 / design §3.7).
//
// mgr is the SINGLE source of truth for the canonical model config. The
// stored artifact is a kv row (`models_config`) shaped
//   {"version": <u64, starts at 1, +1 per write>,
//    "config": <CanonicalConfig>}
// where `version` drives the sandbox-side pull (Phase 4b: the app compares
// versions before re-rendering agent configs; a plain ETag surrogate).
//
// Routes (contract-locked to app/src/routes/models/* — the mgr-web model page
// is a direct port of web/src/panes/models/, so any response-shape drift is
// a cross-layer break):
//   GET  /api/models/config    — masked CanonicalConfig (app get_config)
//   PUT  /api/models/config    — masked-echo merge + validate + version bump
//   GET  /api/models/sync      — {version, config} UNMASKED, sandbox pull
//   POST /api/models/import/pi — absorb ~/.pi/agent/models.json providers
//   POST /api/models/discover  — /v1/models endpoint probe (multi-shape)
//   POST /api/models/test      — minimal completion availability probe
//   GET  /api/models/catalog   — models.dev metadata proxy (1h cache)
//
// NOT ported (mgr semantics don't exist for them; Phase 4c adapts the UI):
// /api/models/agents, apply/:agent, live-provider edit/delete/sync, usage —
// those read/write files INSIDE one sandbox; mgr has no agent installs.
//
// Error shape: app answers `(StatusCode, String)` (plain-text bodies);
// mgr's house style is `{"error": "<msg>"}` JSON (routes.rs ApiError). The
// mgr-web api client decodes both shapes through apiError(), and the models
// page only checks r.ok + r.json()/r.text(), so the semantic contract
// (status code + human-readable message) is preserved either way.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use aio_models::store::{
    ensure_preset_ids, import_from_pi, merge_api_keys, mask_config, validate,
    CanonicalConfig, CostEntry, ImportResponse, PutResponse, StoreError,
};

use crate::db;
use crate::state::AppState;

/// kv row key holding the versioned canonical config (design §3.7).
const KV_MODELS: &str = "models_config";

// ── stored payload ────────────────────────────────────────────────

/// The kv row payload. `version` starts at 1 on the first write and grows
/// by one on every successful mutation (PUT config / import pi) so the
/// sandbox pull loop (Phase 4b) can skip no-op downloads.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct StoredModels {
    version: u64,
    config: CanonicalConfig,
}

/// Read the stored config; a missing kv row means "never configured" and
/// yields the default (app's read_config on a missing file behaves the
/// same). A row that fails to parse is an internal error, not a reset —
/// unlike a corrupt file this cannot happen through the write path (every
/// write serializes a CanonicalConfig we just validated), so surfacing it
/// loudly beats silently rewriting the user's config away.
fn read_stored(conn: &rusqlite::Connection) -> Result<StoredModels, ApiError> {
    match db::kv_get(conn, KV_MODELS)? {
        None => Ok(StoredModels { version: 0, config: CanonicalConfig::default() }),
        Some(text) => serde_json::from_str(&text).map_err(|e| {
            ApiError::internal(format!(
                "stored models_config is corrupt ({e}); kv key {KV_MODELS:?} in state.db"
            ))
        }),
    }
}

/// Serialize + write the kv row. Called with the db Mutex already held (the
/// whole read-merge-write happens inside one lock acquisition — no await
/// points, so the std Mutex discipline of state.rs holds and concurrent PUTs
/// serialize naturally, replacing app's models_lock).
fn write_stored(conn: &rusqlite::Connection, stored: &StoredModels) -> Result<(), ApiError> {
    let text = serde_json::to_string(stored)
        .map_err(|e| ApiError::internal(format!("serialize models_config: {e}")))?;
    db::kv_set(conn, KV_MODELS, &text)?;
    Ok(())
}

// ── error type (house style; mirrors routes.rs but local to keep the
// model routes self-contained like app's (StatusCode, String) tuples) ──

/// Handler error: status + `{"error": msg}` JSON (routes.rs ApiError shape).
/// Validation failures map to 400, upstream probe failures to 502/404 —
/// the same codes the app handlers emit for the same conditions.
#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad(msg: impl Into<String>) -> Self {
        ApiError { status: StatusCode::BAD_REQUEST, message: msg.into() }
    }

    fn internal(msg: impl Into<String>) -> Self {
        ApiError { status: StatusCode::INTERNAL_SERVER_ERROR, message: msg.into() }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

/// db.rs helpers return `anyhow::Result`; on this route group every such
/// failure is an internal error (no validation flows through them).
impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError::internal(format!("{e:#}"))
    }
}

// ── router ─────────────────────────────────────────────────────────

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/models/config", get(get_config).put(put_config))
        .route("/api/models/sync", get(sync))
        .route("/api/models/import/pi", post(import_pi))
        .route("/api/models/discover", post(discover))
        .route("/api/models/test", post(test))
        .route("/api/models/catalog", get(get_catalog))
}

// ── GET/PUT /api/models/config ────────────────────────────────────

/// GET /api/models/config — masked canonical config (app get_config: read
/// the store, mask every apiKey, return; never errors on missing config).
async fn get_config(State(state): State<Arc<AppState>>) -> Result<Json<CanonicalConfig>, ApiError> {
    let mut config = {
        let conn = state.db.lock().unwrap();
        read_stored(&conn)?.config
    };
    mask_config(&mut config);
    Ok(Json(config))
}

/// PUT /api/models/config — masked-echo merge + ensure_preset_ids +
/// validate + write + version bump (app put_config, step for step).
///
/// Serialization: the read-merge-write happens inside one db Mutex
/// acquisition with no await in between, so concurrent PUTs are naturally
/// serialized — the kv row replaces app's models.json + models_lock pair.
async fn put_config(
    State(state): State<Arc<AppState>>,
    Json(mut incoming): Json<CanonicalConfig>,
) -> Result<Json<PutResponse>, ApiError> {
    let guard = state.db.lock().unwrap();

    let stored = read_stored(&guard)?;
    let next_version = stored.version + 1;

    // Masked-echo merge: the frontend sends the mask back when a key is
    // unchanged ("" clears, absent keeps, other replaces — store.rs).
    merge_api_keys(&stored.config, &mut incoming);

    // Backend owns preset ids: backfill ones the frontend created blank.
    ensure_preset_ids(&mut incoming);

    validate(&incoming)
        .map_err(|errs| ApiError::bad(errs.join("; ")))?;

    // CanonicalConfig.version is the legacy file-format field (always 1);
    // the STORE-level version lives in the kv wrapper.
    incoming.version = 1;

    write_stored(&guard, &StoredModels { version: next_version, config: incoming })?;
    drop(guard); // explicit: nothing below may run under the db lock

    Ok(Json(PutResponse { ok: true, warnings: vec![] }))
}

// ── GET /api/models/sync ──────────────────────────────────────────

/// GET /api/models/sync — the sandbox pull endpoint (Phase 4b consumer).
/// Returns the UNMASKED `{version, config}`: the sandbox app writes the
/// plaintext config to its local canonical store and re-renders agent
/// files, which requires the real keys.
///
/// SECURITY BOUNDARY (D6/D9, deliberately accepted): this hands out
/// plaintext API keys without authentication. The trust boundary is the
/// host machine — mgr listens on :8089 which the mgr compose does NOT
/// publish to the host, and aio-mgr-net (the only network the sandbox
/// pull uses, via the `mgr-api` alias) is a host-local docker network
/// that never leaves the machine. Same reasoning as the auth-free
/// gateways (D9): personal single-host deployment.
async fn sync(State(state): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let stored = {
        let conn = state.db.lock().unwrap();
        read_stored(&conn)?
    };
    Ok(Json(json!({
        "version": stored.version,
        "config": stored.config,
    })))
}

// ── POST /api/models/import/pi ────────────────────────────────────

/// POST /api/models/import/pi — absorb pi's own models.json into the
/// canonical library (app import_pi). Path: `MGR_PI_MODELS_FILE` env, else
/// `$HOME/.pi/agent/models.json` (in the containerized form mgr has no
/// ~/.pi — that is EXPECTED; the route is useful in the bare-metal form
/// where mgr runs on the same host as the sandbox user).
async fn import_pi(State(state): State<Arc<AppState>>) -> Result<Json<ImportResponse>, ApiError> {
    let pi_path = pi_models_path();

    let guard = state.db.lock().unwrap();
    let stored = read_stored(&guard)?;
    let mut config = stored.config;

    let result = import_from_pi(&pi_path, &config).map_err(|e| match e {
        StoreError::Io(err) if err.kind() == std::io::ErrorKind::NotFound => {
            ApiError {
                status: StatusCode::NOT_FOUND,
                message: format!(
                    "pi models.json not found at {} (containerized mgr has no ~/.pi; \
                     set MGR_PI_MODELS_FILE or use the bare-metal form)",
                    pi_path.display()
                ),
            }
        }
        StoreError::Io(err) => {
            ApiError::internal(format!("read pi models.json: {err}"))
        }
        StoreError::Corrupt(err) => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            message: format!("pi models.json is corrupt: {err}"),
        },
    })?;

    for (id, provider) in result.providers {
        config.providers.insert(id, provider);
    }

    write_stored(&guard, &StoredModels { version: stored.version + 1, config })?;
    drop(guard);

    Ok(Json(ImportResponse {
        ok: true,
        imported: result.imported,
        skipped: result.skipped,
    }))
}

/// Resolve the pi models.json path (`MGR_PI_MODELS_FILE` overrides the
/// app-style `$HOME/.pi/agent/models.json` default).
fn pi_models_path() -> PathBuf {
    if let Ok(p) = std::env::var("MGR_PI_MODELS_FILE") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home).join(".pi/agent/models.json")
}

// ── provider resolution (shared by discover + test) ─────────────────

/// The effective fields a probe needs, resolved from the stored config
/// (ById) or the literal request body (transient provider being edited).
/// ById reads the REAL stored key — no mask semantics on the probe path
/// (app resolve_provider, design §5).
struct ResolvedProvider {
    base_url: String,
    api: String,
    api_key: Option<String>,
    headers: BTreeMap<String, String>,
}

/// Resolve a provider by id from the kv store, or accept literal fields.
/// `baseUrl` is validated non-empty on the literal branch exactly like
/// app's discover (test resolves by id only and 404s an unknown id).
fn resolve_provider(
    conn: &rusqlite::Connection,
    req: &DiscoverRequest,
) -> Result<ResolvedProvider, ApiError> {
    match req {
        DiscoverRequest::ById { providerId } => {
            let config = read_stored(conn)?.config;
            let p = config.providers.get(providerId).ok_or_else(|| ApiError {
                status: StatusCode::NOT_FOUND,
                message: format!("provider '{providerId}' not found"),
            })?;
            Ok(ResolvedProvider {
                base_url: p.base_url.clone(),
                api: p.api.clone(),
                api_key: p.api_key.clone(),
                headers: p.headers.clone(),
            })
        }
        DiscoverRequest::Literal { baseUrl, api, apiKey } => {
            if baseUrl.trim().is_empty() {
                return Err(ApiError::bad("baseUrl is required"));
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

// ── POST /api/models/discover ─────────────────────────────────────

/// Timeout per candidate request (app design §5: 20s).
const PER_CANDIDATE_TIMEOUT: Duration = Duration::from_secs(20);
/// Total budget across all candidates for one discover call (app §5: 20s).
const TOTAL_BUDGET: Duration = Duration::from_secs(20);

/// POST body: either a provider id (resolve from the store) or literal
/// endpoint fields (a provider being edited in the UI, not yet saved).
/// Untagged: `{providerId}` vs `{baseUrl, api?, apiKey?}` (app discover).
/// Field names ARE the wire contract (same non_snake_case allowance as
/// app's StatsSnapshot).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
#[allow(non_snake_case)]
enum DiscoverRequest {
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

/// One discovered model (app discover.rs DiscoveredModel).
#[derive(Debug, serde::Serialize)]
struct DiscoveredModel {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

#[derive(Debug, serde::Serialize)]
struct DiscoverResponse {
    models: Vec<DiscoveredModel>,
    endpoint: String,
}

/// POST /api/models/discover — fetch the /v1/models list (multi-candidate
/// URL fallback, protocol-adaptive headers, multi-shape parsing). Ported
/// from app/src/routes/models/discover.rs with the store swapped for the
/// kv truth source; URL/header/parse helpers are byte-for-byte the app's
/// (they are pure functions - unit tests live here).
async fn discover(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DiscoverRequest>,
) -> Result<Json<DiscoverResponse>, ApiError> {
    let resolved = {
        let conn = state.db.lock().unwrap();
        // The literal branch validates baseUrl inside (blank -> 400),
        // exactly like the app handler's resolve_provider.
        resolve_provider(&conn, &req)?
    };

    let candidates = candidate_urls(&resolved.base_url, &resolved.api);
    let headers = build_headers(&resolved.api, resolved.api_key.as_deref(), &resolved.headers);

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
                tracing::debug!(target: "mgr::models::discover", "candidate {url} transport err: {e}");
                continue;
            }
            Ok(r) => {
                let status = r.status();
                if status.is_success() {
                    let text = r.text().await.unwrap_or_default();
                    let models = parse_discovered_models(&text);
                    if !models.is_empty() {
                        return Ok(Json(DiscoverResponse { models, endpoint: url.clone() }));
                    }
                    // 2xx but no parseable models -> treat as exhaustion.
                    return Err(ApiError {
                        status: StatusCode::BAD_GATEWAY,
                        message: format!(
                            "no models parsed from {url} (body truncated: {})",
                            truncate(&text, 500)
                        ),
                    });
                }
                // 401/403 from the FIRST candidate short-circuits: the key
                // is wrong, fallback URLs would 401 too. From later
                // candidates it's just a skip - a gateway may expose the
                // list endpoint at a different path.
                if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
                    let body = r.text().await.unwrap_or_default();
                    let err = format!("{} {} :: {}", status.as_u16(), url, truncate(&body, 500));
                    if idx == 0 {
                        return Err(ApiError { status, message: err });
                    }
                    if first_auth_failure.is_none() {
                        first_auth_failure = Some((status, err));
                    }
                    continue;
                }
                tracing::debug!(target: "mgr::models::discover", "candidate {url} status {status}");
                continue;
            }
        }
    }

    if let Some((status, msg)) = first_auth_failure {
        return Err(ApiError { status, message: msg });
    }
    Err(ApiError {
        status: StatusCode::BAD_GATEWAY,
        message: format!("all candidates failed; tried: {}", tried.join(", ")),
    })
}

// ── URL derivation (pure; ported from app discover.rs) ─────────────

/// Strip a trailing path segment if it matches one of `suffixes`.
fn strip_trailing<'a>(base: &'a str, suffixes: &[&str]) -> &'a str {
    for s in suffixes {
        if let Some(stripped) = base.strip_suffix(s) {
            return stripped;
        }
    }
    base
}

/// The anthropic-style suffixes cc-switch strips before re-deriving.
const ANTHROPIC_SUFFIXES: &[&str] = &["/anthropic", "/claude", "/api/coding"];

/// Derive the primary models URL for a base URL + protocol.
fn primary_models_url(base: &str, api: &str) -> String {
    let base = base.trim_end_matches('/');
    if base.ends_with("/models") {
        return base.to_string();
    }
    match api {
        "anthropic-messages" => {
            // Insert /v1 only when the path is empty (pi-web behavior).
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

/// Build the ordered, deduped candidate URL list for a base + protocol.
fn candidate_urls(base: &str, api: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let primary = primary_models_url(base, api);
    out.push(primary.clone());

    let base_trim = base.trim_end_matches('/');

    let push_unique = |out: &mut Vec<String>, url: String| {
        if !out.iter().any(|u| u == &url) {
            out.push(url);
        }
    };

    push_unique(&mut out, format!("{base_trim}/v1/models"));
    push_unique(&mut out, format!("{base_trim}/models"));

    let stripped = strip_trailing(base_trim, ANTHROPIC_SUFFIXES);
    if stripped != base_trim {
        let re_primary = primary_models_url(stripped, api);
        push_unique(&mut out, re_primary);
        push_unique(&mut out, format!("{stripped}/v1/models"));
        push_unique(&mut out, format!("{stripped}/models"));
    }

    out
}

// ── header construction (pure; ported from app discover.rs) ─────────

/// Build the per-request headers for a discover/test call.
fn build_headers(
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
        let lower = k.to_ascii_lowercase();
        if let Some(slot) = h.iter_mut().find(|(n, _)| n.to_ascii_lowercase() == lower) {
            slot.1 = v.clone();
        } else {
            h.push((k.clone(), v.clone()));
        }
    }

    h
}

// ── response parsing (pure; ported from app discover.rs) ───────────

/// Parse a /v1/models response body into a deduped, naturally-sorted list
/// of model ids. Accepted shapes: bare array; object with array under
/// `data|models|results|items`; object-of-objects. `models/` prefix is
/// stripped (Gemini). Duplicates removed; natural sort.
fn parse_discovered_models(body: &str) -> Vec<DiscoveredModel> {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let raw_items = collect_items(&v);
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut out: Vec<DiscoveredModel> = Vec::new();

    for item in raw_items {
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
            (Some(k), Value::String(s)) => (k, Some(s)),
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

    out.sort_by(|a, b| natcmp(&a.id, &b.id));
    out
}

/// Collect the list of (optional key, item value) pairs from a parsed
/// response.
fn collect_items(v: &Value) -> Vec<(Option<String>, Value)> {
    match v {
        Value::Array(arr) => arr.iter().cloned().map(|x| (None, x)).collect(),
        Value::Object(o) => {
            for key in ["data", "models", "results", "items"] {
                if let Some(Value::Array(arr)) = o.get(key) {
                    return arr.iter().cloned().map(|x| (None, x)).collect();
                }
            }
            o.iter()
                .map(|(k, v)| (Some(k.clone()), v.clone()))
                .collect()
        }
        _ => Vec::new(),
    }
}

/// Truncate to `max` chars, appending "…" when truncated (char count, not
/// bytes).
pub(crate) fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max).collect();
        format!("{head}…")
    }
}

/// Natural comparison: digit runs compare numerically, the rest byte-wise.
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

// ── POST /api/models/test ─────────────────────────────────────────

/// Timeout for the minimal completion request (app §5: 20s).
const TEST_TIMEOUT: Duration = Duration::from_secs(20);
/// Max output tokens for the probe (pi-web §3).
const MAX_OUTPUT_TOKENS: u64 = 16;
/// The exact probe prompt (pi-web §3).
const PROBE_PROMPT: &str = "Reply with OK only.";
/// Truncation length for the response text snippet (pi-web §3: 300 chars).
const RESPONSE_TEXT_MAX: usize = 300;

/// POST /api/models/test body (app test.rs TestRequest; camelCase field
/// names are the wire contract).
#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
struct TestRequest {
    providerId: String,
    modelId: String,
    /// Override the provider's stored protocol; defaults to provider.api.
    #[serde(default)]
    protocol: Option<String>,
}

/// POST /api/models/test response (app TestResponse, camelCase).
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TestResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    latency_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_text: Option<String>,
}

/// POST /api/models/test — minimal completion probe. Ported from app
/// test.rs; the provider lookup reads the kv store instead of models.json.
async fn test(
    State(state): State<Arc<AppState>>,
    Json(req): Json<TestRequest>,
) -> Result<Json<TestResponse>, ApiError> {
    if req.providerId.trim().is_empty() || req.modelId.trim().is_empty() {
        return Err(ApiError::bad("providerId and modelId are required"));
    }

    let (provider, protocol) = {
        let conn = state.db.lock().unwrap();
        let config = read_stored(&conn)?.config;
        let provider = config.providers.get(&req.providerId).cloned().ok_or_else(|| {
            ApiError {
                status: StatusCode::NOT_FOUND,
                message: format!("provider '{}' not found", req.providerId),
            }
        })?;
        let protocol = req
            .protocol
            .clone()
            .unwrap_or_else(|| provider.api.clone());
        (provider, protocol)
    };

    // R1: the provider's baseUrl IS the endpoint for every protocol.
    let base_url = provider.base_url.clone();

    // No key => error, but still HTTP 200 with ok:false (UI decides).
    let key = provider.api_key.clone();
    if key.as_deref().is_none_or(|k| k.is_empty()) {
        return Ok(Json(TestResponse {
            ok: false,
            latency_ms: None,
            status: None,
            error: Some(format!("No API key found for \"{}\"", req.providerId)),
            response_text: None,
        }));
    }

    let headers = build_headers(&protocol, key.as_deref(), &provider.headers);
    let endpoint = completion_url(&base_url, &protocol);
    let body = completion_body(&req.modelId, &protocol);

    let start = Instant::now();
    let mut req_builder = state
        .http
        .post(&endpoint)
        .header("Content-Type", "application/json")
        .timeout(TEST_TIMEOUT);
    for (k, v) in &headers {
        req_builder = req_builder.header(k, v);
    }
    let resp = req_builder.json(&body).send().await;
    let latency_ms = start.elapsed().as_millis() as u64;

    match resp {
        Err(e) => {
            let msg = if e.is_timeout() {
                format!("timeout after {}s", TEST_TIMEOUT.as_secs())
            } else {
                e.to_string()
            };
            Ok(Json(TestResponse {
                ok: false,
                latency_ms: Some(latency_ms),
                status: None,
                error: Some(truncate(&msg, RESPONSE_TEXT_MAX)),
                response_text: None,
            }))
        }
        Ok(r) => {
            let status = r.status();
            let status_code = status.as_u16();
            let text = r.text().await.unwrap_or_default();
            if status.is_success() {
                let response_text = extract_response_text(&text, &protocol);
                Ok(Json(TestResponse {
                    ok: true,
                    latency_ms: Some(latency_ms),
                    status: Some(status_code),
                    error: None,
                    response_text: Some(truncate(&response_text, RESPONSE_TEXT_MAX)),
                }))
            } else {
                Ok(Json(TestResponse {
                    ok: false,
                    latency_ms: Some(latency_ms),
                    status: Some(status_code),
                    error: Some(truncate(&text, RESPONSE_TEXT_MAX)),
                    response_text: None,
                }))
            }
        }
    }
}

// ── completion URL/body/extraction (pure; ported from app test.rs) ──

/// Derive the completion endpoint URL for a protocol.
fn completion_url(base: &str, protocol: &str) -> String {
    let base = base.trim_end_matches('/');
    match protocol {
        "openai-completions" => format!("{base}/chat/completions"),
        "openai-responses" => format!("{base}/responses"),
        "anthropic-messages" => format!("{base}/v1/messages"),
        _ => format!("{base}/chat/completions"),
    }
}

/// Build the minimal completion request body for a protocol.
fn completion_body(model: &str, protocol: &str) -> Value {
    match protocol {
        "openai-completions" => json!({
            "model": model,
            "messages": [{"role":"user","content": PROBE_PROMPT}],
            "max_tokens": MAX_OUTPUT_TOKENS,
            "stream": false,
        }),
        "openai-responses" => json!({
            "model": model,
            "input": PROBE_PROMPT,
            "max_output_tokens": MAX_OUTPUT_TOKENS,
        }),
        "anthropic-messages" => json!({
            "model": model,
            "max_tokens": MAX_OUTPUT_TOKENS,
            "messages": [{"role":"user","content": PROBE_PROMPT}],
        }),
        _ => json!({
            "model": model,
            "messages": [{"role":"user","content": PROBE_PROMPT}],
            "max_tokens": MAX_OUTPUT_TOKENS,
            "stream": false,
        }),
    }
}

/// Extract the first text content from a completion response, leniently.
fn extract_response_text(body: &str, protocol: &str) -> String {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };
    match protocol {
        "openai-completions" => v
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|t| t.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default(),
        "openai-responses" => {
            if let Some(s) = v.get("output_text").and_then(|t| t.as_str()) {
                return s.to_string();
            }
            v.get("output")
                .and_then(|o| o.as_array())
                .and_then(|arr| {
                    arr.iter().find_map(|item| {
                        item.get("content")
                            .and_then(|c| c.as_array())
                            .and_then(|c| {
                                c.iter().find_map(|ci| {
                                    if ci.get("type").and_then(|t| t.as_str()) == Some("text") {
                                        ci.get("text").and_then(|t| t.as_str()).map(String::from)
                                    } else {
                                        None
                                    }
                                })
                            })
                    })
                })
                .unwrap_or_default()
        }
        "anthropic-messages" => v
            .get("content")
            .and_then(|c| c.as_array())
            .and_then(|arr| {
                arr.iter().find_map(|ci| {
                    if ci.get("type").and_then(|t| t.as_str()) == Some("text") {
                        ci.get("text").and_then(|t| t.as_str()).map(String::from)
                    } else {
                        None
                    }
                })
            })
            .unwrap_or_default(),
        _ => v
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|t| t.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default(),
    }
}

// ── GET /api/models/catalog ────────────────────────────────────────

const CATALOG_URL: &str = "https://models.dev/api.json";
const FETCH_TIMEOUT: Duration = Duration::from_secs(15);
const CATALOG_TTL: Duration = Duration::from_secs(3600);

// Response types mirror app/src/routes/models/catalog.rs verbatim (typed
// structs, NOT json! Values): serde's skip_serializing_if drops absent
// fields, so the wire shape matches the app byte-for-byte — a json!-built
// object would emit explicit nulls the frontend decode would trip on.

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogModel {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_window: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cost: Option<CostEntry>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogProvider {
    id: String,
    name: String,
    /// Official API base URL from models.dev (absent on ~1/4 of providers).
    /// The frontend builds a host→provider index from this to match a
    /// user's baseUrl data-drivenly instead of a hardcoded host table.
    #[serde(skip_serializing_if = "Option::is_none")]
    api: Option<String>,
    models: Vec<CatalogModel>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogResponse {
    providers: Vec<CatalogProvider>,
    fetched_at: String,
}

/// Cache entry; the fetch happens while holding the tokio Mutex so
/// concurrent requests queue behind the in-flight fetch (in-flight dedup,
/// app catalog.rs).
struct CatalogCache {
    at: Instant,
    data: CatalogResponse,
}

static CATALOG_CACHE: std::sync::OnceLock<tokio::sync::Mutex<Option<CatalogCache>>> =
    std::sync::OnceLock::new();

fn catalog_cache() -> &'static tokio::sync::Mutex<Option<CatalogCache>> {
    CATALOG_CACHE.get_or_init(|| tokio::sync::Mutex::new(None))
}

/// GET /api/models/catalog — models.dev proxy with 1h cache. Ported from
/// app catalog.rs (normalize + cache discipline unchanged).
async fn get_catalog(
    State(state): State<Arc<AppState>>,
) -> Result<Json<CatalogResponse>, ApiError> {
    let mut guard = catalog_cache().lock().await;
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
        .map_err(|e| ApiError {
            status: StatusCode::BAD_GATEWAY,
            message: format!("catalog fetch failed: {e}"),
        })?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(ApiError {
            status: StatusCode::BAD_GATEWAY,
            message: format!("catalog upstream {status}: {}", truncate(&text, 500)),
        });
    }

    let raw: Value = serde_json::from_str(&text).map_err(|e| ApiError {
        status: StatusCode::BAD_GATEWAY,
        message: format!("catalog parse failed: {e} (body: {})", truncate(&text, 500)),
    })?;

    let data = normalize_catalog(&raw);
    *guard = Some(CatalogCache { at: Instant::now(), data: data.clone() });
    Ok(Json(data))
}

/// models.dev's `api.json` normalization (app catalog.rs: every field
/// lookup fallible, never panics on upstream shape drift). `fetched_at`
/// is left empty — the app leaves it empty too (not required by AC).
fn normalize_catalog(raw: &Value) -> CatalogResponse {
    let mut providers = Vec::new();
    if let Some(obj) = raw.as_object() {
        for (provider_id, pv) in obj {
            let name = pv
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(provider_id)
                .to_string();
            let api = pv.get("api").and_then(Value::as_str).map(String::from);
            let mut models = Vec::new();
            if let Some(mobj) = pv.get("models").and_then(Value::as_object) {
                for (model_id, mv) in mobj {
                    models.push(normalize_model(model_id, mv));
                }
            }
            providers.push(CatalogProvider { id: provider_id.clone(), name, api, models });
        }
    }
    providers.sort_by(|a, b| a.id.cmp(&b.id));
    CatalogResponse { providers, fetched_at: String::new() }
}

/// One model node (app catalog.rs normalize_model: every lookup fallible).
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

// ── tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use aio_models::store::{mask_key, ProviderEntry};
    use serde_json::json;

    fn mem_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT);",
        )
        .expect("init kv schema");
        conn
    }

    fn sample_provider(key: &str) -> ProviderEntry {
        ProviderEntry {
            name: "Sample".into(),
            base_url: "https://api.example.com/v1".into(),
            api: "openai-completions".into(),
            api_key: Some(key.into()),
            ..ProviderEntry::default()
        }
    }

    fn sample_config(key: &str) -> CanonicalConfig {
        let mut c = CanonicalConfig::default();
        c.providers.insert("sample".into(), sample_provider(key));
        c
    }

    // --- kv round-trip + versioning ---

    #[test]
    fn stored_roundtrip_preserves_plaintext_key() {
        let conn = mem_db();
        // First read on an empty db: default config, version 0 (never written).
        let first = read_stored(&conn).unwrap();
        assert_eq!(first.version, 0);
        assert!(first.config.providers.is_empty());

        write_stored(&conn, &StoredModels { version: 3, config: sample_config("sk-plaintext-secret") })
            .unwrap();
        let back = read_stored(&conn).unwrap();
        assert_eq!(back.version, 3);
        assert_eq!(
            back.config.providers.get("sample").unwrap().api_key.as_deref(),
            Some("sk-plaintext-secret"),
            "kv stores the plaintext; masking happens only on the GET path"
        );
    }

    #[test]
    fn put_semantics_version_increments_each_write() {
        // put_config writes `stored.version + 1`; first write lands at 1.
        // (The handler itself is async/axum-bound; the store-level version
        // arithmetic is the invariant under test.)
        let conn = mem_db();
        let mut v = 0;
        let mut config = sample_config("sk-key-12345678");
        for _ in 0..3 {
            v += 1;
            write_stored(&conn, &StoredModels { version: v, config: config.clone() }).unwrap();
        }
        assert_eq!(read_stored(&conn).unwrap().version, 3);
        v += 1;
        config.providers.remove("sample");
        write_stored(&conn, &StoredModels { version: v, config }).unwrap();
        let back = read_stored(&conn).unwrap();
        assert_eq!(back.version, 4);
        assert!(back.config.providers.is_empty());
    }

    #[test]
    fn put_masked_echo_merge_keeps_plaintext_in_store() {
        // The masked-echo contract: the frontend echoes the mask back; the
        // STORED key must remain the plaintext (merge_api_keys, ported
        // semantics - the mgr PUT handler runs the same three calls).
        let conn = mem_db();
        let stored = sample_config("sk-key-12345678");
        write_stored(&conn, &StoredModels { version: 1, config: stored.clone() }).unwrap();

        // Incoming: same provider, apiKey = the MASK (frontend echo).
        let mut incoming = sample_config(&mask_key("sk-key-12345678"));
        merge_api_keys(&stored, &mut incoming);
        assert_eq!(
            incoming.providers.get("sample").unwrap().api_key.as_deref(),
            Some("sk-key-12345678"),
            "mask echo restores the stored plaintext key"
        );

        // "" clears.
        let mut incoming = sample_config("");
        merge_api_keys(&stored, &mut incoming);
        assert_eq!(incoming.providers.get("sample").unwrap().api_key, None);

        // Absent keeps.
        let mut incoming = sample_config("");
        incoming.providers.get_mut("sample").unwrap().api_key = None;
        merge_api_keys(&stored, &mut incoming);
        assert_eq!(
            incoming.providers.get("sample").unwrap().api_key.as_deref(),
            Some("sk-key-12345678")
        );

        // Other replaces.
        let mut incoming = sample_config("sk-other");
        merge_api_keys(&stored, &mut incoming);
        assert_eq!(
            incoming.providers.get("sample").unwrap().api_key.as_deref(),
            Some("sk-other")
        );
    }

    #[test]
    fn corrupt_kv_row_is_an_internal_error_not_a_reset() {
        let conn = mem_db();
        db::kv_set(&conn, KV_MODELS, "not json{{").unwrap();
        let err = read_stored(&conn).unwrap_err();
        assert_eq!(err.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(err.message.contains("corrupt"));
        // The row is untouched — surfacing must never rewrite user config.
        assert_eq!(db::kv_get(&conn, KV_MODELS).unwrap().as_deref(), Some("not json{{"));
    }

    #[test]
    fn ensure_preset_ids_backfills_on_put_path() {
        // put_config runs ensure_preset_ids between merge and validate; a
        // preset created with an empty id must get one (backend owns ids).
        let mut config = CanonicalConfig::default();
        config.providers.insert("sample".into(), sample_provider("sk"));
        let mut presets = aio_models::store::ClaudePresets::default();
        presets.presets.push(aio_models::store::ClaudePreset {
            id: String::new(),
            name: "P".into(),
            provider: "sample".into(),
            model: "m1".into(),
            ..Default::default()
        });
        presets.current = Some(String::new());
        config.agents.claude = Some(presets);

        ensure_preset_ids(&mut config);
        let p = config.agents.claude.as_ref().unwrap();
        assert!(!p.presets[0].id.is_empty(), "empty preset id backfilled");
        assert_eq!(p.current.as_deref(), Some(p.presets[0].id.as_str()));
    }

    // --- handler-level contract locks (cross-layer seams) -----------
    //
    // The store-level tests above lock the pieces; these call the handlers
    // directly (State + Json, same pattern as app's models/mod.rs tests) to
    // lock the WIRE assembly: GET masking, the sync payload shape the
    // sandbox pull decodes, and the PUT merge→validate→version-bump order.

    fn mgr_state() -> Arc<AppState> {
        Arc::new(AppState::new_for_test())
    }

    #[tokio::test]
    async fn get_config_handler_returns_masked_keys() {
        // GET /api/models/config mask contract (app get_config parity): the
        // stored plaintext never reaches the wire on this route.
        let state = mgr_state();
        {
            let conn = state.db.lock().unwrap();
            write_stored(
                &conn,
                &StoredModels { version: 2, config: sample_config("sk-plaintext-secret") },
            )
            .unwrap();
        }
        let Json(cfg) = get_config(State(state)).await.unwrap();
        let key = cfg.providers.get("sample").unwrap().api_key.clone().unwrap();
        assert_ne!(key, "sk-plaintext-secret");
        assert!(key.contains("****"), "masked shape, got {key}");
    }

    #[tokio::test]
    async fn sync_handler_shape_is_version_plus_unmasked_config() {
        // Wire contract with app/src/mgr_sync.rs SyncPayload {version,
        // config}: exactly these two fields, camelCase provider fields, and
        // the REAL key — the sandbox renders native files from this payload.
        let state = mgr_state();
        {
            let conn = state.db.lock().unwrap();
            write_stored(
                &conn,
                &StoredModels { version: 7, config: sample_config("sk-real-key") },
            )
            .unwrap();
        }
        let Json(v) = sync(State(state)).await.unwrap();
        let obj = v.as_object().expect("sync payload is an object");
        assert_eq!(obj.len(), 2, "exactly {{version, config}}: {obj:?}");
        assert_eq!(v["version"], 7);
        assert_eq!(v["config"]["providers"]["sample"]["apiKey"], "sk-real-key");
    }

    #[tokio::test]
    async fn put_config_handler_merge_validate_and_version_bump() {
        // Handler assembly (app put_config parity): masked-echo merge keeps
        // the stored plaintext, validate gates the write, and the kv version
        // bumps only on success.
        let state = mgr_state();
        {
            let conn = state.db.lock().unwrap();
            write_stored(
                &conn,
                &StoredModels { version: 4, config: sample_config("sk-key-12345678") },
            )
            .unwrap();
        }

        // Invalid PUT (assignment references an unknown provider): 400, and
        // the stored version/config are untouched.
        let mut bad = CanonicalConfig::default();
        bad.agents.pi = Some(aio_models::store::AgentAssignment {
            provider: "nope".into(),
            model: "m".into(),
        });
        let err = put_config(State(state.clone()), Json(bad)).await.unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert!(
            err.message.contains("unknown provider"),
            "validate error surfaced: {}",
            err.message
        );
        {
            let conn = state.db.lock().unwrap();
            assert_eq!(read_stored(&conn).unwrap().version, 4,
                "failed PUT must not bump the version");
        }

        // Valid masked-echo PUT: plaintext survives the merge, version 4→5.
        let incoming = sample_config(&mask_key("sk-key-12345678"));
        let Json(resp) = put_config(State(state.clone()), Json(incoming)).await.unwrap();
        assert!(resp.ok);
        {
            let conn = state.db.lock().unwrap();
            let stored = read_stored(&conn).unwrap();
            assert_eq!(stored.version, 5);
            assert_eq!(
                stored.config.providers.get("sample").unwrap().api_key.as_deref(),
                Some("sk-key-12345678"),
                "mask echo restores the plaintext on the handler path"
            );
        }
    }

    // --- discover pure helpers (app parity) ---

    #[test]
    fn candidate_urls_openai_shapes() {
        let urls = candidate_urls("https://api.openai.com", "openai-completions");
        assert_eq!(urls[0], "https://api.openai.com/models");
        assert!(urls.iter().any(|u| u == "https://api.openai.com/v1/models"));

        let urls = candidate_urls("https://api.openai.com/v1", "openai-completions");
        assert_eq!(urls[0], "https://api.openai.com/v1/models");
        assert_eq!(urls.len(), 2); // primary + bare /models
    }

    #[test]
    fn candidate_urls_anthropic_inserts_v1() {
        let urls = candidate_urls("https://api.anthropic.com", "anthropic-messages");
        assert_eq!(urls[0], "https://api.anthropic.com/v1/models?limit=1000");
    }

    #[test]
    fn candidate_urls_dedupe() {
        let urls = candidate_urls("https://api.example.com/v1", "openai-completions");
        let mut seen = std::collections::HashSet::new();
        for u in &urls {
            assert!(seen.insert(u.clone()), "duplicate: {u}");
        }
    }

    #[test]
    fn build_headers_by_protocol() {
        let h = build_headers("openai-completions", Some("sk-test1234"), &BTreeMap::new());
        let auth = h.iter().find(|(n, _)| n.eq_ignore_ascii_case("authorization"));
        assert_eq!(auth.map(|(_, v)| v.as_str()), Some("Bearer sk-test1234"));

        let h = build_headers("anthropic-messages", Some("sk-ant-xx"), &BTreeMap::new());
        assert!(h.iter().any(|(n, v)| n == "anthropic-version" && v == "2023-06-01"));
        assert!(h.iter().all(|(n, _)| !n.eq_ignore_ascii_case("authorization")));

        let h = build_headers("openai-completions", None, &BTreeMap::new());
        assert!(h.iter().all(|(n, _)| !n.eq_ignore_ascii_case("authorization")));
    }

    #[test]
    fn parse_discovered_models_shapes() {
        let m = parse_discovered_models(r#"{"data":[{"id":"gpt-4"},{"id":"gpt-3.5-turbo"}]}"#);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].id, "gpt-3.5-turbo"); // natural sort

        let m = parse_discovered_models(r#"{"model-a":{"name":"A"},"model-b":{}}"#);
        assert_eq!(m.len(), 2);

        let m = parse_discovered_models(r#"["models/gemini-1.5-pro"]"#);
        assert_eq!(m[0].id, "gemini-1.5-pro");

        assert!(parse_discovered_models("not json").is_empty());
        assert!(parse_discovered_models(r#""hello""#).is_empty());
    }

    // --- test-route pure helpers (app parity) ---

    #[test]
    fn completion_url_and_body() {
        assert_eq!(
            completion_url("https://api.x.com/v1/", "openai-completions"),
            "https://api.x.com/v1/chat/completions"
        );
        assert_eq!(
            completion_url("https://api.anthropic.com", "anthropic-messages"),
            "https://api.anthropic.com/v1/messages"
        );
        let b = completion_body("gpt-4", "openai-completions");
        assert_eq!(b["max_tokens"], 16);
        assert_eq!(b["messages"][0]["content"], PROBE_PROMPT);
    }

    #[test]
    fn extract_response_text_variants() {
        assert_eq!(
            extract_response_text(r#"{"choices":[{"message":{"content":"OK"}}]}"#, "openai-completions"),
            "OK"
        );
        assert_eq!(extract_response_text(r#"{"output_text":"OK"}"#, "openai-responses"), "OK");
        assert_eq!(
            extract_response_text(r#"{"content":[{"type":"text","text":"OK"}]}"#, "anthropic-messages"),
            "OK"
        );
        assert_eq!(extract_response_text("not json", "openai-completions"), "");
    }

    // --- catalog normalization ---

    #[test]
    fn normalize_catalog_maps_and_tolerates_drift() {
        let raw = json!({
            "openai": {
                "name": "OpenAI",
                "api": "https://api.openai.com/v1",
                "models": {
                    "gpt-x": {
                        "name": "GPT X",
                        "reasoning": true,
                        "modalities": {"input": ["text"]},
                        "limit": {"context": 200000, "output": 8192},
                        "cost": {"input": 5.0, "output": 15.0, "cache_read": 0.5, "cache_write": 6.25}
                    }
                }
            },
            "bare": {"models": {"m1": {}}}
        });
        let out = normalize_catalog(&raw);
        assert_eq!(out.providers.len(), 2);
        let p = out.providers.iter().find(|p| p.id == "openai").unwrap();
        assert_eq!(p.name, "OpenAI");
        assert_eq!(p.api.as_deref(), Some("https://api.openai.com/v1"));
        assert_eq!(p.models.len(), 1);
        let m = &p.models[0];
        assert_eq!(m.id, "gpt-x");
        assert_eq!(m.name.as_deref(), Some("GPT X"));
        assert_eq!(m.reasoning, Some(true));
        assert_eq!(m.context_window, Some(200000));
        assert_eq!(m.max_tokens, Some(8192));
        let c = m.cost.as_ref().unwrap();
        assert_eq!(c.input, Some(5.0));
        assert_eq!(c.output, Some(15.0));
        assert_eq!(c.cache_read, Some(0.5));
        assert_eq!(c.cache_write, Some(6.25));

        let bare = out.providers.iter().find(|p| p.id == "bare").unwrap();
        assert_eq!(bare.models.len(), 1);
        assert!(bare.models[0].name.is_none());
        assert!(bare.api.is_none());

        // Non-object top level: empty, no panic.
        assert!(normalize_catalog(&json!([1, 2, 3])).providers.is_empty());
    }

    #[test]
    fn catalog_serialization_matches_app_shape() {
        // Wire-shape lock: absent fields are OMITTED (skip_serializing_if),
        // cost maps kebab → camelCase — the exact contract the ported
        // mgr-web catalog decoder (types.ts catalogRecommend) consumes.
        let raw = json!({ "p": { "models": { "m": {} } } });
        let out = normalize_catalog(&raw);
        let text = serde_json::to_value(&out).unwrap();
        let m = &text["providers"][0]["models"][0];
        assert!(m.get("name").is_none(), "absent name omitted, not null");
        assert!(m.get("reasoning").is_none());
        assert!(m.get("cost").is_none());

        let raw = json!({ "p": { "models": { "m": {
            "cost": {"input": 1.0, "output": 2.0, "cache_read": 0.1, "cache_write": 0.2}
        } } } });
        let out = normalize_catalog(&raw);
        let text = serde_json::to_value(&out).unwrap();
        let cost = &text["providers"][0]["models"][0]["cost"];
        assert_eq!(cost["cacheRead"], 0.1);
        assert_eq!(cost["cacheWrite"], 0.2);
        assert_eq!(text["fetchedAt"], "");
    }

    // --- pi import path ---

    #[test]
    fn pi_models_path_env_override() {
        // Env-var-dependent; guard by capturing and restoring is fragile in
        // parallel tests, so only assert the fallback shape (no env set in
        // the test runner).
        if std::env::var("MGR_PI_MODELS_FILE").is_err() {
            let p = pi_models_path();
            assert!(p.ends_with(".pi/agent/models.json"), "default is $HOME-relative");
        }
    }

    #[test]
    fn truncate_shapes() {
        assert_eq!(truncate("abc", 10), "abc");
        let t = truncate(&"a".repeat(600), 500);
        assert!(t.ends_with('…'));
        assert_eq!(t.chars().count(), 501);
    }
}
