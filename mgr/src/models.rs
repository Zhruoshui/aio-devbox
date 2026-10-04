// Model-config routes — the mgr side of the cc-switch refactor
// (task 10-04-model-config-cc-switch, Phase 1.2 / Phase 3).
//
// mgr keeps ONLY the provider library: a shared TEMPLATE source that sandboxes
// pull from when configuring their own agents (方案A: 模板复制, 拷贝后独立).
// There is no profile system, no sandbox assignment, no canonical-config
// up-lift — each sandbox owns its agent configs end-to-end (prd D1/D2).
//
// The stored artifact is a kv row (`models_library`) shaped
//   {"version": <u32>, "providers": { "<id>": ProviderTemplate, ... }}
// (aio_models::template::ProviderLibrary — the Phase 1.1 data model).
//
// Routes:
//   GET    /api/models/providers        — the masked provider library
//   PUT    /api/models/providers        — masked-echo merge + replace-all
//   POST   /api/models/providers        — add one provider (body = template)
//   PUT    /api/models/providers/:id    — update one provider
//   DELETE /api/models/providers/:id    — remove one provider
//   GET    /api/models/templates        — public template list (masked) for
//                                          sandbox-side pickers
//   POST   /api/models/discover         — provider probe (ById or Literal)
//   POST   /api/models/test             — minimal completion probe
//
// REMOVED in this refactor (prd "废弃的功能"):
//   - /api/models/profiles + /api/models/profiles/:id  (profile system)
//   - /api/models/config (?profile=)                   (mgr-side canonical)
//   - /api/models/sync                                 (sandbox pull)
//   - /api/models/import/pi                            (pi import)
//   - /api/models/catalog                              (models.dev proxy)
//   - PUT /api/sandboxes/:name/model_profile           (sandbox assignment)
//
// discover/test survive unchanged: they are pure HTTP probes (no profile
// semantics), and the sandbox-side provider editor needs them to test an
// endpoint before saving. ById resolves against the provider library;
// Literal probes transient fields.
//
// Error shape: `{"error": "<msg>"}` JSON (house style, routes.rs ApiError).
// The mgr-web api client decodes both shapes through apiError(), so the
// semantic contract (status code + human-readable message) is preserved.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use aio_models::template::{
    mask_api_key, mask_library, ProviderLibrary, ProviderTemplate,
};

use crate::db;
use crate::state::AppState;

/// kv row key holding the provider library (cc-switch Phase 1.2).
const KV_LIBRARY: &str = "models_library";
/// The LEGACY multi-profile key (unified Phase 4). Read-only (migration
/// input); never written once the new key exists. The migration path is the
/// same as the legacy `models_config` row: we just extract its providers
/// into the new library shape and drop everything else.
const KV_MODELS_LEGACY: &str = "models_profiles";

// ── error type (house style; mirrors routes.rs but local) ─────────

/// Handler error: status + `{"error": msg}` JSON (routes.rs ApiError shape).
/// Validation failures map to 400, upstream probe failures to 502/404 —
/// the same codes the app handlers emit for the same conditions.
///
/// `pub(crate)` fields: routes.rs consumes the library helpers (which return
/// this type) through `ApiError::with_status` — same JSON shape on the wire
/// either way.
#[derive(Debug)]
pub(crate) struct ApiError {
    pub(crate) status: StatusCode,
    pub(crate) message: String,
}

impl ApiError {
    fn bad(msg: impl Into<String>) -> Self {
        ApiError {
            status: StatusCode::BAD_REQUEST,
            message: msg.into(),
        }
    }

