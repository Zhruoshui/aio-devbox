// Model-config routes — the mgr side of the config up-lift (sandbox-mgr
// Phase 4a, prd D6 / design §3.7), extended to multi-profile + per-sandbox
// assignment (unified Phase 4, prd D8 / design §4).
//
// mgr is the SINGLE source of truth for the canonical model config, per
// PROFILE. The stored artifact is a kv row (`models_profiles`) shaped
//   {"version": <u64, GLOBAL, +1 per write>,
//    "profiles": [ {"id": <slug>, "name": <display>, "config": <CanonicalConfig>} ],
//    "assignments": { "<sandbox-name>": "<profile-id>" }}
// A profile IS a whole CanonicalConfig (aio-models is untouched); the wrapper
// adds the identity + assignment dimensions. `version` is a plain ETag
// surrogate — the sandbox pull deep-compares content, so a global bump that
// touches an unrelated profile costs nothing but a skipped compare.
//
// Migration (design §4.1): a legacy `models_config` row (single global
// config, Phase 4a) becomes profile id "default" with EVERY existing sandbox
// assigned to it — behavior-identical for all pre-upgrade sandboxes. The old
// row survives until the new row is written; a rolled-back mgr (new key
// missing, old key present) reads the old key back. Both directions are
// therefore safe across an upgrade/downgrade cycle.
//
// Routes (contract-locked to app/src/mgr_sync.rs + mgr-web):
//   GET  /api/models/profiles            — [{id, name, version, assigned[]}]
//   POST /api/models/profiles            — {name} -> new empty profile
//   PUT  /api/models/profiles/:id        — masked-echo merge + validate
//   DEL  /api/models/profiles/:id        — refuse the last one; unassign
//   GET  /api/models/sync?name=<sbx>     — {version, config} UNMASKED for the
//                                           sandbox's ASSIGNED profile; 404
//                                           when the sandbox is unassigned or
//                                           unknown (app keeps local silently)
//   GET/PUT /api/models/config           — profile-scoped (?profile=, default
//                                           = first profile) legacy pair
//   POST /api/models/import/pi           — absorb ~/.pi/agent/models.json (?profile=)
//   POST /api/models/discover | /test    — provider probe (?profile= scoping)
//   GET  /api/models/catalog             — models.dev proxy (global, 1h cache)
// PUT /api/sandboxes/:name/model_profile lives in routes.rs (sandbox-scoped).
//
// NOT ported (mgr semantics don't exist for them): /api/models/agents,
// apply/:agent, live-provider edit/delete/sync, usage — those read/write
// files INSIDE one sandbox; mgr has no agent installs.
//
// Error shape: app answers `(StatusCode, String)` (plain-text bodies);
// mgr's house style is `{"error": "<msg>"}` JSON (routes.rs ApiError). The
// mgr-web api client decodes both shapes through apiError(), so the semantic
// contract (status code + human-readable message) is preserved either way.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use aio_models::store::{
    ensure_preset_ids, import_from_pi, mask_config, merge_api_keys, validate, CanonicalConfig,
    CostEntry, ImportResponse, PutResponse, StoreError,
};

use crate::db;
use crate::state::AppState;

/// kv row key holding the multi-profile store (unified Phase 4, design §4.1).
const KV_MODELS: &str = "models_profiles";
/// The LEGACY Phase-4a single-config key. Read-only (migration input); never
/// written once the new key exists.
const KV_MODELS_LEGACY: &str = "models_config";
/// Profile id the legacy config migrates into (design §4.1: "default", with
/// every existing sandbox assigned — zero behavior change).
const DEFAULT_PROFILE_ID: &str = "default";

// ── stored payload ────────────────────────────────────────────────

/// One named profile: a whole CanonicalConfig plus display metadata.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub config: CanonicalConfig,
}

/// One sandbox's model assignment: which profile, plus an optional agent
/// subset (S2, D4c). `agents: None` = ALL four agents (pi/claude/codex/
/// opencode) — also the meaning of pre-S2/legacy data, so old payloads
/// migrate to this shape without behavior change (AC4). `Some([])` = zero
/// agents, which makes the sync endpoint 404 (sandbox keeps local — AC3).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct StoredAssignment {
    pub profile: Option<String>,
    #[serde(default)]
    pub agents: Option<Vec<String>>,
}

/// The agent whitelist (S2). `agents` values outside this set are rejected
/// on PUT (400) and silently ignored on sync (only assigned agents render).
pub const VALID_AGENTS: [&str; 4] = ["pi", "claude", "codex", "opencode"];

/// Deserialize the assignments map accepting BOTH shapes: the pre-S2 legacy
/// form `{<sandbox>: "<profile-id>"}` (bare string value — every sandbox
/// implicitly gets all agents, `agents: None`) and the S2 form
/// `{<sandbox>: {"profile": <id|null>, "agents": <[..]|null>}}`. Anything
/// else is a corrupt row (surfaced as internal error, never a silent reset).
fn deserialize_assignments<'de, D>(d: D) -> Result<BTreeMap<String, StoredAssignment>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(d)?;
    let obj = v.as_object().ok_or_else(|| {
        serde::de::Error::custom("models_profiles assignments must be a JSON object")
    })?;
    let mut out = BTreeMap::new();
    for (name, value) in obj {
        let entry = if let Some(id) = value.as_str() {
            // Legacy bare-string value → full-agent assignment.
            StoredAssignment {
                profile: Some(id.to_string()),
                agents: None,
            }
        } else {
            serde_json::from_value(value.clone()).map_err(serde::de::Error::custom)?
        };
        out.insert(name.clone(), entry);
    }
    Ok(out)
}

/// The kv row payload. `version` is GLOBAL (bumped on every successful
/// mutation of any profile — the sandbox pull's deep compare makes per-profile
/// versioning an unnecessary cost). `assignments` maps sandbox name → its
/// {profile id, agent subset}; an unassigned sandbox pulls nothing (sync 404s,
/// app keeps local).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct StoredModels {
    version: u64,
    #[serde(default)]
    profiles: Vec<Profile>,
    #[serde(default, deserialize_with = "deserialize_assignments")]
    assignments: BTreeMap<String, StoredAssignment>,
}

impl StoredModels {
    /// The fresh store: one empty "default" profile, no assignments (the
    /// migration fills the config + assignments when a legacy row exists).
    fn fresh() -> Self {
        StoredModels {
            version: 0,
            profiles: vec![Profile {
                id: DEFAULT_PROFILE_ID.into(),
                name: "Default".into(),
                config: CanonicalConfig::default(),
            }],
            assignments: BTreeMap::new(),
        }
    }