    fn internal(msg: impl Into<String>) -> Self {
        ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: msg.into(),
        }
    }

    fn not_found(msg: impl Into<String>) -> Self {
        ApiError {
            status: StatusCode::NOT_FOUND,
            message: msg.into(),
        }
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

// ── library store ─────────────────────────────────────────────────

/// Read the provider library, running the legacy migration when needed.
///
/// - new key present → parse it (corrupt = internal error, never a reset —
///   every write serialized a validated library, so corruption means something
///   external touched the db);
/// - new key missing + legacy key present → MIGRATE: extract the providers
///   from the legacy single-config row's first profile and write the new key,
///   delete the old one (the old content survives until the new write
///   succeeded — a crash in between just re-runs this next boot);
/// - neither key → fresh empty library (in-memory only; nothing is written
///   until a mutation happens).
fn read_library(conn: &rusqlite::Connection) -> Result<ProviderLibrary, ApiError> {
    if let Some(text) = db::kv_get(conn, KV_LIBRARY)? {
        return serde_json::from_str(&text).map_err(|e| {
            ApiError::internal(format!(
                "stored {KV_LIBRARY} is corrupt ({e}); kv key {KV_LIBRARY:?} in state.db"
            ))
        });
    }
    if let Some(legacy_text) = db::kv_get(conn, KV_MODELS_LEGACY)? {
        // Legacy row shape (unified Phase 4):
        //   {"version": u64, "profiles": [{"id","name","config": CanonicalConfig}],
        //    "assignments": {...}}
        // We only need the providers of the FIRST profile (the migration
        // target); everything else (profiles themselves, assignments) is
        // dropped by design (the sandbox owns its config now).
        #[derive(serde::Deserialize)]
        struct LegacyProfile {
            config: LegacyConfig,
        }
        #[derive(serde::Deserialize)]
        struct LegacyConfig {
            providers: BTreeMap<String, LegacyProvider>,
        }
        #[derive(serde::Deserialize)]
        struct LegacyProvider {
            name: String,
            #[serde(rename = "baseUrl")]
            base_url: String,
            api: String,
            #[serde(default, rename = "apiKey")]
            api_key: Option<String>,
            #[serde(default)]
            headers: BTreeMap<String, String>,
            #[serde(default)]
            models: Vec<LegacyModel>,
        }
        #[derive(serde::Deserialize)]
        struct LegacyModel {
            id: String,
            #[serde(default)]
            name: Option<String>,
            #[serde(default)]
            reasoning: Option<bool>,
            #[serde(default, rename = "contextWindow")]
            context_window: Option<u64>,
            #[serde(default, rename = "maxTokens")]
            max_tokens: Option<u64>,
        }
        #[derive(serde::Deserialize)]
        struct LegacyRow {
            profiles: Vec<LegacyProfile>,
        }
        let legacy: LegacyRow = serde_json::from_str(&legacy_text).map_err(|e| {
            ApiError::internal(format!(
                "stored {KV_MODELS_LEGACY} is corrupt ({e}); kv key {KV_MODELS_LEGACY:?} in state.db"
            ))
        })?;
        let mut library = ProviderLibrary::default();
        if let Some(first) = legacy.profiles.first() {
            for (id, p) in &first.config.providers {
                library.providers.insert(
                    id.clone(),
                    ProviderTemplate {
                        name: p.name.clone(),
                        base_url: p.base_url.clone(),
                        api: p.api.clone(),
                        api_key: p.api_key.clone(),
                        headers: p.headers.clone(),
                        models: p
                            .models
                            .iter()
                            .map(|m| aio_models::template::TemplateModelEntry {
                                id: m.id.clone(),
                                name: m.name.clone(),
                                reasoning: m.reasoning,
                                context_window: m.context_window,
                                max_tokens: m.max_tokens,
                                cost: None,
                            })
                            .collect(),
                        website_url: None,
                        category: None,
                        notes: None,
                    },
                );
            }
        }
        write_library(conn, &library)?;
        db::kv_del(conn, KV_MODELS_LEGACY)?;
        tracing::info!(
            "migrated legacy {KV_MODELS_LEGACY} -> {KV_LIBRARY} ({} provider(s) carried over)",
            library.providers.len()
        );
        return Ok(library);
    }
    // The even-older single-config key (Phase 4a). Same migration target.
    if let Some(legacy_text) = db::kv_get(conn, "models_config")? {
        #[derive(serde::Deserialize)]
        struct LegacyRow {
            config: LegacyConfig2,
        }
        #[derive(serde::Deserialize)]
        struct LegacyConfig2 {
            providers: BTreeMap<String, LegacyProvider2>,
        }
        #[derive(serde::Deserialize)]
        struct LegacyProvider2 {
            name: String,
            #[serde(rename = "baseUrl")]
            base_url: String,
            api: String,
            #[serde(default, rename = "apiKey")]
            api_key: Option<String>,
        }
        let legacy: LegacyRow = serde_json::from_str(&legacy_text).map_err(|e| {
            ApiError::internal(format!(
                "stored models_config is corrupt ({e}); kv key \"models_config\" in state.db"
            ))
        })?;
        let mut library = ProviderLibrary::default();
        for (id, p) in &legacy.config.providers {
            library.providers.insert(
                id.clone(),
                ProviderTemplate {
                    name: p.name.clone(),
                    base_url: p.base_url.clone(),
                    api: p.api.clone(),
                    api_key: p.api_key.clone(),
                    ..Default::default()
                },
            );
        }
        write_library(conn, &library)?;
        db::kv_del(conn, "models_config")?;
        tracing::info!(
            "migrated legacy models_config -> {KV_LIBRARY} ({} provider(s) carried over)",
            library.providers.len()
        );
        return Ok(library);
    }
    Ok(ProviderLibrary::default())
}

/// Serialize + write the kv row. Called with the db Mutex already held (the
/// whole read-merge-write happens inside one lock acquisition — no await
/// points, so the std Mutex discipline of state.rs holds and concurrent PUTs
/// serialize naturally).
fn write_library(conn: &rusqlite::Connection, library: &ProviderLibrary) -> Result<(), ApiError> {
    let text = serde_json::to_string(library)
        .map_err(|e| ApiError::internal(format!("serialize {KV_LIBRARY}: {e}")))?;
    db::kv_set(conn, KV_LIBRARY, &text)?;
    Ok(())
}

/// Merge the incoming (possibly mask-echoed) library into the stored one:
/// for each provider, `apiKey == Some(mask)` (i.e. the frontend echoed the
/// mask back) restores the STORED plaintext; `Some("")` clears; absent
/// (None) keeps the STORED plaintext too (the canonical semantic of "the
/// frontend didn't touch the key field"). Mirrors the old CanonicalConfig
/// merge_api_keys semantics at the library level.
fn merge_api_keys(stored: &ProviderLibrary, incoming: &mut ProviderLibrary) {
    for (id, template) in incoming.providers.iter_mut() {
        let stored_key = stored
            .providers
            .get(id)
            .and_then(|p| p.api_key.as_deref());
        let Some(new_key) = template.api_key.clone() else {
            // Absent keeps: the frontend didn't send an apiKey field, so
            // carry the stored plaintext forward.
            template.api_key = stored_key.map(str::to_string);
            continue;
        };
        if new_key.is_empty() {
            template.api_key = None; // "" clears
            continue;
        }
        let is_mask_echo = stored_key
            .map(|k| mask_api_key(k) == new_key)
            .unwrap_or(false);
        template.api_key = if is_mask_echo {
            stored_key.map(str::to_string) // restore plaintext
        } else {
            Some(new_key) // explicit new key
        };
    }
}

/// Validate a provider library before persisting: every provider must have
/// a non-empty name, a non-empty baseUrl, and a non-blank api.
fn validate_library(library: &ProviderLibrary) -> Result<(), ApiError> {
    for (id, p) in &library.providers {
        if p.name.trim().is_empty() {
            return Err(ApiError::bad(format!("provider {id:?}: name is required")));
        }
        if p.base_url.trim().is_empty() {
            return Err(ApiError::bad(format!("provider {id:?}: baseUrl is required")));
        }
        if p.api.trim().is_empty() {
            return Err(ApiError::bad(format!("provider {id:?}: api is required")));
        }
        for m in &p.models {
            if m.id.trim().is_empty() {
                return Err(ApiError::bad(format!(
                    "provider {id:?}: model id must not be empty"
                )));
            }
        }
    }
    Ok(())
}

// ── router ────────────────────────────────────────────────────────

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/api/models/providers",
            get(get_providers).put(put_providers).post(post_provider_v2),
        )
        .route(
            "/api/models/providers/:id",
            put(put_provider).delete(delete_provider),
        )
        .route("/api/models/templates", get(get_templates))
        .route("/api/models/discover", post(discover))
        .route("/api/models/test", post(test))
}