    fn profile(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    fn profile_mut(&mut self, id: &str) -> Option<&mut Profile> {
        self.profiles.iter_mut().find(|p| p.id == id)
    }

    /// The profile a ?profile= query selects: the given id, else the FIRST
    /// profile (insertion order is kept; "default" leads unless deleted).
    fn selected(&self, want: Option<&str>) -> Result<&Profile, ApiError> {
        match want {
            Some(id) => self.profile(id).ok_or_else(|| ApiError {
                status: StatusCode::NOT_FOUND,
                message: format!("model profile {id:?} not found"),
            }),
            None => self
                .profiles
                .first()
                .ok_or_else(|| ApiError::internal("no model profiles exist (store is corrupt?)")),
        }
    }
}

/// Read the stored profiles, running the legacy migration when needed.
///
/// - new key present → parse it (corrupt = internal error, never a reset —
///   every write serialized a validated store, so corruption means something
///   external touched the db);
/// - new key missing + legacy key present → MIGRATE: legacy config becomes
///   the "default" profile, every existing sandbox row is assigned to it,
///   write the new key, delete the old one (old content survives until the
///   new write succeeded — a crash in between just re-runs this next boot);
/// - neither key → fresh default store (in-memory only; nothing is written
///   until a mutation happens, same laziness as Phase 4a).
fn read_stored(conn: &rusqlite::Connection) -> Result<StoredModels, ApiError> {
    if let Some(text) = db::kv_get(conn, KV_MODELS)? {
        return serde_json::from_str(&text).map_err(|e| {
            ApiError::internal(format!(
                "stored {KV_MODELS} is corrupt ({e}); kv key {KV_MODELS:?} in state.db"
            ))
        });
    }
    let Some(legacy) = db::kv_get(conn, KV_MODELS_LEGACY)? else {
        return Ok(StoredModels::fresh());
    };
    // Legacy row: {"version": u64, "config": CanonicalConfig}.
    #[derive(serde::Deserialize)]
    struct LegacyRow {
        #[serde(default)]
        version: u64,
        config: CanonicalConfig,
    }
    let legacy: LegacyRow = serde_json::from_str(&legacy).map_err(|e| {
        ApiError::internal(format!(
            "stored {KV_MODELS_LEGACY} is corrupt ({e}); kv key {KV_MODELS_LEGACY:?} in state.db"
        ))
    })?;
    let mut stored = StoredModels {
        version: legacy.version.max(1),
        profiles: vec![Profile {
            id: DEFAULT_PROFILE_ID.into(),
            name: "Default".into(),
            config: legacy.config,
        }],
        assignments: BTreeMap::new(),
    };
    // Every existing sandbox keeps pulling exactly what it pulled before
    // (S2: legacy rows carry no agent subset — `agents: None` = all agents).
    for name in db::list_sandbox_names(conn)? {
        stored.assignments.insert(
            name,
            StoredAssignment {
                profile: Some(DEFAULT_PROFILE_ID.into()),
                agents: None,
            },
        );
    }
    write_stored(conn, &stored)?;
    db::kv_del(conn, KV_MODELS_LEGACY)?;
    tracing::info!(
        "migrated legacy {KV_MODELS_LEGACY} -> {KV_MODELS} (default profile, {} sandbox(s) assigned)",
        stored.assignments.len()
    );
    Ok(stored)
}

/// Serialize + write the kv row. Called with the db Mutex already held (the
/// whole read-merge-write happens inside one lock acquisition — no await
/// points, so the std Mutex discipline of state.rs holds and concurrent PUTs
/// serialize naturally, replacing app's models_lock).
fn write_stored(conn: &rusqlite::Connection, stored: &StoredModels) -> Result<(), ApiError> {
    let text = serde_json::to_string(stored)
        .map_err(|e| ApiError::internal(format!("serialize {KV_MODELS}: {e}")))?;
    db::kv_set(conn, KV_MODELS, &text)?;
    Ok(())
}

// ── cross-module assignment surface (routes.rs PUT /:name/model_profile) ──

/// The full assignment record for a sandbox, for sandbox_json's
/// `model_profile` + `model_agents` fields (None = unassigned; the JSON
/// carries null then). Runs the legacy migration lazily like every other
/// read path.
pub fn assignment(
    conn: &rusqlite::Connection,
    sandbox: &str,
) -> Result<Option<StoredAssignment>, ApiError> {
    Ok(read_stored(conn)?.assignments.get(sandbox).cloned())
}

/// The profile id a sandbox is assigned to (convenience over `assignment`;
/// kept for call sites that only need the id). None = unassigned.
/// `#[cfg(test)]`: production call sites use `assignment` (which also carries
/// the agent subset); this shim survives for the tests' brevity.
#[cfg(test)]
pub(crate) fn assigned_profile(
    conn: &rusqlite::Connection,
    sandbox: &str,
) -> Result<Option<String>, ApiError> {
    Ok(assignment(conn, sandbox)?.and_then(|a| a.profile))
}

/// The WHOLE assignment map, for the sandbox list (one store parse instead
/// of one per row — routes.rs list_sandboxes). Like every read path, runs
/// the legacy migration lazily.
pub fn read_assignments(
    conn: &rusqlite::Connection,
) -> Result<std::collections::BTreeMap<String, StoredAssignment>, ApiError> {
    Ok(read_stored(conn)?.assignments)
}

/// Assign (`Some(id)`, optional agent subset `agents`) or unassign (`None`)
/// a sandbox's model profile. Pure kv write — NEVER triggers a recreate
/// (design §4.2: env changes go through the recreate job; the assignment
/// lands on the sandbox's next 60s pull). Unknown profile id = 404; an
/// agent name outside the VALID_AGENTS whitelist = 400.
///
/// `agents` semantics (S2, D4c):
///   - `None`        → every agent renders (legacy/full-assignment default);
///   - `Some([])`    → zero agents (sync 404s, sandbox keeps local);
///   - `Some(list)`  → exactly the listed agents render; the rest of the
///     sandbox's local agent files are left untouched (AC3).
pub fn set_assignment(
    conn: &rusqlite::Connection,
    sandbox: &str,
    profile: Option<&str>,
    agents: Option<&[String]>,
) -> Result<(), ApiError> {
    let mut stored = read_stored(conn)?;
    match profile {
        Some(id) => {
            if stored.profile(id).is_none() {
                return Err(ApiError {
                    status: StatusCode::NOT_FOUND,
                    message: format!("model profile {id:?} not found"),
                });
            }
            if let Some(list) = agents {
                for name in list {
                    if !VALID_AGENTS.contains(&name.as_str()) {
                        return Err(ApiError::bad(format!(
                            "unknown agent {name:?} (valid: {})",
                            VALID_AGENTS.join(", ")
                        )));
                    }
                }
            }
            stored.assignments.insert(
                sandbox.to_string(),
                StoredAssignment {
                    profile: Some(id.to_string()),
                    agents: agents.map(|a| a.to_vec()),
                },
            );
        }
        None => {
            stored.assignments.remove(sandbox);
        }
    }
    stored.version += 1;
    write_stored(conn, &stored)
}

/// Generate a profile id: `profile-<5 hex>` splitmix64 (the same shape as
/// aio-models' gen_preset_id — kept local because that one is preset-scoped
/// and profile ids are a different domain).
fn gen_profile_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut z = nanos ^ n.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    format!("profile-{z:05x}")
}