// ── GET/PUT/POST /api/models/providers ────────────────────────────

/// GET /api/models/providers — the masked provider library (the plaintext
/// apiKey never leaves the mgr on this route; POST/PUT echo the mask back).
async fn get_providers(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ProviderLibrary>, ApiError> {
    let mut library = {
        let conn = state.db.lock().unwrap();
        read_library(&conn)?
    };
    mask_library(&mut library);
    Ok(Json(library))
}

/// PUT /api/models/providers — masked-echo merge + validate + write (whole
/// library replace; the frontend edits the full grid and saves in one shot).
async fn put_providers(
    State(state): State<Arc<AppState>>,
    Json(mut incoming): Json<ProviderLibrary>,
) -> Result<Json<Value>, ApiError> {
    let guard = state.db.lock().unwrap();
    let stored = read_library(&guard)?;
    let next_version = stored.version + 1;

    // Masked-echo merge: the frontend sends the mask back when a key is
    // unchanged ("" clears, absent keeps, other replaces).
    merge_api_keys(&stored, &mut incoming);
    validate_library(&incoming)?;

    let mut next = incoming;
    next.version = next_version;
    write_library(&guard, &next)?;
    drop(guard); // explicit: nothing below may run under the db lock

    Ok(Json(json!({ "ok": true })))
}

/// POST /api/models/providers {id, ...template} — add one provider.
#[derive(Debug, Deserialize)]
struct PostProviderBody {
    id: String,
    #[serde(flatten)]
    template: ProviderTemplate,
}

async fn post_provider_v2(
    State(state): State<Arc<AppState>>,
    Json(body): Json<PostProviderBody>,
) -> Result<Json<Value>, ApiError> {
    let id = body.id.trim();
    if id.is_empty() {
        return Err(ApiError::bad("provider id is required"));
    }
    let guard = state.db.lock().unwrap();
    let mut library = read_library(&guard)?;
    if library.providers.contains_key(id) {
        return Err(ApiError::bad(format!("provider {id:?} already exists")));
    }
    let mut template = body.template;
    // A brand-new provider never has a mask echo; an empty apiKey clears.
    if template.api_key.as_deref() == Some("") {
        template.api_key = None;
    }
    validate_provider(id, &template)?;
    library.providers.insert(id.to_string(), template);
    library.version += 1;
    write_library(&guard, &library)?;
    drop(guard);
    Ok(Json(json!({ "ok": true, "id": id })))
}

/// PUT /api/models/providers/:id — update one provider (masked-echo merge
/// on the apiKey; validate; version bump).
async fn put_provider(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(mut template): Json<ProviderTemplate>,
) -> Result<Json<Value>, ApiError> {
    let guard = state.db.lock().unwrap();
    let mut library = read_library(&guard)?;
    if !library.providers.contains_key(&id) {
        return Err(ApiError::not_found(format!("provider {id:?} not found")));
    }
    // Masked-echo merge for the apiKey: "" clears, mask restores plaintext,
    // absent keeps the stored plaintext, anything else replaces.
    let stored_key = library
        .providers
        .get(&id)
        .and_then(|p| p.api_key.as_deref());
    match template.api_key.clone() {
        None => {
            template.api_key = stored_key.map(str::to_string);
        }
        Some(new_key) if new_key.is_empty() => {
            template.api_key = None;
        }
        Some(new_key) => {
            let is_mask_echo = stored_key
                .map(|k| mask_api_key(k) == new_key)
                .unwrap_or(false);
            template.api_key = if is_mask_echo {
                stored_key.map(str::to_string)
            } else {
                Some(new_key)
            };
        }
    }
    validate_provider(&id, &template)?;
    library.providers.insert(id.clone(), template);
    library.version += 1;
    write_library(&guard, &library)?;
    drop(guard);
    Ok(Json(json!({ "ok": true, "id": id })))
}

/// DELETE /api/models/providers/:id — remove one provider.
async fn delete_provider(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let guard = state.db.lock().unwrap();
    let mut library = read_library(&guard)?;
    if library.providers.remove(&id).is_none() {
        return Err(ApiError::not_found(format!("provider {id:?} not found")));
    }
    library.version += 1;
    write_library(&guard, &library)?;
    drop(guard);
    Ok(Json(json!({ "ok": true, "id": id })))
}

/// Per-provider validation (extracted so POST/PUT share it).
fn validate_provider(id: &str, p: &ProviderTemplate) -> Result<(), ApiError> {
    if p.name.trim().is_empty() {
        return Err(ApiError::bad(format!("provider {id:?}: name is required")));
    }
    if p.base_url.trim().is_empty() {
        return Err(ApiError::bad(format!("provider {id:?}: baseUrl is required")));
    }
    if p.api.trim().is_empty() {
        return Err(ApiError::bad(format!("provider {id:?}: api is required")));
    }
    for m in &p.models {
        if m.id.trim().is_empty() {
            return Err(ApiError::bad(format!(
                "provider {id:?}: model id must not be empty"
            )));
        }
    }
    Ok(())
}

// ── GET /api/models/templates ─────────────────────────────────────

/// GET /api/models/templates — the PUBLIC template list for sandbox-side
/// pickers. Same shape as /api/models/providers but WITHOUT the apiKey field
/// at all (not even masked): the sandbox user is expected to paste their own
/// key after copying the template.
async fn get_templates(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, ApiError> {
    let library = {
        let conn = state.db.lock().unwrap();
        read_library(&conn)?
    };
    let templates: Vec<Value> = library
        .providers
        .iter()
        .map(|(id, p)| {
            json!({
                "id": id,
                "name": p.name,
                "baseUrl": p.base_url,
                "api": p.api,
                "models": p.models,
                "websiteUrl": p.website_url,
                "category": p.category,
                "notes": p.notes,
            })
        })
        .collect();
    Ok(Json(json!({
        "version": library.version,
        "templates": templates,
    })))
}

// ── provider resolution (shared by discover + test) ─────────────────

/// The effective fields a probe needs, resolved from the stored library
/// (ById) or the literal request body (transient provider being edited).
/// ById reads the REAL stored key — no mask semantics on the probe path.
struct ResolvedProvider {
    base_url: String,
    api: String,
    api_key: Option<String>,
    headers: BTreeMap<String, String>,
}

/// Resolve a provider by id from the library, or accept literal fields.
/// `baseUrl` is validated non-empty on the literal branch exactly like the
/// app's discover (test resolves by id only and 404s an unknown id).
fn resolve_provider(
    library: &ProviderLibrary,
    req: &DiscoverRequest,
) -> Result<ResolvedProvider, ApiError> {
    match req {
        DiscoverRequest::ById { providerId } => {
            let p = library.providers.get(providerId).ok_or_else(|| {
                ApiError::not_found(format!("provider '{providerId}' not found"))
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
    /// Resolve everything from the provider library by id.
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
/// provider library; URL/header/parse helpers are byte-for-byte the app's
/// (they are pure functions - unit tests live here).
async fn discover(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DiscoverRequest>,
) -> Result<Json<DiscoverResponse>, ApiError> {
    let resolved = {
        let conn = state.db.lock().unwrap();
        let library = read_library(&conn)?;
        // The literal branch validates baseUrl inside (blank -> 400),
        // exactly like the app handler's resolve_provider.
        resolve_provider(&library, &req)?
    };

    let candidates = candidate_urls(&resolved.base_url, &resolved.api);
    let headers = build_headers(
        &resolved.api,
        resolved.api_key.as_deref(),
        &resolved.headers,
    );

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

        let mut req_builder = state
            .http
            .get(url.as_str())
            .header("Accept", "application/json");
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
                        return Ok(Json(DiscoverResponse {
                            models,
                            endpoint: url.clone(),
                        }));
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
                        return Err(ApiError {
                            status,
                            message: err,
                        });
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
        return Err(ApiError {
            status,
            message: msg,
        });
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
            let after_scheme = base.split_once("://").map(|(_, rest)| rest).unwrap_or(base);
            let path = after_scheme.split_once('/').map(|(_, p)| p).unwrap_or("");
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

/// POST /api/models/test body: either a provider id (resolve from the
/// library) or literal endpoint fields (a provider being edited in the UI,
/// not yet saved) — the same untagged split as [`DiscoverRequest`], plus
/// the modelId/protocol probe fields shared by both branches. camelCase
/// field names are the wire contract (app test.rs).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
#[allow(non_snake_case)]
enum TestRequest {
    /// Resolve everything from the library by id.
    ById {
        providerId: String,
        modelId: String,
        /// Override the provider's stored protocol; defaults to provider.api.
        #[serde(default)]
        protocol: Option<String>,
    },
    /// Literal fields (transient provider being edited, or ad-hoc endpoint).
    Literal {
        baseUrl: String,
        #[serde(default = "default_api")]
        api: String,
        apiKey: Option<String>,
        modelId: String,
        #[serde(default)]
        protocol: Option<String>,
    },
}

impl TestRequest {
    fn model_id(&self) -> &str {
        match self {
            TestRequest::ById { modelId, .. } | TestRequest::Literal { modelId, .. } => modelId,
        }
    }

    fn protocol_override(&self) -> Option<&str> {
        match self {
            TestRequest::ById { protocol, .. } | TestRequest::Literal { protocol, .. } => {
                protocol.as_deref()
            }
        }
    }

    /// The provider half as a [`DiscoverRequest`], so test resolves the
    /// endpoint exactly like discover (same 404 / blank-baseUrl contract).
    fn provider_part(&self) -> DiscoverRequest {
        match self {
            TestRequest::ById { providerId, .. } => DiscoverRequest::ById {
                providerId: providerId.clone(),
            },
            TestRequest::Literal {
                baseUrl,
                api,
                apiKey,
                ..
            } => DiscoverRequest::Literal {
                baseUrl: baseUrl.clone(),
                api: api.clone(),
                apiKey: apiKey.clone(),
            },
        }
    }

    /// What identifies the endpoint in user-facing errors (the no-key
    /// message): the provider id on the ById branch, the URL otherwise.
    fn label(&self) -> &str {
        match self {
            TestRequest::ById { providerId, .. } => providerId,
            TestRequest::Literal { baseUrl, .. } => baseUrl,
        }
    }
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
/// test.rs; the provider lookup reads the provider library instead of
/// models.json. The untagged body mirrors discover: ById resolves the
/// stored provider, Literal probes the fields being edited (pre-save)
/// without touching the library.
async fn test(
    State(state): State<Arc<AppState>>,
    Json(req): Json<TestRequest>,
) -> Result<Json<TestResponse>, ApiError> {
    if let TestRequest::ById {
        providerId,
        modelId,
        ..
    } = &req
    {
        if providerId.trim().is_empty() || modelId.trim().is_empty() {
            return Err(ApiError::bad("providerId and modelId are required"));
        }
    }
    if req.model_id().trim().is_empty() {
        return Err(ApiError::bad("modelId is required"));
    }

    let (resolved, protocol) = {
        let conn = state.db.lock().unwrap();
        let library = read_library(&conn)?;
        let resolved = resolve_provider(&library, &req.provider_part())?;
        let protocol = req
            .protocol_override()
            .map(str::to_owned)
            .unwrap_or_else(|| resolved.api.clone());
        (resolved, protocol)
    };

    // R1: the provider's baseUrl IS the endpoint for every protocol.
    let base_url = resolved.base_url;

    // No key => error, but still HTTP 200 with ok:false (UI decides).
    let key = resolved.api_key;
    if key.as_deref().is_none_or(|k| k.is_empty()) {
        return Ok(Json(TestResponse {
            ok: false,
            latency_ms: None,
            status: None,
            error: Some(format!("No API key found for \"{}\"", req.label())),
            response_text: None,
        }));
    }

    let headers = build_headers(&protocol, key.as_deref(), &resolved.headers);
    let endpoint = completion_url(&base_url, &protocol);
    let body = completion_body(req.model_id(), &protocol);

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

// ── tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mem_db() -> rusqlite::Connection {
        // Just the kv table is enough for the library tests (no sandbox
        // rows needed — there is no assignment layer anymore).
        let conn = rusqlite::Connection::open_in_memory().expect("open in-memory db");
        db::init_schema(&conn).expect("init schema");
        conn
    }

    fn sample_template(key: &str) -> ProviderTemplate {
        ProviderTemplate {
            name: "Sample".into(),
            base_url: "https://api.example.com/v1".into(),
            api: "openai-completions".into(),
            api_key: Some(key.into()),
            ..Default::default()
        }
    }

    fn sample_library(key: &str) -> ProviderLibrary {
        let mut lib = ProviderLibrary::default();
        lib.providers.insert("sample".into(), sample_template(key));
        lib
    }

    // --- kv round-trip + versioning ---

    #[test]
    fn library_roundtrip_preserves_plaintext_key() {
        let conn = mem_db();
        // First read on an empty db: fresh empty library, version 0 —
        // nothing written until a mutation.
        let first = read_library(&conn).unwrap();
        assert_eq!(first.version, 0);
        assert!(first.providers.is_empty());

        let mut lib = sample_library("sk-plaintext-secret");
        lib.version = 3;
        write_library(&conn, &lib).unwrap();
        let back = read_library(&conn).unwrap();
        assert_eq!(back.version, 3);
        assert_eq!(
            back.providers.get("sample").unwrap().api_key.as_deref(),
            Some("sk-plaintext-secret"),
            "kv stores the plaintext; masking happens only on the GET path"
        );
    }

    #[test]
    fn put_semantics_version_increments_each_write() {
        let conn = mem_db();
        let mut lib = sample_library("sk-key-12345678");
        for v in 1..=3 {
            lib.version = v;
            write_library(&conn, &lib).unwrap();
        }
        assert_eq!(read_library(&conn).unwrap().version, 3);
        lib.version = 4;
        lib.providers.remove("sample");
        write_library(&conn, &lib).unwrap();
        let back = read_library(&conn).unwrap();
        assert_eq!(back.version, 4);
        assert!(back.providers.is_empty());
    }

    #[test]
    fn corrupt_kv_row_is_an_internal_error_not_a_reset() {
        let conn = mem_db();
        db::kv_set(&conn, KV_LIBRARY, "not json{{").unwrap();
        let err = read_library(&conn).unwrap_err();
        assert_eq!(err.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(err.message.contains("corrupt"));
        // The row is untouched — surfacing must never rewrite user config.
        assert_eq!(
            db::kv_get(&conn, KV_LIBRARY).unwrap().as_deref(),
            Some("not json{{")
        );
    }

    // --- masked-echo merge (PUT semantics) ---

    #[test]
    fn put_masked_echo_merge_keeps_plaintext_in_store() {
        // The masked-echo contract: the frontend echoes the mask back; the
        // STORED key must remain the plaintext.
        let stored = sample_library("sk-key-12345678");

        // Mask echo restores the stored plaintext.
        let mut incoming = sample_library(&mask_api_key("sk-key-12345678"));
        merge_api_keys(&stored, &mut incoming);
        assert_eq!(
            incoming.providers.get("sample").unwrap().api_key.as_deref(),
            Some("sk-key-12345678")
        );

        // "" clears.
        let mut incoming = sample_library("");
        merge_api_keys(&stored, &mut incoming);
        assert_eq!(incoming.providers.get("sample").unwrap().api_key, None);

        // Absent keeps.
        let mut incoming = sample_library("");
        incoming.providers.get_mut("sample").unwrap().api_key = None;
        merge_api_keys(&stored, &mut incoming);
        assert_eq!(
            incoming.providers.get("sample").unwrap().api_key.as_deref(),
            Some("sk-key-12345678")
        );

        // Other replaces.
        let mut incoming = sample_library("sk-other");
        merge_api_keys(&stored, &mut incoming);
        assert_eq!(
            incoming.providers.get("sample").unwrap().api_key.as_deref(),
            Some("sk-other")
        );
    }

    // --- validation ---

    #[test]
    fn validate_library_rejects_blank_required_fields() {
        let mut lib = ProviderLibrary::default();
        lib.providers.insert(
            "bad".into(),
            ProviderTemplate {
                name: "  ".into(),
                base_url: "https://api.example.com".into(),
                api: "openai-completions".into(),
                ..Default::default()
            },
        );
        let err = validate_library(&lib).unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert!(err.message.contains("name"), "{}", err.message);

        let mut lib = ProviderLibrary::default();
        lib.providers.insert(
            "bad".into(),
            ProviderTemplate {
                name: "X".into(),
                base_url: "".into(),
                api: "openai-completions".into(),
                ..Default::default()
            },
        );
        let err = validate_library(&lib).unwrap_err();
        assert!(err.message.contains("baseUrl"), "{}", err.message);
    }

    // --- handler-level contract locks ---

    fn mgr_state() -> Arc<AppState> {
        Arc::new(AppState::new_for_test())
    }

    #[tokio::test]
    async fn get_providers_handler_returns_masked_keys() {
        // GET /api/models/providers mask contract: the stored plaintext
        // never reaches the wire on this route.
        let state = mgr_state();
        {
            let conn = state.db.lock().unwrap();
            write_library(&conn, &sample_library("sk-plaintext-secret")).unwrap();
        }
        let Json(lib) = get_providers(State(state)).await.unwrap();
        let key = lib
            .providers
            .get("sample")
            .unwrap()
            .api_key
            .clone()
            .unwrap();
        assert_ne!(key, "sk-plaintext-secret");
        assert!(key.contains("****"), "masked shape, got {key}");
    }

    #[tokio::test]
    async fn put_providers_handler_merge_validate_and_version_bump() {
        // Handler assembly: masked-echo merge keeps the stored plaintext,
        // validate gates the write, and the kv version bumps only on success.
        let state = mgr_state();
        {
            let conn = state.db.lock().unwrap();
            let mut lib = sample_library("sk-key-12345678");
            lib.version = 4;
            write_library(&conn, &lib).unwrap();
        }

        // Invalid PUT (blank name): 400, and the stored version/config are
        // untouched.
        let mut bad = ProviderLibrary::default();
        bad.providers.insert(
            "sample".into(),
            ProviderTemplate {
                name: "".into(),
                base_url: "https://api.example.com".into(),
                api: "openai-completions".into(),
                ..Default::default()
            },
        );
        let err = put_providers(State(state.clone()), Json(bad)).await.unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        {
            let conn = state.db.lock().unwrap();
            assert_eq!(
                read_library(&conn).unwrap().version,
                4,
                "failed PUT must not bump the version"
            );
        }

        // Valid masked-echo PUT: plaintext survives the merge, version 4→5.
        let incoming = sample_library(&mask_api_key("sk-key-12345678"));
        let Json(resp) = put_providers(State(state.clone()), Json(incoming))
            .await
            .unwrap();
        assert_eq!(resp["ok"], true);
        {
            let conn = state.db.lock().unwrap();
            let lib = read_library(&conn).unwrap();
            assert_eq!(lib.version, 5);
            assert_eq!(
                lib.providers.get("sample").unwrap().api_key.as_deref(),
                Some("sk-key-12345678"),
                "mask echo restores the plaintext on the handler path"
            );
        }
    }

    #[tokio::test]
    async fn post_provider_rejects_duplicate_and_creates_new() {
        let state = mgr_state();
        {
            let conn = state.db.lock().unwrap();
            write_library(&conn, &sample_library("sk-key")).unwrap();
        }
        // Duplicate id: 400.
        let err = post_provider_v2(
            State(state.clone()),
            Json(PostProviderBody {
                id: "sample".into(),
                template: sample_template("sk-other"),
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);

        // New id: creates.
        let Json(resp) = post_provider_v2(
            State(state.clone()),
            Json(PostProviderBody {
                id: "second".into(),
                template: ProviderTemplate {
                    name: "Second".into(),
                    base_url: "https://api.second.com/v1".into(),
                    api: "openai-completions".into(),
                    ..Default::default()
                },
            }),
        )
        .await
        .unwrap();
        assert_eq!(resp["ok"], true);
        {
            let conn = state.db.lock().unwrap();
            let lib = read_library(&conn).unwrap();
            assert_eq!(lib.providers.len(), 2);
            assert!(lib.providers.contains_key("second"));
        }
    }

    #[tokio::test]
    async fn put_and_delete_provider_roundtrip() {
        let state = mgr_state();
        {
            let conn = state.db.lock().unwrap();
            write_library(&conn, &sample_library("sk-key")).unwrap();
        }
        // Unknown id: 404.
        let err = put_provider(
            State(state.clone()),
            Path("ghost".into()),
            Json(sample_template("sk-x")),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, StatusCode::NOT_FOUND);

        // Known id: masked-echo merge.
        let Json(resp) = put_provider(
            State(state.clone()),
            Path("sample".into()),
            Json(sample_template(&mask_api_key("sk-key"))),
        )
        .await
        .unwrap();
        assert_eq!(resp["ok"], true);
        {
            let conn = state.db.lock().unwrap();
            let lib = read_library(&conn).unwrap();
            assert_eq!(
                lib.providers.get("sample").unwrap().api_key.as_deref(),
                Some("sk-key"),
                "mask echo restores the plaintext"
            );
        }

        // Delete: removes the provider.
        let Json(resp) = delete_provider(State(state.clone()), Path("sample".into()))
            .await
            .unwrap();
        assert_eq!(resp["ok"], true);
        {
            let conn = state.db.lock().unwrap();
            let lib = read_library(&conn).unwrap();
            assert!(lib.providers.is_empty());
        }

        // Delete unknown: 404.
        let err = delete_provider(State(state), Path("ghost".into()))
            .await
            .unwrap_err();
        assert_eq!(err.status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_templates_omits_api_key_entirely() {
        // The template list is what sandbox-side pickers consume. The
        // apiKey must NOT travel at all (not even masked): the sandbox user
        // pastes their own key after copying.
        let state = mgr_state();
        {
            let conn = state.db.lock().unwrap();
            write_library(&conn, &sample_library("sk-secret")).unwrap();
        }
        let Json(v) = get_templates(State(state)).await.unwrap();
        let templates = v["templates"].as_array().expect("templates array");
        assert_eq!(templates.len(), 1);
        assert!(
            templates[0].get("apiKey").is_none(),
            "apiKey must be absent, not null: {}",
            templates[0]
        );
        assert_eq!(templates[0]["name"], "Sample");
        assert_eq!(templates[0]["baseUrl"], "https://api.example.com/v1");
    }

    // --- legacy migration ---

    #[test]
    fn legacy_models_profiles_migrates_to_library() {
        // The pre-cc-switch multi-profile row: extract the first profile's
        // providers into the new library, drop the rest.
        let conn = mem_db();
        db::kv_set(
            &conn,
            KV_MODELS_LEGACY,
            &serde_json::to_string(&json!({
                "version": 5,
                "profiles": [{
                    "id": "default",
                    "name": "Default",
                    "config": {
                        "version": 1,
                        "providers": {
                            "sample": {
                                "name": "Sample",
                                "baseUrl": "https://api.example.com/v1",
                                "api": "openai-completions",
                                "apiKey": "sk-legacy-key",
                            }
                        },
                        "agents": {}
                    }
                }],
                "assignments": {"alpha": "default"}
            }))
            .unwrap(),
        )
        .unwrap();
        let lib = read_library(&conn).unwrap();
        assert_eq!(
            lib.providers.get("sample").unwrap().api_key.as_deref(),
            Some("sk-legacy-key"),
            "the legacy provider carries over"
        );
        assert!(
            db::kv_get(&conn, KV_MODELS_LEGACY).unwrap().is_none(),
            "old key deleted"
        );
        assert!(
            db::kv_get(&conn, KV_LIBRARY).unwrap().is_some(),
            "new key persisted"
        );
    }

    #[test]
    fn legacy_models_config_migrates_to_library() {
        // The even-older Phase-4a single-config row.
        let conn = mem_db();
        db::kv_set(
            &conn,
            "models_config",
            &serde_json::to_string(&json!({
                "version": 2,
                "config": {
                    "version": 1,
                    "providers": {
                        "old": {
                            "name": "Old",
                            "baseUrl": "https://old.example.com",
                            "api": "openai-completions",
                            "apiKey": "sk-old-key"
                        }
                    },
                    "agents": {}
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let lib = read_library(&conn).unwrap();
        assert_eq!(
            lib.providers.get("old").unwrap().api_key.as_deref(),
            Some("sk-old-key")
        );
        assert!(db::kv_get(&conn, "models_config").unwrap().is_none());
        assert!(db::kv_get(&conn, KV_LIBRARY).unwrap().is_some());
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
        let auth = h
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case("authorization"));
        assert_eq!(auth.map(|(_, v)| v.as_str()), Some("Bearer sk-test1234"));

        let h = build_headers("anthropic-messages", Some("sk-ant-xx"), &BTreeMap::new());
        assert!(h
            .iter()
            .any(|(n, v)| n == "anthropic-version" && v == "2023-06-01"));
        assert!(h
            .iter()
            .all(|(n, _)| !n.eq_ignore_ascii_case("authorization")));

        let h = build_headers("openai-completions", None, &BTreeMap::new());
        assert!(h
            .iter()
            .all(|(n, _)| !n.eq_ignore_ascii_case("authorization")));
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
            extract_response_text(
                r#"{"choices":[{"message":{"content":"OK"}}]}"#,
                "openai-completions"
            ),
            "OK"
        );
        assert_eq!(
            extract_response_text(r#"{"output_text":"OK"}"#, "openai-responses"),
            "OK"
        );
        assert_eq!(
            extract_response_text(
                r#"{"content":[{"type":"text","text":"OK"}]}"#,
                "anthropic-messages"
            ),
            "OK"
        );
        assert_eq!(extract_response_text("not json", "openai-completions"), "");
    }

    #[test]
    fn truncate_shapes() {
        assert_eq!(truncate("abc", 10), "abc");
        let t = truncate(&"a".repeat(600), 500);
        assert!(t.ends_with('…'));
        assert_eq!(t.chars().count(), 501);
    }

    // --- POST /api/models/test untagged body (Issue #23) ---

    #[test]
    fn test_request_untagged_deserialization() {
        // The two wire shapes (mirroring DiscoverRequest): `{providerId,
        // modelId}` resolves from the library; `{baseUrl, api?, apiKey?,
        // modelId}` probes a provider being edited pre-save. `api` defaults
        // to openai-completions; `protocol` is optional on both branches.
        let by_id: TestRequest =
            serde_json::from_str(r#"{"providerId":"sample","modelId":"m"}"#).unwrap();
        assert!(
            matches!(&by_id, TestRequest::ById { providerId, modelId, protocol }
            if providerId == "sample" && modelId == "m" && protocol.is_none())
        );

        let literal: TestRequest = serde_json::from_str(
            r#"{"baseUrl":"https://api.example.com/v1","apiKey":"sk-x","modelId":"m"}"#,
        )
        .unwrap();
        match &literal {
            TestRequest::Literal {
                baseUrl,
                api,
                apiKey,
                modelId,
                protocol,
            } => {
                assert_eq!(baseUrl, "https://api.example.com/v1");
                assert_eq!(api, "openai-completions", "api defaults when absent");
                assert_eq!(apiKey.as_deref(), Some("sk-x"));
                assert_eq!(modelId, "m");
                assert!(protocol.is_none());
            }
            other => panic!("expected Literal, got {other:?}"),
        }

        let with_protocol: TestRequest = serde_json::from_str(
            r#"{"baseUrl":"https://api.example.com/v1","api":"anthropic-messages","modelId":"m","protocol":"anthropic-messages"}"#,
        )
        .unwrap();
        assert!(
            matches!(&with_protocol, TestRequest::Literal { api, protocol, .. }
                if api == "anthropic-messages" && protocol.as_deref() == Some("anthropic-messages")),
            "explicit api + protocol override: {with_protocol:?}"
        );

        // providerId-present bodies still take the ById branch even when
        // baseUrl-like extras ride along (variant order = precedence).
        let mixed: TestRequest = serde_json::from_str(
            r#"{"providerId":"sample","modelId":"m","baseUrl":"https://ignored"}"#,
        )
        .unwrap();
        assert!(matches!(mixed, TestRequest::ById { .. }));

        // Neither shape: untagged deserialization fails (axum 422).
        assert!(serde_json::from_str::<TestRequest>(r#"{"modelId":"m"}"#).is_err());
    }

    #[tokio::test]
    async fn test_handler_literal_branch_matches_by_id_contract() {
        // Issue #23: the literal branch exists so an UNSAVED provider's test
        // button still fires a real probe. Contract parity with ById: no key
        // → HTTP 200 {ok:false}; blank baseUrl → 400; unknown id → 404.
        let state = mgr_state();
        {
            let conn = state.db.lock().unwrap();
            write_library(&conn, &sample_library("sk-stored")).unwrap();
        }
        let call = |body: TestRequest| test(State(state.clone()), Json(body));

        // Literal without a key: 200 + ok:false naming the baseUrl (the
        // ById branch names the provider id). Network-free short-circuit.
        let Json(resp) = call(TestRequest::Literal {
            baseUrl: "https://api.example.com/v1".into(),
            api: "openai-completions".into(),
            apiKey: None,
            modelId: "m".into(),
            protocol: None,
        })
        .await
        .unwrap();
        assert!(!resp.ok);
        assert!(
            resp.error.as_deref().unwrap().contains("api.example.com"),
            "error names the endpoint: {:?}",
            resp.error
        );

        // Literal blank baseUrl: 400, same as discover's resolver.
        let err = call(TestRequest::Literal {
            baseUrl: "  ".into(),
            api: "openai-completions".into(),
            apiKey: Some("sk-x".into()),
            modelId: "m".into(),
            protocol: None,
        })
        .await
        .unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);

        // Literal missing modelId: 400.
        let err = call(TestRequest::Literal {
            baseUrl: "https://api.example.com/v1".into(),
            api: "openai-completions".into(),
            apiKey: Some("sk-x".into()),
            modelId: "".into(),
            protocol: None,
        })
        .await
        .unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);

        // ById regression: unknown provider still 404s with the same message.
        let err = call(TestRequest::ById {
            providerId: "ghost".into(),
            modelId: "m".into(),
            protocol: None,
        })
        .await
        .unwrap_err();
        assert_eq!(err.status, StatusCode::NOT_FOUND);
        assert!(err.message.contains("ghost"), "{}", err.message);
    }
}