// ── error type (house style; mirrors routes.rs but local to keep the
// model routes self-contained like app's (StatusCode, String) tuples) ──

/// Handler error: status + `{"error": msg}` JSON (routes.rs ApiError shape).
/// Validation failures map to 400, upstream probe failures to 502/404 —
/// the same codes the app handlers emit for the same conditions.
///
/// `pub(crate)` fields: routes.rs consumes the assignment helpers (which
/// return this type) through `ApiError::with_status` — same JSON shape on
/// the wire either way.
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
        .route(
            "/api/models/profiles",
            get(list_profiles).post(create_profile),
        )
        .route(
            "/api/models/profiles/:id",
            get(get_profile).put(put_profile).delete(delete_profile),
        )
        .route("/api/models/sync", get(sync))
        .route("/api/models/import/pi", post(import_pi))
        .route("/api/models/discover", post(discover))
        .route("/api/models/test", post(test))
        .route("/api/models/catalog", get(get_catalog))
}

// ── profile scoping (?profile=) ────────────────────────────────────

/// Query param shared by the profile-scoped routes: which profile a GET/PUT
/// config / import / discover / test call operates on. Absent = the first
/// profile (the models page always sends it explicitly once >1 exist).
#[derive(Debug, Clone, Deserialize, Default)]
struct ProfileQuery {
    profile: Option<String>,
}

// ── GET/PUT /api/models/config (profile-scoped) ────────────────────

/// GET /api/models/config — masked canonical config of the selected profile
/// (app get_config: read, mask every apiKey, return; never errors on a
/// missing store).
async fn get_config(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ProfileQuery>,
) -> Result<Json<CanonicalConfig>, ApiError> {
    let mut config = {
        let conn = state.db.lock().unwrap();
        read_stored(&conn)?
            .selected(q.profile.as_deref())?
            .config
            .clone()
    };
    mask_config(&mut config);
    Ok(Json(config))
}

/// PUT /api/models/config — masked-echo merge + ensure_preset_ids +
/// validate + write + version bump (app put_config, step for step) into the
/// selected profile.
///
/// Serialization: the read-merge-write happens inside one db Mutex
/// acquisition with no await in between, so concurrent PUTs are naturally
/// serialized — the kv row replaces app's models.json + models_lock pair.
async fn put_config(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ProfileQuery>,
    Json(mut incoming): Json<CanonicalConfig>,
) -> Result<Json<PutResponse>, ApiError> {
    let guard = state.db.lock().unwrap();

    let mut stored = read_stored(&guard)?;
    let profile_id = stored.selected(q.profile.as_deref())?.id.clone();
    let stored_config = stored.profile(&profile_id).unwrap().config.clone();
    let next_version = stored.version + 1;

    // Masked-echo merge: the frontend sends the mask back when a key is
    // unchanged ("" clears, absent keeps, other replaces — store.rs).
    merge_api_keys(&stored_config, &mut incoming);

    // Backend owns preset ids: backfill ones the frontend created blank.
    ensure_preset_ids(&mut incoming);

    validate(&incoming).map_err(|errs| ApiError::bad(errs.join("; ")))?;

    // CanonicalConfig.version is the legacy file-format field (always 1);
    // the STORE-level version lives in the kv wrapper.
    incoming.version = 1;

    stored.profile_mut(&profile_id).unwrap().config = incoming;
    stored.version = next_version;
    write_stored(&guard, &stored)?;
    drop(guard); // explicit: nothing below may run under the db lock

    Ok(Json(PutResponse {
        ok: true,
        warnings: vec![],
    }))
}

// ── GET/POST /api/models/profiles, GET/PUT/DELETE /:id ────────────

/// GET /api/models/profiles — the profile LIST (no configs; the page fetches
/// the selected profile's config separately). `assigned` carries the sandbox
/// names so the UI can show usage without a second call.
async fn list_profiles(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let stored = {
        let conn = state.db.lock().unwrap();
        read_stored(&conn)?
    };
    let profiles: Vec<serde_json::Value> = stored
        .profiles
        .iter()
        .map(|p| {
            let assigned: Vec<&String> = stored
                .assignments
                .iter()
                .filter(|(_, a)| a.profile.as_deref() == Some(p.id.as_str()))
                .map(|(name, _)| name)
                .collect();
            json!({ "id": p.id, "name": p.name, "version": stored.version, "assigned": assigned })
        })
        .collect();
    Ok(Json(json!({ "profiles": profiles })))
}

/// One profile's masked config + name (id in the path).
async fn get_profile(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut profile = {
        let conn = state.db.lock().unwrap();
        read_stored(&conn)?
            .profile(&id)
            .cloned()
            .ok_or_else(|| ApiError {
                status: StatusCode::NOT_FOUND,
                message: format!("model profile {id:?} not found"),
            })?
    };
    mask_config(&mut profile.config);
    Ok(Json(
        json!({ "id": profile.id, "name": profile.name, "config": profile.config }),
    ))
}

/// POST /api/models/profiles {name} — create an empty profile. The id is
/// backend-owned (gen_profile_id), same split as preset ids.
#[derive(Debug, Deserialize)]
struct CreateProfileBody {
    name: String,
}

async fn create_profile(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateProfileBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad("profile name must not be empty"));
    }
    let conn = state.db.lock().unwrap();
    let mut stored = read_stored(&conn)?;
    let id = gen_profile_id();
    stored.profiles.push(Profile {
        id: id.clone(),
        name: name.to_string(),
        config: CanonicalConfig::default(),
    });
    stored.version += 1;
    write_stored(&conn, &stored)?;
    Ok(Json(json!({ "id": id, "name": name })))
}

/// PUT /api/models/profiles/:id — replace a profile's config (masked-echo
/// merge + validate pipeline, same as PUT /api/models/config) and/or rename
/// it (`name` in the body is optional metadata; absent = keep).
#[derive(Debug, Deserialize)]
struct PutProfileBody {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    config: Option<CanonicalConfig>,
}

async fn put_profile(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<PutProfileBody>,
) -> Result<Json<PutResponse>, ApiError> {
    let guard = state.db.lock().unwrap();
    let mut stored = read_stored(&guard)?;
    let stored_config = stored
        .profile(&id)
        .cloned()
        .ok_or_else(|| ApiError {
            status: StatusCode::NOT_FOUND,
            message: format!("model profile {id:?} not found"),
        })?
        .config;

    if let Some(ref new_name) = body.name {
        let trimmed = new_name.trim();
        if trimmed.is_empty() {
            return Err(ApiError::bad("profile name must not be empty"));
        }
        stored.profile_mut(&id).unwrap().name = trimmed.to_string();
    }
    if let Some(mut incoming) = body.config {
        merge_api_keys(&stored_config, &mut incoming);
        ensure_preset_ids(&mut incoming);
        validate(&incoming).map_err(|errs| ApiError::bad(errs.join("; ")))?;
        incoming.version = 1;
        stored.profile_mut(&id).unwrap().config = incoming;
    }
    stored.version += 1;
    write_stored(&guard, &stored)?;
    drop(guard);
    Ok(Json(PutResponse {
        ok: true,
        warnings: vec![],
    }))
}

/// DELETE /api/models/profiles/:id — refuse the LAST profile (mgr must
/// always have at least one; the models page would have nothing to render)
/// and unassign every sandbox pointing at the deleted one (they fall back to
/// local config: sync 404s, app keeps local — the unbind semantics).
async fn delete_profile(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let guard = state.db.lock().unwrap();
    let mut stored = read_stored(&guard)?;
    if stored.profile(&id).is_none() {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            message: format!("model profile {id:?} not found"),
        });
    }
    if stored.profiles.len() == 1 {
        return Err(ApiError::bad("cannot delete the last model profile"));
    }
    stored.profiles.retain(|p| p.id != id);
    stored
        .assignments
        .retain(|_, a| a.profile.as_deref() != Some(id.as_str()));
    stored.version += 1;
    write_stored(&guard, &stored)?;
    drop(guard);
    Ok(Json(json!({ "ok": true, "id": id })))
}

// ── GET /api/models/sync?name=<sandbox> ────────────────────────────

/// GET /api/models/sync — the sandbox pull endpoint (Phase 4b consumer,
/// unified Phase 4: now sandbox-scoped). Returns the UNMASKED
/// `{version, config}` of the sandbox's ASSIGNED profile: the sandbox app
/// writes the plaintext config to its local canonical store and re-renders
/// agent files, which requires the real keys.
///
/// 404 matrix (design §4.2 — the app treats 404 as "unassigned, keep local"
/// with a debug log, NOT a warn):
///   - `name` absent → 404 (the pull MUST identify itself; pre-upgrade apps
///     that send no name keep their local cache, they never break);
///   - unknown sandbox → 404;
///   - unassigned sandbox → 404.
///
/// SECURITY BOUNDARY (D6/D9, deliberately accepted): this hands out
/// plaintext API keys without authentication. The trust boundary is the
/// host machine — mgr listens on :8089 which the mgr compose does NOT
/// publish to the host, and aio-mgr-net (the only network the sandbox
/// pull uses, via the `mgr-api` alias) is a host-local docker network
/// that never leaves the machine. Same reasoning as the auth-free
/// gateways (D9): personal single-host deployment.
#[derive(Debug, Deserialize)]
struct SyncQuery {
    name: Option<String>,
}

async fn sync(
    State(state): State<Arc<AppState>>,
    Query(q): Query<SyncQuery>,
) -> Result<Json<Value>, ApiError> {
    let unassigned = || ApiError {
        status: StatusCode::NOT_FOUND,
        message: "no model profile assigned (or unknown sandbox)".into(),
    };
    let Some(name) = q.name.as_deref() else {
        return Err(unassigned());
    };
    let stored = {
        let conn = state.db.lock().unwrap();
        // The sandbox must be REGISTERED — an arbitrary caller must not be
        // able to probe names. This also runs the legacy migration lazily
        // (a fresh mgr that never opened the models page still answers the
        // first pull correctly).
        if db::get_sandbox(&conn, name)?.is_none() {
            return Err(unassigned());
        }
        read_stored(&conn)?
    };
    // S2 (D4c): the assignment carries an optional agent subset. `agents`
    // Some([]) = zero agents = the sandbox must NOT render anything — same
    // keep-local 404 path as an unassigned sandbox (AC3). `agents` None =
    // all agents (also the legacy shape). A present assignment with a null
    // profile is treated as unassigned (defensive; PUT removes the row).
    let Some(assign) = stored.assignments.get(name) else {
        return Err(unassigned());
    };
    let Some(profile_id) = assign.profile.as_deref() else {
        return Err(unassigned());
    };
    // S2 (AC3): an explicitly EMPTY agent subset means the sandbox must
    // render nothing — same keep-local 404 path as unassigned.
    if matches!(assign.agents, Some(ref a) if a.is_empty()) {
        return Err(unassigned());
    }
    let profile = stored.profile(profile_id).ok_or_else(unassigned)?;
    Ok(Json(json!({
        "version": stored.version,
        "config": profile.config,
        // None (absent) = all agents; Some(list) = render only these.
        "agents": assign.agents,
    })))
}

// ── POST /api/models/import/pi (profile-scoped) ────────────────────

/// POST /api/models/import/pi — absorb pi's own models.json into the
/// selected profile's canonical library (app import_pi). Path:
/// `MGR_PI_MODELS_FILE` env, else `$HOME/.pi/agent/models.json` (in the
/// containerized form mgr has no ~/.pi — that is EXPECTED; the route is
/// useful in the bare-metal form where mgr runs on the same host as the
/// sandbox user).
async fn import_pi(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ProfileQuery>,
) -> Result<Json<ImportResponse>, ApiError> {
    let pi_path = pi_models_path();

    let guard = state.db.lock().unwrap();
    let mut stored = read_stored(&guard)?;
    let profile_id = stored.selected(q.profile.as_deref())?.id.clone();
    let mut config = stored.profile(&profile_id).unwrap().config.clone();

    let result = import_from_pi(&pi_path, &config).map_err(|e| match e {
        StoreError::Io(err) if err.kind() == std::io::ErrorKind::NotFound => ApiError {
            status: StatusCode::NOT_FOUND,
            message: format!(
                "pi models.json not found at {} (containerized mgr has no ~/.pi; \
                     set MGR_PI_MODELS_FILE or use the bare-metal form)",
                pi_path.display()
            ),
        },
        StoreError::Io(err) => ApiError::internal(format!("read pi models.json: {err}")),
        StoreError::Corrupt(err) => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            message: format!("pi models.json is corrupt: {err}"),
        },
    })?;

    for (id, provider) in result.providers {
        config.providers.insert(id, provider);
    }

    stored.profile_mut(&profile_id).unwrap().config = config;
    stored.version += 1;
    write_stored(&guard, &stored)?;
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

/// Resolve a provider by id from the selected profile's config, or accept
/// literal fields. `baseUrl` is validated non-empty on the literal branch
/// exactly like app's discover (test resolves by id only and 404s an unknown
/// id).
fn resolve_provider(
    config: &CanonicalConfig,
    req: &DiscoverRequest,
) -> Result<ResolvedProvider, ApiError> {
    match req {
        DiscoverRequest::ById { providerId } => {
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
    Query(q): Query<ProfileQuery>,
    Json(req): Json<DiscoverRequest>,
) -> Result<Json<DiscoverResponse>, ApiError> {
    let resolved = {
        let conn = state.db.lock().unwrap();
        let config = read_stored(&conn)?
            .selected(q.profile.as_deref())?
            .config
            .clone();
        // The literal branch validates baseUrl inside (blank -> 400),
        // exactly like the app handler's resolve_provider.
        resolve_provider(&config, &req)?
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
/// store) or literal endpoint fields (a provider being edited in the UI,
/// not yet saved) — the same untagged split as [`DiscoverRequest`], plus
/// the modelId/protocol probe fields shared by both branches. camelCase
/// field names are the wire contract (app test.rs).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
#[allow(non_snake_case)]
enum TestRequest {
    /// Resolve everything from the canonical store by provider id.
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
/// test.rs; the provider lookup reads the kv store instead of models.json.
/// The untagged body mirrors discover: ById resolves the stored provider,
/// Literal probes the fields being edited (pre-save) without touching the
/// store's provider map.
async fn test(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ProfileQuery>,
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
        let config = read_stored(&conn)?
            .selected(q.profile.as_deref())?
            .config
            .clone();
        let resolved = resolve_provider(&config, &req.provider_part())?;
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
    *guard = Some(CatalogCache {
        at: Instant::now(),
        data: data.clone(),
    });
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
            providers.push(CatalogProvider {
                id: provider_id.clone(),
                name,
                api,
                models,
            });
        }
    }
    providers.sort_by(|a, b| a.id.cmp(&b.id));
    CatalogResponse {
        providers,
        fetched_at: String::new(),
    }
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
    let cost = mv
        .get("cost")
        .and_then(Value::as_object)
        .map(|c| CostEntry {
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
        // Full schema (not just kv): the migration + assignment tests need
        // the sandboxes table through the real db helpers.
        let conn = rusqlite::Connection::open_in_memory().expect("open in-memory db");
        db::init_schema(&conn).expect("init schema");
        conn
    }

    /// A single-profile store: the pre-unified shape wrapped as the
    /// "default" profile, no assignments.
    fn single(key: &str) -> StoredModels {
        StoredModels {
            version: 1,
            profiles: vec![Profile {
                id: DEFAULT_PROFILE_ID.into(),
                name: "Default".into(),
                config: sample_config(key),
            }],
            assignments: BTreeMap::new(),
        }
    }

    /// Two profiles with DIFFERENT keys, so sync-resolution tests can tell
    /// them apart on the wire. "default" leads (insertion order).
    fn two_profiles() -> StoredModels {
        StoredModels {
            version: 1,
            profiles: vec![
                Profile {
                    id: DEFAULT_PROFILE_ID.into(),
                    name: "Default".into(),
                    config: sample_config("sk-default-key"),
                },
                Profile {
                    id: "profile-second".into(),
                    name: "Second".into(),
                    config: sample_config("sk-second-key"),
                },
            ],
            assignments: BTreeMap::new(),
        }
    }

    /// Register a sandbox row (sync requires the name to exist in the
    /// sandboxes table; only name + the NOT NULL columns matter here).
    fn register_sandbox(conn: &rusqlite::Connection, name: &str) {
        db::insert_sandbox(
            conn,
            &db::SandboxRow {
                name: name.into(),
                created_at: 0,
                env_json: "{}".into(),
                env_hash: "h".into(),
                cpus: None,
                mem_mb: None,
                status: "running".into(),
                adopted: false,
                external_compose: None,
                services_json: None,
            },
        )
        .expect("insert sandbox row");
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

    /// A full-agent assignment (the `agents: None` shape).
    fn assign(profile: &str) -> StoredAssignment {
        StoredAssignment {
            profile: Some(profile.to_string()),
            agents: None,
        }
    }

    /// An agent-subset assignment (S2).
    fn assign_agents(profile: &str, agents: &[&str]) -> StoredAssignment {
        StoredAssignment {
            profile: Some(profile.to_string()),
            agents: Some(agents.iter().map(|s| s.to_string()).collect()),
        }
    }

    // --- kv round-trip + versioning ---

    #[test]
    fn stored_roundtrip_preserves_plaintext_key() {
        let conn = mem_db();
        // First read on an empty db: fresh store (one empty "default"
        // profile, version 0 — nothing written until a mutation).
        let first = read_stored(&conn).unwrap();
        assert_eq!(first.version, 0);
        assert_eq!(first.profiles.len(), 1);
        assert_eq!(first.profiles[0].id, DEFAULT_PROFILE_ID);
        assert!(first.profiles[0].config.providers.is_empty());
        assert!(first.assignments.is_empty());

        let mut stored = single("sk-plaintext-secret");
        stored.version = 3;
        write_stored(&conn, &stored).unwrap();
        let back = read_stored(&conn).unwrap();
        assert_eq!(back.version, 3);
        assert_eq!(
            back.profiles[0]
                .config
                .providers
                .get("sample")
                .unwrap()
                .api_key
                .as_deref(),
            Some("sk-plaintext-secret"),
            "kv stores the plaintext; masking happens only on the GET path"
        );
    }

    #[test]
    fn put_semantics_version_increments_each_write() {
        // put_config / put_profile / set_assignment all write
        // `stored.version + 1`; first write lands at 1. (The handlers are
        // async/axum-bound; the store-level version arithmetic is the
        // invariant under test.)
        let conn = mem_db();
        let mut stored = single("sk-key-12345678");
        for v in 1..=3 {
            stored.version = v;
            write_stored(&conn, &stored).unwrap();
        }
        assert_eq!(read_stored(&conn).unwrap().version, 3);
        stored.version = 4;
        stored.profiles[0].config.providers.remove("sample");
        write_stored(&conn, &stored).unwrap();
        let back = read_stored(&conn).unwrap();
        assert_eq!(back.version, 4);
        assert!(back.profiles[0].config.providers.is_empty());
    }

    #[test]
    fn put_masked_echo_merge_keeps_plaintext_in_store() {
        // The masked-echo contract: the frontend echoes the mask back; the
        // STORED key must remain the plaintext (merge_api_keys, ported
        // semantics - the mgr PUT handlers run the same three calls).
        let conn = mem_db();
        let stored = sample_config("sk-key-12345678");
        write_stored(&conn, &single("sk-key-12345678")).unwrap();

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
        assert_eq!(
            db::kv_get(&conn, KV_MODELS).unwrap().as_deref(),
            Some("not json{{")
        );
    }

    #[test]
    fn ensure_preset_ids_backfills_on_put_path() {
        // put_config runs ensure_preset_ids between merge and validate; a
        // preset created with an empty id must get one (backend owns ids).
        let mut config = CanonicalConfig::default();
        config
            .providers
            .insert("sample".into(), sample_provider("sk"));
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
        // stored plaintext never reaches the wire on this route. Profile
        // scoping: absent ?profile= selects the FIRST profile; an explicit
        // ?profile= selects that one; an unknown id 404s.
        let state = mgr_state();
        {
            let conn = state.db.lock().unwrap();
            write_stored(&conn, &two_profiles()).unwrap();
        }
        let Json(cfg) = get_config(State(state.clone()), Query(Default::default()))
            .await
            .unwrap();
        let key = cfg
            .providers
            .get("sample")
            .unwrap()
            .api_key
            .clone()
            .unwrap();
        assert_ne!(key, "sk-default-key");
        assert!(key.contains("****"), "masked shape, got {key}");

        let Json(cfg) = get_config(
            State(state.clone()),
            Query(ProfileQuery {
                profile: Some("profile-second".into()),
            }),
        )
        .await
        .unwrap();
        assert_ne!(
            cfg.providers.get("sample").unwrap().api_key.as_deref(),
            Some("sk-second-key")
        );

        let err = get_config(
            State(state.clone()),
            Query(ProfileQuery {
                profile: Some("nope".into()),
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn sync_handler_shape_is_version_plus_unmasked_config() {
        // Wire contract with app/src/mgr_sync.rs SyncPayload {version,
        // config}: exactly these two fields, camelCase provider fields, and
        // the REAL key of the ASSIGNED profile — the sandbox renders native
        // files from this payload.
        let state = mgr_state();
        {
            let conn = state.db.lock().unwrap();
            let mut stored = two_profiles();
            stored.version = 7;
            stored
                .assignments
                .insert("alpha".into(), assign(DEFAULT_PROFILE_ID));
            stored
                .assignments
                .insert("beta".into(), assign("profile-second"));
            register_sandbox(&conn, "alpha");
            register_sandbox(&conn, "beta");
            write_stored(&conn, &stored).unwrap();
        }
        let Json(v) = sync(
            State(state.clone()),
            Query(SyncQuery {
                name: Some("alpha".into()),
            }),
        )
        .await
        .unwrap();
        let obj = v.as_object().expect("sync payload is an object");
        assert_eq!(obj.len(), 3, "exactly {{version, config, agents}}: {obj:?}");
        assert_eq!(v["version"], 7);
        assert_eq!(
            v["config"]["providers"]["sample"]["apiKey"],
            "sk-default-key"
        );
        assert!(
            v.get("agents").unwrap().is_null(),
            "absent subset serializes as null (all agents): {obj:?}"
        );

        // A different sandbox gets a different profile's config — the
        // per-sandbox resolution is the whole point of D8.
        let Json(v) = sync(
            State(state),
            Query(SyncQuery {
                name: Some("beta".into()),
            }),
        )
        .await
        .unwrap();
        assert_eq!(
            v["config"]["providers"]["sample"]["apiKey"],
            "sk-second-key"
        );
    }

    #[tokio::test]
    async fn sync_404_matrix_nameless_unknown_and_unassigned() {
        // The 404 contract (design §4.2/§4.3 — the app's keep-local path):
        // no name, unknown sandbox, and unassigned sandbox all 404. The
        // responses are indistinguishable BY DESIGN — sync must not leak
        // which sandbox names exist to an arbitrary network caller.
        let state = mgr_state();
        {
            let conn = state.db.lock().unwrap();
            let mut stored = two_profiles();
            stored
                .assignments
                .insert("alpha".into(), assign(DEFAULT_PROFILE_ID));
            register_sandbox(&conn, "alpha");
            register_sandbox(&conn, "beta"); // registered but UNassigned
            write_stored(&conn, &stored).unwrap();
        }
        for (label, query) in [
            (
                "no name (pre-MGR_SANDBOX_NAME app)",
                SyncQuery { name: None },
            ),
            (
                "unknown sandbox",
                SyncQuery {
                    name: Some("ghost".into()),
                },
            ),
            (
                "registered, unassigned",
                SyncQuery {
                    name: Some("beta".into()),
                },
            ),
        ] {
            let err = sync(State(state.clone()), Query(query)).await.unwrap_err();
            assert_eq!(err.status, StatusCode::NOT_FOUND, "{label}");
        }
        // The one non-404: assigned and registered.
        assert!(sync(
            State(state),
            Query(SyncQuery {
                name: Some("alpha".into())
            })
        )
        .await
        .is_ok());
    }

    #[tokio::test]
    async fn put_config_handler_merge_validate_and_version_bump() {
        // Handler assembly (app put_config parity): masked-echo merge keeps
        // the stored plaintext, validate gates the write, and the kv version
        // bumps only on success. The edit lands in the SELECTED profile;
        // the other profile is untouched (per-profile isolation).
        let state = mgr_state();
        {
            let conn = state.db.lock().unwrap();
            let mut stored = two_profiles();
            stored.version = 4;
            write_stored(&conn, &stored).unwrap();
        }
        let second = ProfileQuery {
            profile: Some("profile-second".into()),
        };

        // Invalid PUT (assignment references an unknown provider): 400, and
        // the stored version/config are untouched.
        let mut bad = CanonicalConfig::default();
        bad.agents.pi = Some(aio_models::store::AgentAssignment {
            provider: "nope".into(),
            model: "m".into(),
        });
        let err = put_config(State(state.clone()), Query(second.clone()), Json(bad))
            .await
            .unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert!(
            err.message.contains("unknown provider"),
            "validate error surfaced: {}",
            err.message
        );
        {
            let conn = state.db.lock().unwrap();
            assert_eq!(
                read_stored(&conn).unwrap().version,
                4,
                "failed PUT must not bump the version"
            );
        }

        // Valid masked-echo PUT: plaintext survives the merge, version 4→5,
        // and the OTHER profile's key is untouched.
        let incoming = sample_config(&mask_key("sk-second-key"));
        let Json(resp) = put_config(State(state.clone()), Query(second), Json(incoming))
            .await
            .unwrap();
        assert!(resp.ok);
        {
            let conn = state.db.lock().unwrap();
            let stored = read_stored(&conn).unwrap();
            assert_eq!(stored.version, 5);
            assert_eq!(
                stored
                    .profile("profile-second")
                    .unwrap()
                    .config
                    .providers
                    .get("sample")
                    .unwrap()
                    .api_key
                    .as_deref(),
                Some("sk-second-key"),
                "mask echo restores the plaintext on the handler path"
            );
            assert_eq!(
                stored
                    .profile(DEFAULT_PROFILE_ID)
                    .unwrap()
                    .config
                    .providers
                    .get("sample")
                    .unwrap()
                    .api_key
                    .as_deref(),
                Some("sk-default-key"),
                "a profile edit never bleeds into the other profiles"
            );
        }
    }

    // --- unified Phase 4: migration, profiles CRUD, assignment (D8) ---

    #[test]
    fn legacy_models_config_migrates_to_default_profile_with_all_sandboxes_assigned() {
        // Design §4.1: legacy single-config row becomes the "default"
        // profile, EVERY existing sandbox is assigned to it, the new key is
        // written, and the old key is deleted — behavior-identical for all
        // pre-upgrade sandboxes.
        let conn = mem_db();
        register_sandbox(&conn, "alpha");
        register_sandbox(&conn, "beta");
        db::kv_set(
            &conn,
            KV_MODELS_LEGACY,
            &serde_json::to_string(&json!({
                "version": 5,
                "config": sample_config("sk-legacy-key"),
            }))
            .unwrap(),
        )
        .unwrap();

        let stored = read_stored(&conn).unwrap();
        assert_eq!(
            stored.version, 5,
            "version carries over (deep compare tolerates drift)"
        );
        assert_eq!(stored.profiles.len(), 1);
        assert_eq!(stored.profiles[0].id, DEFAULT_PROFILE_ID);
        assert_eq!(
            stored.profiles[0]
                .config
                .providers
                .get("sample")
                .unwrap()
                .api_key
                .as_deref(),
            Some("sk-legacy-key"),
            "the legacy config IS the default profile's config"
        );
        assert_eq!(
            stored.assignments,
            BTreeMap::from([
                ("alpha".to_string(), assign(DEFAULT_PROFILE_ID)),
                ("beta".to_string(), assign(DEFAULT_PROFILE_ID)),
            ]),
            "every pre-existing sandbox keeps pulling exactly what it pulled before"
        );
        assert!(
            db::kv_get(&conn, KV_MODELS_LEGACY).unwrap().is_none(),
            "old key deleted"
        );
        assert!(
            db::kv_get(&conn, KV_MODELS).unwrap().is_some(),
            "new key persisted"
        );
    }

    #[test]
    fn legacy_migration_crash_between_writes_is_idempotent() {
        // The rollback story (design §4.1): if the process dies after
        // write_stored but before kv_del, the next read sees the NEW key and
        // returns it directly — never re-migrating, never double-assigning.
        // And a rolled-back mgr (new key missing, old key present) reads
        // the old key back: both directions safe across an upgrade cycle.
        let conn = mem_db();
        register_sandbox(&conn, "alpha");
        let legacy = serde_json::to_string(&json!({
            "version": 2,
            "config": sample_config("sk-legacy-key"),
        }))
        .unwrap();
        db::kv_set(&conn, KV_MODELS_LEGACY, &legacy).unwrap();

        // First read migrates.
        let migrated = read_stored(&conn).unwrap();
        assert_eq!(migrated.assignments.len(), 1);

        // Simulate a crash-before-del by restoring the old key ALONGSIDE
        // the new one; the new key wins (read path short-circuits).
        db::kv_set(&conn, KV_MODELS_LEGACY, &legacy).unwrap();
        let again = read_stored(&conn).unwrap();
        assert_eq!(again.assignments.len(), 1, "no double-assignment on re-run");
        assert_eq!(
            again.profiles[0]
                .config
                .providers
                .get("sample")
                .unwrap()
                .api_key
                .as_deref(),
            Some("sk-legacy-key")
        );

        // Downgrade direction: new key gone, old key present → old truth.
        db::kv_del(&conn, KV_MODELS).unwrap();
        let rolled_back = read_stored(&conn).unwrap();
        assert_eq!(rolled_back.profiles.len(), 1);
        assert_eq!(
            rolled_back.profiles[0]
                .config
                .providers
                .get("sample")
                .unwrap()
                .api_key
                .as_deref(),
            Some("sk-legacy-key")
        );
    }

    #[tokio::test]
    async fn profile_crud_assign_and_unassign() {
        let state = mgr_state();
        register_sandbox(&state.db.lock().unwrap(), "alpha");

        // POST create: backend-owned id, version bump.
        let Json(created) = create_profile(
            State(state.clone()),
            Json(CreateProfileBody {
                name: "Second".into(),
            }),
        )
        .await
        .unwrap();
        let second_id = created["id"].as_str().unwrap().to_string();
        assert!(!second_id.is_empty(), "backend owns the profile id");
        {
            let conn = state.db.lock().unwrap();
            let stored = read_stored(&conn).unwrap();
            assert_eq!(stored.profiles.len(), 2);
            assert_eq!(stored.profile(&second_id).unwrap().name, "Second");
            assert_eq!(stored.version, 1, "creation bumps the global version");
        }

        // PUT rename + config replace on the new profile.
        let err = put_profile(
            State(state.clone()),
            Path("nope".into()),
            Json(PutProfileBody {
                name: None,
                config: None,
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, StatusCode::NOT_FOUND, "unknown profile id");

        let Json(resp) = put_profile(
            State(state.clone()),
            Path(second_id.clone()),
            Json(PutProfileBody {
                name: Some("Renamed".into()),
                config: Some(sample_config("sk-second-key")),
            }),
        )
        .await
        .unwrap();
        assert!(resp.ok);
        {
            let conn = state.db.lock().unwrap();
            let stored = read_stored(&conn).unwrap();
            assert_eq!(stored.profile(&second_id).unwrap().name, "Renamed");
            assert_eq!(
                stored
                    .profile(&second_id)
                    .unwrap()
                    .config
                    .providers
                    .get("sample")
                    .unwrap()
                    .api_key
                    .as_deref(),
                Some("sk-second-key")
            );
        }

        // DELETE: unassigns sandboxes pointing at it (they fall to local).
        let Json(_) = delete_profile(State(state.clone()), Path(second_id.clone()))
            .await
            .unwrap();
        {
            let conn = state.db.lock().unwrap();
            let stored = read_stored(&conn).unwrap();
            assert!(stored.profile(&second_id).is_none());
        }
        // DELETE the last one: refused.
        let err = delete_profile(State(state.clone()), Path(DEFAULT_PROFILE_ID.into()))
            .await
            .unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert!(
            err.message.contains("last"),
            "refusal message: {}",
            err.message
        );
    }

    #[tokio::test]
    async fn set_assignment_assign_reassign_unassign_and_unknown_profile() {
        let state = mgr_state();
        register_sandbox(&state.db.lock().unwrap(), "alpha");
        {
            let conn = state.db.lock().unwrap();
            let stored = two_profiles();
            write_stored(&conn, &stored).unwrap();
        }

        // Unassigned sandbox: assigned_profile is None.
        {
            let conn = state.db.lock().unwrap();
            assert_eq!(assigned_profile(&conn, "alpha").unwrap(), None);
        }

        // Assign to the second profile; sync resolves THAT config.
        {
            let conn = state.db.lock().unwrap();
            set_assignment(&conn, "alpha", Some("profile-second"), None).unwrap();
        }
        {
            let conn = state.db.lock().unwrap();
            assert_eq!(
                assigned_profile(&conn, "alpha").unwrap().as_deref(),
                Some("profile-second")
            );
        }
        let Json(v) = sync(
            State(state.clone()),
            Query(SyncQuery {
                name: Some("alpha".into()),
            }),
        )
        .await
        .unwrap();
        assert_eq!(
            v["config"]["providers"]["sample"]["apiKey"],
            "sk-second-key"
        );

        // Reassign to default: the next pull gets the default config.
        {
            let conn = state.db.lock().unwrap();
            set_assignment(&conn, "alpha", Some(DEFAULT_PROFILE_ID), None).unwrap();
        }
        let Json(v) = sync(
            State(state.clone()),
            Query(SyncQuery {
                name: Some("alpha".into()),
            }),
        )
        .await
        .unwrap();
        assert_eq!(
            v["config"]["providers"]["sample"]["apiKey"],
            "sk-default-key"
        );

        // Unknown profile: 404, assignment unchanged.
        let err = {
            let conn = state.db.lock().unwrap();
            set_assignment(&conn, "alpha", Some("nope"), None).unwrap_err()
        };
        assert_eq!(err.status, StatusCode::NOT_FOUND);
        {
            let conn = state.db.lock().unwrap();
            assert_eq!(
                assigned_profile(&conn, "alpha").unwrap().as_deref(),
                Some(DEFAULT_PROFILE_ID)
            );
        }

        // Unassign: assigned_profile is None again and sync 404s.
        {
            let conn = state.db.lock().unwrap();
            set_assignment(&conn, "alpha", None, None).unwrap();
        }
        let err = sync(
            State(state),
            Query(SyncQuery {
                name: Some("alpha".into()),
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(
            err.status,
            StatusCode::NOT_FOUND,
            "unassigned = keep-local 404"
        );
    }

    // --- S2: agent-subset assignment (D4c) ---

    #[test]
    fn legacy_bare_string_assignment_reads_as_all_agents() {
        // AC4: a pre-S2 kv payload (`{"<sbx>": "<profile-id>"}`) must parse
        // to `agents: None` = every agent renders, profile preserved.
        let conn = mem_db();
        register_sandbox(&conn, "alpha");
        db::kv_set(
            &conn,
            KV_MODELS,
            &serde_json::to_string(&json!({
                "version": 3,
                "profiles": [ { "id": DEFAULT_PROFILE_ID, "name": "Default",
                                "config": sample_config("sk-key-abcdef") } ],
                "assignments": { "alpha": DEFAULT_PROFILE_ID }
            }))
            .unwrap(),
        )
        .unwrap();
        let stored = read_stored(&conn).unwrap();
        let a = stored.assignments.get("alpha").expect("alpha assigned");
        assert_eq!(a.profile.as_deref(), Some(DEFAULT_PROFILE_ID));
        assert_eq!(
            a.agents, None,
            "bare-string legacy value => None (all agents), AC4"
        );
    }

    #[test]
    fn set_assignment_rejects_unknown_agent_and_accepts_subset() {
        // Whiltelist: an agent outside VALID_AGENTS is 400, store untouched.
        let conn = mem_db();
        register_sandbox(&conn, "alpha");
        {
            let conn = &conn;
            let stored = two_profiles();
            write_stored(conn, &stored).unwrap();
            let err = set_assignment(
                conn,
                "alpha",
                Some(DEFAULT_PROFILE_ID),
                Some(&["pi".to_string(), "skynet".to_string()]),
            )
            .unwrap_err();
            assert_eq!(err.status, StatusCode::BAD_REQUEST);
            assert!(err.message.contains("unknown agent"), "{}", err.message);
            assert_eq!(read_stored(conn).unwrap().version, 1, "rejected write");
        }
        // A valid subset persists agents Some(["pi","opencode"]).
        set_assignment(
            &conn,
            "alpha",
            Some(DEFAULT_PROFILE_ID),
            Some(&["pi".to_string(), "opencode".to_string()]),
        )
        .unwrap();
        let stored = read_stored(&conn).unwrap();
        assert_eq!(
            stored.assignments.get("alpha").unwrap().agents.as_deref(),
            Some(["pi".to_string(), "opencode".to_string()].as_slice())
        );
    }

    #[tokio::test]
    async fn sync_payload_carries_agents_and_empty_agents_404s() {
        // AC2/AC3 wire contract: sync returns `agents` alongside version+config
        // for a subset; Some([]) (zero agents) behaves like unassigned → 404
        // so the sandbox keeps local (AC3).
        let state = mgr_state();
        {
            let conn = state.db.lock().unwrap();
            register_sandbox(&conn, "alpha");
            register_sandbox(&conn, "beta");
            let mut stored = two_profiles();
            stored.assignments.insert(
                "alpha".into(),
                assign_agents(DEFAULT_PROFILE_ID, &["pi", "opencode"]),
            );
            stored
                .assignments
                .insert("beta".into(), assign_agents(DEFAULT_PROFILE_ID, &[]));
            write_stored(&conn, &stored).unwrap();
        }
        // Subset sandbox: agents travels as an array.
        let Json(v) = sync(
            State(state.clone()),
            Query(SyncQuery {
                name: Some("alpha".into()),
            }),
        )
        .await
        .unwrap();
        assert_eq!(v["agents"], json!(["pi", "opencode"]));

        // Zero-agent sandbox: 404 = keep local (AC3).
        let err = sync(
            State(state),
            Query(SyncQuery {
                name: Some("beta".into()),
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(
            err.status,
            StatusCode::NOT_FOUND,
            "Some([]) = keep-local 404"
        );
    }

    #[test]
    fn assigned_profile_helpers_agree_with_subset() {
        // `assignment` (used by sandbox_json) exposes BOTH profile and agents;
        // the full-assignment case keeps the id lookback in sync with
        // `assigned_profile` for the same row.
        let conn = mem_db();
        register_sandbox(&conn, "alpha");
        write_stored(&conn, &two_profiles()).unwrap();
        set_assignment(
            &conn,
            "alpha",
            Some("profile-second"),
            Some(&["claude".to_string()]),
        )
        .unwrap();
        let a = assignment(&conn, "alpha").unwrap().unwrap();
        assert_eq!(a.profile.as_deref(), Some("profile-second"));
        assert_eq!(a.agents.as_deref(), Some(["claude".to_string()].as_slice()));
        assert_eq!(
            assigned_profile(&conn, "alpha").unwrap().as_deref(),
            Some("profile-second")
        );
    }

    #[tokio::test]
    async fn read_assignments_bulk_form_matches_single_lookups() {
        // routes.rs list_sandboxes consumes the bulk form — it must agree
        // with assigned_profile (both run the lazy migration, so a migrated
        // store and a fresh one land the same).
        let state = mgr_state();
        {
            let conn = state.db.lock().unwrap();
            register_sandbox(&conn, "alpha");
            register_sandbox(&conn, "beta");
            let mut stored = two_profiles();
            stored
                .assignments
                .insert("alpha".into(), assign(DEFAULT_PROFILE_ID));
            stored
                .assignments
                .insert("beta".into(), assign("profile-second"));
            write_stored(&conn, &stored).unwrap();
        }
        let bulk = {
            let conn = state.db.lock().unwrap();
            read_assignments(&conn).unwrap()
        };
        assert_eq!(bulk.len(), 2);
        {
            let conn = state.db.lock().unwrap();
            assert_eq!(
                assigned_profile(&conn, "alpha").unwrap().as_deref(),
                Some(DEFAULT_PROFILE_ID)
            );
            assert_eq!(
                assigned_profile(&conn, "beta").unwrap().as_deref(),
                Some("profile-second")
            );
        }
        assert_eq!(
            bulk.get("alpha").map(|a| a.profile.as_deref()),
            Some(Some(DEFAULT_PROFILE_ID))
        );
        assert_eq!(
            bulk.get("beta").map(|a| a.profile.as_deref()),
            Some(Some("profile-second"))
        );
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
            assert!(
                p.ends_with(".pi/agent/models.json"),
                "default is $HOME-relative"
            );
        }
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
        // modelId}` resolves from the store; `{baseUrl, api?, apiKey?,
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
            write_stored(&conn, &single("sk-stored")).unwrap();
        }
        let call = |body: TestRequest| {
            test(
                State(state.clone()),
                Query(ProfileQuery::default()),
                Json(body),
            )
        };

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
