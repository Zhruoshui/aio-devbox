// HTTP API surface (sandbox-mgr Phase 1, design §3.4).
//
// Shape mirrors the sandbox app's /api style: JSON in/out, axum State,
// handlers thin over the db/docker modules. Long operations (create,
// recreate, delete) return 202 + {job} immediately; the client polls
// GET /api/jobs/:id (no SSE - personal-scale polling is enough, design §3.4).

use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::caddy;
use crate::db;
use crate::docker;
use crate::envhash;
use crate::jobs;
use crate::models::StoredAssignment;
use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/scenarios", get(scenarios))
        .route("/api/sandboxes", get(list_sandboxes).post(create_sandbox))
        // Adopt (Phase 5, design §3.8): register an external compose stack.
        // Static segment, so it wins over the :name routes regardless of
        // registration order (same rule as /api/scenarios).
        .route("/api/sandboxes/adopt", post(adopt_sandbox))
        .route(
            "/api/sandboxes/:name",
            get(get_sandbox).put(put_sandbox).delete(delete_sandbox),
        )
        .route("/api/sandboxes/:name/start", post(start_sandbox))
        .route("/api/sandboxes/:name/stop", post(stop_sandbox))
        .route("/api/sandboxes/:name/restart", post(restart_sandbox))
        // On-demand single-service start (D4, unified Phase 3): the one
        // profile-gated sidecar `up` deliberately does not carry. Synchronous
        // (container create+start of a pre-built image, seconds), so no job.
        .route(
            "/api/sandboxes/:name/service/:service/start",
            post(service_start),
        )
        // Model-profile assignment (D8, unified Phase 4): a pure kv write
        // the sandbox's 60s pull picks up — deliberately a SEPARATE route
        // from PUT /api/sandboxes/:name, whose env changes run the recreate
        // job (models.rs set_assignment).
        .route("/api/sandboxes/:name/model_profile", put(put_model_profile))
        .route("/api/sandboxes/:name/entry_url", get(entry_url))
        .route("/api/images", get(list_images))
        .route("/api/images/cleanup", post(cleanup_images))
        .route("/api/images/:env_hash/delete", post(delete_image))
        .route("/api/jobs/:id", get(get_job))
        // Phase 4 model-config routes (models.rs) + usage fan-out (usage.rs).
        // Each module owns its sub-router; merge keeps them ahead of the
        // /api seam below (static segments win either way - merge is the
        // registration-order form of that rule). The sandbox proxy
        // (proxy.rs, sandbox-mgr unified Phase 1) merges the same way - it
        // registers no top-level API routes of its own, just the
        // /api/sbx/:name/*path catch-all.
        .merge(crate::models::router())
        .merge(crate::usage::router())
        .merge(crate::proxy::router())
        // Unmatched /api path: 404 JSON in the ApiError shape, never the SPA
        // (mgr-web's apiError would die on HTML instead of showing the
        // message). Same three-route discipline as app/src/main.rs: matchit
        // 0.7.3's *rest needs the bare /api and /api/ forms listed
        // explicitly, and static segments above always win.
        .route("/api", any(api_not_found))
        .route("/api/", any(api_not_found))
        .route("/api/*rest", any(api_not_found))
        // Belt-and-braces for the seam above: matchit 0.7.3 does not backtrack
        // from a partially-walked subtree to a sibling catch-all, so a
        // trailing-slash form of a routed prefix (e.g. /api/sbx/<name>/ or
        // /api/sandboxes/) matches NO route and lands on the router default
        // fallback. Without this guard, axum's bare 404 would answer there in
        // the API-only test router and the SPA would answer 200 HTML in
        // production - exactly the seam violation api-contracts.md forbids.
        // The SPA must therefore be mounted via explicit routes in main.rs
        // (never fallback_service) so it cannot shadow this guard.
        .fallback(unmatched_fallback)
}

/// Router-level default: an unrouted path is a 404 JSON in the ApiError
/// shape. For /api/* paths this is the trailing-slash gap above (real seam
/// discipline lives in the three explicit routes); for non-/api paths it is
/// what keeps the API-only test router (proxy.rs serve()) HTML-free. main.rs
/// overrides the non-/api half by mounting the SPA on explicit routes.
async fn unmatched_fallback(uri: Uri) -> Response {
    if uri.path().starts_with("/api/") || uri.path() == "/api" {
        api_not_found().await.into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

/// Handler for the /api seam routes above (any method).
async fn api_not_found() -> ApiError {
    ApiError {
        status: StatusCode::NOT_FOUND,
        message: "no such API route".into(),
    }
}

// ── errors ─────────────────────────────────────────────────────────

/// Handler error: status + message JSON. Validation sites use `ApiError::bad`
/// (400, the message is the fix); internal failures go through
/// `From<anyhow::Error>` (500, message is the `anyhow` chain tail).
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad(msg: impl Into<String>) -> Self {
        ApiError {
            status: StatusCode::BAD_REQUEST,
            message: msg.into(),
        }
    }

    /// A non-400 status in the same JSON shape. The lifecycle handlers only
    /// ever need 400/500 (bad / anyhow), but the sandbox proxy (proxy.rs)
    /// is a router: an unknown :name is a genuine 404 and an unreachable
    /// sandbox a genuine 502, all in the one {"error": ...} shape mgr-web
    /// decodes.
    pub fn with_status(status: StatusCode, message: String) -> Self {
        ApiError { status, message }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("{e:#}"),
        }
    }
}

type ApiResult<T> = Result<T, ApiError>;

/// Bridge models.rs' local ApiError twin (the assignment helpers) into this
/// module's type: same status + message, same JSON shape on the wire.
impl From<crate::models::ApiError> for ApiError {
    fn from(e: crate::models::ApiError) -> Self {
        ApiError::with_status(e.status, e.message)
    }
}

// ── scenarios ──────────────────────────────────────────────────────

async fn scenarios(State(state): State<Arc<AppState>>) -> ApiResult<Json<serde_json::Value>> {
    let known =
        aio_config::scenario::scan(&state.repo.join("scenarios")).map_err(ApiError::from)?;
    let mut list: Vec<serde_json::Value> = known
        .iter()
        .map(|s| {
            json!({
                "id": s.meta.id,
                "name": s.meta.name,
                "description": s.meta.description,
                "category": s.meta.category,
                // Install "rung" inside the layer (mise / apt / npm /
                // tarball). EnvPicker groups L3 by this to render the
                // mise-vs-system ladder; the TUI does the same via
                // scenario::installer_rank.
                "installer": s.meta.installer,
                "always_on": s.meta.always_on,
                "default_version": s.meta.default_version,
                "versions": s.meta.versions.iter().map(|v| &v.label).collect::<Vec<_>>(),
            })
        })
        .collect();
    // Layer order like the TUI grouping (scenario.rs category_rank).
    list.sort_by_key(|v| {
        let cat = v["category"].as_str().unwrap_or_default().to_string();
        aio_config::scenario::category_rank(&cat)
    });
    Ok(Json(json!({ "scenarios": list })))
}

// ── sandboxes ──────────────────────────────────────────────────────

/// The four-switch "services" shape the API/UI speak in (S1). Three of the
/// four are also scenarios (pi/pi-web) — normalize_services folds them into
/// env.scenarios before storage, so only code_server/vnc survive as stored
/// Services. Default all-on = the pre-S1 unconditional behavior.
#[derive(Deserialize, Clone)]
struct ServicesBody {
    #[serde(default = "true_default")]
    code_server: bool,
    #[serde(default = "true_default")]
    vnc: bool,
    #[serde(default = "true_default")]
    pi: bool,
    #[serde(default = "true_default")]
    pi_web: bool,
}

fn true_default() -> bool {
    true
}

// Absent services (old client, no field) = all on, the pre-S1 unconditional
// behavior. NOTE: derive(Default) would give all-false — exactly the kind
// of subtle default the S1 contract forbids — so it is written out.
impl Default for ServicesBody {
    fn default() -> Self {
        ServicesBody {
            code_server: true,
            vnc: true,
            pi: true,
            pi_web: true,
        }
    }
}

/// Fold the four-switch services shape into (SandboxEnv, Services):
/// pi/pi_web move into env.scenarios (they ARE scenarios, single source of
/// truth in env_json); pi_web depends on pi (its config lives under the pi
/// install) and vnc (its Chromium is the vnc sidecar's) — enforced here so a
/// caller that turns on pi_web without its deps gets a clear 400 instead of a
/// broken pane.
fn normalize_services(
    env: envhash::SandboxEnv,
    body: Option<&ServicesBody>,
) -> Result<(envhash::SandboxEnv, db::Services), ApiError> {
    let b = body.cloned().unwrap_or_default();
    let mut scenarios = env.scenarios.clone();
    scenarios.retain(|s| s != "pi" && s != "pi-web");
    if b.pi_web {
        // pi-web's dependencies are REQUIRED, not auto-enabled (R1: 400
        // when either is missing): its config lives under the pi install and
        // its Chromium is the vnc sidecar's, so pi_web=true with pi or vnc
        // off is contradictory. The frontend blocks the combination too;
        // this is the backend's defence.
        if !b.pi || !b.vnc {
            return Err(ApiError::bad(
                "pi-web 依赖 pi 与 vnc（Chromium 由 vnc 侧车承载），请同时开启 pi 与 VNC 服务",
            ));
        }
        scenarios.push("pi".into());
        scenarios.push("pi-web".into());
    } else if b.pi {
        scenarios.push("pi".into());
    }
    let env2 = envhash::SandboxEnv {
        scenarios,
        versions: env.versions,
    };
    let services = db::Services {
        code_server: b.code_server,
        vnc: b.vnc,
    };
    Ok((env2, services))
}

/// The four-switch services shape of a stored row: code_server/vnc from
/// services_json, pi/pi_web from env.scenarios. Rows written BEFORE S1
/// (services_json NULL) read back ALL-ON: pre-S1 native rows were built
/// with every service unconditional (pi/pi-web were always_on scenarios
/// then, never listed in env.scenarios — deriving from an empty set would
/// report them absent, and a PUT-recreate would silently strip them),
/// and adopted rows keep the NULL all-on read by design (the adopt flow's
/// documented choice: mgr never builds or toggles for them).
fn installed_services_of(row: &db::SandboxRow) -> ServicesBody {
    let stored = db::services_of(row);
    let pre_s1 = row.services_json.is_none();
    let env: envhash::SandboxEnv = serde_json::from_str(&row.env_json).unwrap_or_default();
    let has = |id: &str| pre_s1 || env.scenarios.iter().any(|s| s == id);
    ServicesBody {
        code_server: stored.code_server,
        vnc: stored.vnc,
        pi: has("pi"),
        pi_web: has("pi-web"),
    }
}

#[derive(Deserialize)]
struct SandboxBody {
    name: String,
    #[serde(default)]
    env: envhash::SandboxEnv,
    #[serde(default)]
    services: Option<ServicesBody>,
    cpus: Option<f64>,
    mem_mb: Option<i64>,
}

/// The name-slug contract shared by create/adopt and the sandbox proxy
/// (proxy.rs: the proxy checks the RAW route segment here before any
/// upstream URL is built - the slug alphabet never needs percent-decoding,
/// so this check is exactly as strict on encoded input).
pub fn validate_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.len() <= 32
        && name.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "invalid name {name:?}: slug of [a-z0-9-], max 32 chars, must start alnum"
        ))
    }
}

/// Registration timestamp (seconds since epoch).
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Resource-limit validation (Phase 3, mgr-web). Negative values are
/// rejected with a fix-it message; `0` is a legal "no limit" sentinel (it is
/// normalized to None on create, and means "clear the current limit" on PUT -
/// see put_sandbox). 0 is meaningless as an actual limit, so no valid input
/// is lost to the sentinel.
fn check_limits(cpus: Option<f64>, mem_mb: Option<i64>) -> Result<(), ApiError> {
    if let Some(c) = cpus {
        if c < 0.0 {
            return Err(ApiError::bad("cpus must be >= 0 (0 = no limit)"));
        }
    }
    if let Some(m) = mem_mb {
        if m < 0 {
            return Err(ApiError::bad("mem_mb must be >= 0 (0 = no limit)"));
        }
    }
    Ok(())
}

/// Create-side normalization: 0 / absent -> None (no limit).
fn limit_or_none(cpus: Option<f64>) -> Option<f64> {
    cpus.filter(|c| *c > 0.0)
}

fn limit_or_none_mb(mem_mb: Option<i64>) -> Option<i64> {
    mem_mb.filter(|m| *m > 0)
}

async fn create_sandbox(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SandboxBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if let Err(msg) = validate_name(&body.name) {
        return Err(ApiError::bad(msg));
    }
    if body.name == "mgr" || body.name.starts_with("sbx-") {
        return Err(ApiError::bad(format!("reserved name {:?}", body.name)));
    }
    // Reject the compose-project collision up front (sbx-<name> project).
    {
        let conn = state.db.lock().unwrap();
        if db::get_sandbox(&conn, &body.name)?.is_some() {
            return Err(ApiError::bad(format!(
                "sandbox {:?} already exists",
                body.name
            )));
        }
    }
    // Fold the four-switch services shape into env + Services (S1): pi/pi_web
    // move into env.scenarios, code_server/vnc become the stored Services.
    let (env, services) = normalize_services(body.env, body.services.as_ref())?;
    // Validate env against the catalog before accepting the job.
    env.to_manifest_checked(&state.repo)
        .map_err(|e| ApiError::bad(format!("{e:#}")))?;
    check_limits(body.cpus, body.mem_mb)?;
    let (cpus, mem_mb) = (limit_or_none(body.cpus), limit_or_none_mb(body.mem_mb));

    let row = db::SandboxRow {
        name: body.name.clone(),
        created_at: now_secs(),
        env_json: env.canonical_json(),
        env_hash: String::new(), // filled by the job on success
        services_json: Some(services.canonical_json()),
        cpus,
        mem_mb,
        status: "creating".into(),
        adopted: false,
        external_compose: None,
    };
    {
        let conn = state.db.lock().unwrap();
        db::insert_sandbox(&conn, &row)?;
    }
    let job_id = jobs::spawn_create(
        state.clone(),
        body.name.clone(),
        env,
        cpus,
        mem_mb,
        false,
        services.canonical_json(),
    )
    .await?;
    Ok(Json(json!({ "job": job_id, "name": body.name })))
}

// ── adopt: register an external stack (Phase 5, design §3.8) ────────

/// Default compose service names of the repo stack (docker-compose.yml).
/// Adopt-body overrides apply at adoption time; later lifecycle ops (the
/// start handler's alias re-connect, un-registration's alias cleanup)
/// resolve containers with these defaults - the sandboxes schema has no
/// column for them, and the standard repo stack is gateway/app.
const DEFAULT_GATEWAY_SERVICE: &str = "gateway";
const DEFAULT_APP_SERVICE: &str = "app";

#[derive(Deserialize)]
struct AdoptBody {
    name: String,
    /// Compose file of the external stack (relative paths are repo-rooted,
    /// see resolve_compose_path).
    compose_path: String,
    #[serde(default)]
    gateway_service: Option<String>,
    #[serde(default)]
    app_service: Option<String>,
}

/// Resolve an adopt-body compose path: RELATIVE paths join the repo root
/// (the wizard documents this), ABSOLUTE paths pass through (bare-metal mgr
/// can point anywhere on the host; the containerized form only sees paths
/// under its read-only repo mount, which the join covers anyway).
fn resolve_compose_path(repo: &FsPath, given: &str) -> PathBuf {
    let p = FsPath::new(given);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        repo.join(p)
    }
}

/// Pick the container name of a compose service from ps rows: a running
/// instance first (aliases must be connectable), any state as fallback
/// (un-registration may run against a stopped stack - disconnecting a
/// stopped container is still valid).
fn find_service_container<'a>(ps: &'a [docker::ComposePsEntry], service: &str) -> Option<&'a str> {
    ps.iter()
        .find(|e| e.service == service && e.state.eq_ignore_ascii_case("running"))
        .or_else(|| ps.iter().find(|e| e.service == service))
        .map(|e| e.name.as_str())
}

/// The registered external compose file of an adopted row, or a 400
/// explaining what is missing (the row exists but the file moved/was
/// deleted - the stack may still be running, we just lost the pointer).
fn external_compose_file(row: &db::SandboxRow) -> Result<PathBuf, ApiError> {
    row.external_compose
        .as_deref()
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .ok_or_else(|| {
            ApiError::bad(format!(
                "adopted sandbox {:?} has no reachable compose file (registered {:?})",
                row.name, row.external_compose
            ))
        })
}

/// Adopt an existing, RUNNING compose stack: read-only management, nothing
/// recreated (design §3.8). Synchronous end to end (seconds: ps + two
/// network connects + a Caddyfile regen), same style as start/stop.
async fn adopt_sandbox(
    State(state): State<Arc<AppState>>,
    Json(body): Json<AdoptBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if let Err(msg) = validate_name(&body.name) {
        return Err(ApiError::bad(msg));
    }
    if body.name == "mgr" || body.name.starts_with("sbx-") {
        return Err(ApiError::bad(format!("reserved name {:?}", body.name)));
    }
    {
        let conn = state.db.lock().unwrap();
        if db::get_sandbox(&conn, &body.name)?.is_some() {
            return Err(ApiError::bad(format!(
                "sandbox {:?} already exists",
                body.name
            )));
        }
    }
    let compose_file = resolve_compose_path(&state.repo, &body.compose_path);
    if !compose_file.exists() {
        return Err(ApiError::bad(format!(
            "compose file not found: {} (relative paths resolve against the repo root {})",
            compose_file.display(),
            state.repo.display()
        )));
    }
    // The stack must be RUNNING: adopting wires subdomain routes at live
    // containers, and a stopped stack has no containers to alias. No `-p`
    // here - the project name is the one compose derives from the file's
    // directory (docker.rs external variants).
    let ps = docker::compose_ps_file(&compose_file).await?;
    if !ps.iter().any(|e| e.state.eq_ignore_ascii_case("running")) {
        return Err(ApiError::bad(format!(
            "no running services in {} - start the stack first (e.g. `make up`), then adopt",
            compose_file.display()
        )));
    }
    let gateway_service = body
        .gateway_service
        .unwrap_or_else(|| DEFAULT_GATEWAY_SERVICE.into());
    let app_service = body
        .app_service
        .unwrap_or_else(|| DEFAULT_APP_SERVICE.into());
    let gateway_ct = find_service_container(&ps, &gateway_service).ok_or_else(|| {
        ApiError::bad(format!(
            "service {gateway_service:?} not found in {}",
            compose_file.display()
        ))
    })?;
    let app_ct = find_service_container(&ps, &app_service).ok_or_else(|| {
        ApiError::bad(format!(
            "service {app_service:?} not found in {}",
            compose_file.display()
        ))
    })?;

    // Shared network + the two mgr aliases. The alias names are LOAD-BEARING
    // and must mirror caddy.rs render's upstreams exactly (the total gateway
    // dials sbx-<name>:8080 and sbx-<name>-piweb:30141 by DNS name): for
    // mgr-created sandboxes composegen bakes them in as network aliases of
    // the same names - adopting an external stack reproduces that membership
    // by hand because its own compose knows nothing of aio-mgr-net.
    docker::ensure_network("aio-mgr-net").await?;
    docker::network_connect_alias("aio-mgr-net", gateway_ct, &format!("sbx-{}", body.name)).await?;
    docker::network_connect_alias("aio-mgr-net", app_ct, &format!("sbx-{}-piweb", body.name))
        .await?;

    // Register (BEFORE regenerate: the Caddyfile renders from the table).
    // env stays empty by design - inferring it from the running stack is
    // explicitly out of scope (design §3.8 "不强制").
    let row = db::SandboxRow {
        name: body.name.clone(),
        created_at: now_secs(),
        env_json: "{}".into(),
        env_hash: String::new(),
        // Adopted stacks keep NULL (= all-on read): the stack brings whatever
        // services it has; mgr never builds or toggles for adopted rows.
        services_json: None,
        cpus: None,
        mem_mb: None,
        status: "running".into(),
        adopted: true,
        external_compose: Some(compose_file.display().to_string()),
    };
    {
        let conn = state.db.lock().unwrap();
        db::insert_sandbox(&conn, &row)?;
    }
    caddy::regenerate(&state).await.map_err(ApiError::from)?;

    // entry_url/piweb_url are deliberately PORT-LESS: the canonical
    // container-network form (caddy Host matching ignores ports; a host
    // publish on :80 needs no port). The browser-facing port — when the mgr
    // UI is reached through a non-default host port like 8081 — is
    // re-attached client-side by mgr-web's withMgrPort
    // (09-10-mgr-subdomain-port-follow). Do not bake a port in here: the
    // backend cannot know which host port the browser used.
    Ok(Json(json!({
        "name": body.name,
        "entry_url": format!("http://sbx-{}.mgr.localhost/", body.name),
        "piweb_url": format!("http://sbx-{}-piweb.mgr.localhost/", body.name),
    })))
}

/// Re-establish both mgr aliases on an external stack's containers (start
/// handler, after compose up). Container names come from a FRESH ps - they
/// change on every recreate, so nothing durable can store them. A missing
/// service is an error the caller surfaces: silently skipping would leave
/// the subdomain routes dead with no hint why.
async fn reconnect_adopted_aliases(
    name: &str,
    ps: &[docker::ComposePsEntry],
) -> Result<(), ApiError> {
    let gateway_ct = find_service_container(ps, DEFAULT_GATEWAY_SERVICE).ok_or_else(|| {
        ApiError::bad(format!(
            "service {DEFAULT_GATEWAY_SERVICE:?} not found - mgr subdomain aliases not restored"
        ))
    })?;
    let app_ct = find_service_container(ps, DEFAULT_APP_SERVICE).ok_or_else(|| {
        ApiError::bad(format!(
            "service {DEFAULT_APP_SERVICE:?} not found - mgr subdomain aliases not restored"
        ))
    })?;
    // Same alias <-> caddy.rs render coupling as in adopt_sandbox.
    docker::network_connect_alias("aio-mgr-net", gateway_ct, &format!("sbx-{name}")).await?;
    docker::network_connect_alias("aio-mgr-net", app_ct, &format!("sbx-{name}-piweb")).await?;
    Ok(())
}

/// Live compose state merged onto a DB row. The DB status is the intent
/// (creating/running/error); `live` reports what compose actually says:
/// "running" / "stopped" / "gone" (no containers) / "unknown" (compose ps
/// itself failed - docker down, stale compose file: shown, never hidden).
/// `model_assignment` is the assigned {profile id, agent subset} or None (D8;
/// S2: `model_agents` carries the subset — null = all agents). Caller resolves
/// it once per list request — the store parse is not per-row free.
async fn sandbox_json(
    state: &Arc<AppState>,
    row: &db::SandboxRow,
    model_assignment: Option<StoredAssignment>,
) -> serde_json::Value {
    // S1 installed services (see installed_services_of): code_server/vnc
    // from services_json, pi/pi-web from the scenario set, all-on for
    // pre-S1/adopted rows (services_json NULL).
    let inst = installed_services_of(row);
    // ps target: the mgr-generated compose for native rows; for adopted rows
    // the REGISTERED external file - external_compose is the single truth for
    // where an adopted stack lives (nothing else records it), and a missing
    // file reports live=unknown, the same "pointer lost, stack may well be
    // up" semantics as a ps failure.
    let ps_result = if row.adopted {
        match row
            .external_compose
            .as_deref()
            .map(PathBuf::from)
            .filter(|p| p.exists())
        {
            Some(file) => docker::compose_ps_file(&file).await,
            None => Err(anyhow::anyhow!("external compose file missing")),
        }
    } else {
        let compose_file = state.instance_dir(&row.name).join("compose.yml");
        docker::compose_ps(&envhash::project_name(&row.name), &compose_file).await
    };
    // A ps FAILURE is not "gone": the containers may well be running and the
    // daemon is merely unreachable. Mapping the error to an empty ps list
    // would make a docker outage render every sandbox as "gone" (observed
    // contract drift: types.ts/liveLabel in mgr-web carry an "unknown" badge
    // that this branch is the only producer of).
    let (live, ps) = match ps_result {
        Ok(ps) => {
            let running = ps.iter().any(|e| e.state.eq_ignore_ascii_case("running"));
            let live = if ps.is_empty() {
                "gone"
            } else if running {
                "running"
            } else {
                "stopped"
            };
            (live, ps)
        }
        Err(e) => {
            tracing::warn!(sandbox = %row.name, error = %format!("{e:#}"), "compose ps failed");
            ("unknown", Vec::new())
        }
    };
    let short_hash = if row.env_hash.len() >= 12 {
        &row.env_hash[..12]
    } else {
        ""
    };
    // Adopted rows own no image: they run whatever the external compose
    // pins. The literal keeps the list page's image column meaningful.
    let image = if row.adopted {
        "external".to_string()
    } else {
        format!("sandbox-app-{short_hash}")
    };
    json!({
        "name": row.name,
        "status": row.status,
        "live": live,
        "adopted": row.adopted,
        "created_at": row.created_at,
        "cpus": row.cpus,
        "mem_mb": row.mem_mb,
        "env": serde_json::from_str::<serde_json::Value>(&row.env_json).unwrap_or(json!({})),
        "image": image,
        // S1: installed services (installed_services_of). Read-only on the
        // frontend — fixed by the image content at create time.
        "installed_services": {
            "code_server": inst.code_server,
            "vnc": inst.vnc,
            "pi": inst.pi,
            "pi_web": inst.pi_web,
        },
        // The sbx- prefix must mirror the total-gateway site blocks exactly
        // (caddy.rs render) - the sandbox-net alias is also sbx-<name>/
        // sbx-<name>-piweb (composegen), so the prefix is the shared identity.
        // Port-less on purpose - see the adopt handler's note above: the
        // browser port is re-attached client-side by mgr-web's withMgrPort.
        "entry_url": format!("http://sbx-{}.mgr.localhost/", row.name),
        "piweb_url": format!("http://sbx-{}-piweb.mgr.localhost/", row.name),
        // Assigned model profile (D8): null = unassigned (sandbox keeps its
        // local models.json untouched).
        // Assigned model profile (D8): null = unassigned (sandbox keeps its
        // local models.json untouched).
        "model_profile": model_assignment.as_ref().and_then(|a| a.profile.clone()),
        // S2 (D4c): assigned agent subset — null = all agents render,
        // [] = none (sandbox keeps local), [..] = render exactly these.
        "model_agents": model_assignment.and_then(|a| a.agents),
        "services": ps.iter().map(|e| json!({
            "service": e.service, "name": e.name, "state": e.state, "status": e.status,
        })).collect::<Vec<_>>(),
    })
}

async fn list_sandboxes(State(state): State<Arc<AppState>>) -> ApiResult<Json<serde_json::Value>> {
    let rows = {
        let conn = state.db.lock().unwrap();
        let rows = db::list_sandboxes(&conn)?;
        // Profile assignments resolved in ONE store parse per list call (the
        // models store can be sizeable; assigned_profile would re-parse it
        // per row — models.rs read_assignments is the bulk form).
        let stored = crate::models::read_assignments(&conn)?;
        let assignments: std::collections::HashMap<String, crate::models::StoredAssignment> = rows
            .iter()
            .filter_map(|r| stored.get(&r.name).cloned().map(|a| (r.name.clone(), a)))
            .collect();
        (rows, assignments)
    };
    let mut out = Vec::with_capacity(rows.0.len());
    for row in &rows.0 {
        let assignment = rows.1.get(&row.name).cloned();
        out.push(sandbox_json(&state, row, assignment).await);
    }
    Ok(Json(json!({ "sandboxes": out })))
}

async fn get_sandbox(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let row = {
        let conn = state.db.lock().unwrap();
        db::get_sandbox(&conn, &name)?
            .ok_or_else(|| ApiError::bad(format!("sandbox {name:?} not found")))?
    };
    let assignment = {
        let conn = state.db.lock().unwrap();
        crate::models::assignment(&conn, &name)?
    };
    Ok(Json(sandbox_json(&state, &row, assignment).await))
}

#[derive(Deserialize)]
struct PutBody {
    env: Option<envhash::SandboxEnv>,
    // services is intentionally NOT accepted on PUT: the installed set is
    // fixed by the image content at create time (S1), so it is immutable
    // here. The UI renders it read-only; an unknown field is ignored by
    // serde, but the field is declared for the API contract's clarity.
    #[serde(default)]
    services: Option<ServicesBody>,
    cpus: Option<f64>,
    mem_mb: Option<i64>,
}

async fn put_sandbox(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<PutBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let row = {
        let conn = state.db.lock().unwrap();
        db::get_sandbox(&conn, &name)?
            .ok_or_else(|| ApiError::bad(format!("sandbox {name:?} not found")))?
    };
    if row.adopted {
        return Err(ApiError::bad(
            "adopted external stack: env/resource changes unsupported (design §3.8)",
        ));
    }
    check_limits(body.cpus, body.mem_mb)?;
    // Merge: absent fields keep current values; an EXPLICIT 0 CLEARS a
    // resource limit (the UI's "empty field = unlimited" - a plain null
    // would silently keep the old limit, which is not what a cleared input
    // means). A new env hash drives the recreate flow (design §3.6 PUT).
    // body.services is deliberately IGNORED: the installed set is fixed at
    // create time (image content). Named here so the API contract stays
    // explicit that it accepted-but-ignored (serde would silently skip an
    // undeclared field anyway; declaring it documents the intent).
    let _ = &body.services;
    let env = body
        .env
        .unwrap_or_else(|| serde_json::from_str(&row.env_json).expect("env_json roundtrips"));
    let cpus = match body.cpus {
        Some(c) if c > 0.0 => Some(c),
        Some(_) => None,  // explicit 0 = clear
        None => row.cpus, // absent = keep
    };
    let mem_mb = match body.mem_mb {
        Some(m) if m > 0 => Some(m),
        Some(_) => None,
        None => row.mem_mb,
    };
    // S1: services are immutable on PUT, but the client's env swap must not
    // drop the pi/pi-web scenarios — they live in env_json and are hidden
    // from the env editor (the services area owns them). Re-normalize with
    // the CURRENT row's four-switch shape (installed_services_of): pre-S1
    // rows read all-on, so their first PUT-recreate bakes pi/pi-web again —
    // same assembly bytes, same hash, image reuse — instead of silently
    // stripping them (AC4).
    let (env, services) = normalize_services(env, Some(&installed_services_of(&row)))?;
    let services_json = services.canonical_json();
    env.to_manifest_checked(&state.repo)
        .map_err(|e| ApiError::bad(format!("{e:#}")))?;

    db::update_sandbox_status(&state.db.lock().unwrap(), &name, "creating")?;
    let job_id = jobs::spawn_create(
        state.clone(),
        name.clone(),
        env,
        cpus,
        mem_mb,
        true,
        services_json,
    )
    .await?;
    Ok(Json(json!({ "job": job_id, "name": name })))
}

#[derive(Deserialize)]
struct DeleteQuery {
    volumes: Option<String>,
}

async fn delete_sandbox(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Query(q): Query<DeleteQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let volumes = matches!(q.volumes.as_deref(), Some("1") | Some("true"));
    let row = {
        let conn = state.db.lock().unwrap();
        db::get_sandbox(&conn, &name)?
            .ok_or_else(|| ApiError::bad(format!("sandbox {name:?} not found")))?
    };
    // Adopted rows take the synchronous un-registration path: there is no
    // compose to tear down, so no job and no progress view - just {ok}. The
    // UI knows to stay on the list (types.ts DeleteReply).
    if row.adopted {
        return unadopt_sandbox(state, row).await;
    }
    // Row removal happens inside the job on success.
    let job_id = jobs::spawn_delete(state.clone(), name.clone(), volumes).await?;
    Ok(Json(
        json!({ "job": job_id, "name": name, "volumes": volumes }),
    ))
}

/// Un-register an adopted external stack (design §3.8): remove the row +
/// routes ONLY - containers/volumes are not ours to delete. The two
/// aio-mgr-net aliases are best-effort disconnected so a NEW sandbox (or a
/// re-adopt) with the same name does not collide with a stale alias;
/// container names are only knowable live, so ps runs again here, and a ps
/// failure never blocks the un-registration (only the disconnect is lost -
/// a warning; network_connect_alias repairs a stale alias on re-adopt).
async fn unadopt_sandbox(
    state: Arc<AppState>,
    row: db::SandboxRow,
) -> Result<Json<serde_json::Value>, ApiError> {
    {
        let conn = state.db.lock().unwrap();
        db::delete_sandbox(&conn, &row.name)?;
        // Same assignment hygiene as the native delete path (jobs.rs): an
        // adopted row's assignment is usually inert (no MGR_URL), but it is
        // recorded and would be inherited by a same-name sandbox later.
        if let Err(e) = crate::models::set_assignment(&conn, &row.name, None, None) {
            tracing::warn!(sandbox = %row.name, error = %e.message, "assignment cleanup failed");
        }
    }
    if let Some(file) = row
        .external_compose
        .as_deref()
        .map(PathBuf::from)
        .filter(|p| p.exists())
    {
        match docker::compose_ps_file(&file).await {
            Ok(ps) => {
                if let Some(ct) = find_service_container(&ps, DEFAULT_GATEWAY_SERVICE) {
                    docker::network_disconnect("aio-mgr-net", ct).await;
                }
                if let Some(ct) = find_service_container(&ps, DEFAULT_APP_SERVICE) {
                    docker::network_disconnect("aio-mgr-net", ct).await;
                }
            }
            Err(e) => tracing::warn!(
                sandbox = %row.name,
                error = %format!("{e:#}"),
                "compose ps failed while un-adopting; aliases left as-is"
            ),
        }
    }
    // Total gateway: drop this sandbox's site pair (after the row removal,
    // the regeneration no longer sees it).
    caddy::regenerate(&state).await.map_err(ApiError::from)?;
    Ok(Json(json!({ "ok": true, "name": row.name })))
}

/// Fetch a sandbox row or 404-shaped 400 (the lifecycle handlers' prologue).
fn require_row(state: &Arc<AppState>, name: &str) -> Result<db::SandboxRow, ApiError> {
    let conn = state.db.lock().unwrap();
    db::get_sandbox(&conn, name)?
        .ok_or_else(|| ApiError::bad(format!("sandbox {name:?} not found")))
}

/// Start = compose up -d (idempotent; images already exist for a registered
/// sandbox). Short enough to be synchronous. Adopted rows go through the
/// external file (no `-p`) and MUST re-connect the two mgr aliases after
/// up: the external compose does not know about aio-mgr-net, so a full
/// down/up recreates containers WITHOUT the aliases and the subdomain
/// routes would silently die on the first stop/start cycle (design §6
/// known cost - idempotent thanks to network_connect_alias's
/// disconnect-reconnect on "already exists").
async fn start_sandbox(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let row = require_row(&state, &name)?;
    let out = if row.adopted {
        let compose_file = external_compose_file(&row)?;
        docker::ensure_network("aio-mgr-net").await?;
        let out = docker::compose_up_file(&compose_file).await?;
        let ps = docker::compose_ps_file(&compose_file).await?;
        reconnect_adopted_aliases(&name, &ps).await?;
        out
    } else {
        let compose_file = state.instance_dir(&name).join("compose.yml");
        if !compose_file.exists() {
            return Err(ApiError::bad(format!(
                "sandbox {name:?} has no compose file"
            )));
        }
        docker::ensure_network("aio-mgr-net").await?;
        // S1: vnc profile only when the sandbox has the vnc service.
        let svc = db::services_of(&row);
        docker::compose_up(&envhash::project_name(&name), &compose_file, false, svc.vnc).await?
    };
    db::update_sandbox_status(&state.db.lock().unwrap(), &name, "running")?;
    Ok(Json(json!({ "ok": true, "output": out.trim() })))
}

async fn stop_sandbox(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let row = require_row(&state, &name)?;
    let out = if row.adopted {
        docker::compose_stop_file(&external_compose_file(&row)?).await?
    } else {
        let compose_file = state.instance_dir(&name).join("compose.yml");
        docker::compose_stop(&envhash::project_name(&name), &compose_file).await?
    };
    db::update_sandbox_status(&state.db.lock().unwrap(), &name, "stopped")?;
    Ok(Json(json!({ "ok": true, "output": out.trim() })))
}

async fn restart_sandbox(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let row = require_row(&state, &name)?;
    let out = if row.adopted {
        docker::compose_restart_file(&external_compose_file(&row)?).await?
    } else {
        let compose_file = state.instance_dir(&name).join("compose.yml");
        docker::compose_restart(&envhash::project_name(&name), &compose_file).await?
    };
    db::update_sandbox_status(&state.db.lock().unwrap(), &name, "running")?;
    Ok(Json(json!({ "ok": true, "output": out.trim() })))
}

/// On-demand services (D4): the profile-gated sidecars `up` deliberately
/// does not carry. The second tuple item is the service's OWN profile flag
/// (compose only sees a profile-gated service when its profile is active).
/// The whitelist is closed - an arbitrary service name must never reach a
/// compose argv.
const ON_DEMAND_SERVICES: [(&str, [&str; 2]); 1] = [("code-server", docker::CODE_SERVER_PROFILE)];

/// Compose-ps truth for the service-start guard, mirroring sandbox_json's
/// live mapping ("running" = any running entry; empty ps = "gone"). A ps
/// ERROR propagates as 500 before this helper - same honesty as the other
/// lifecycle handlers.
fn ps_live(ps: &[docker::ComposePsEntry]) -> &'static str {
    if ps.iter().any(|e| e.state.eq_ignore_ascii_case("running")) {
        "running"
    } else if ps.is_empty() {
        "gone"
    } else {
        "stopped"
    }
}

fn not_running(name: &str, service: &str, live: &str) -> ApiError {
    ApiError::bad(format!(
        "sandbox {name:?} is {live} - start the sandbox before starting {service:?}"
    ))
}

/// POST /api/sandboxes/:name/service/:service/start (D4): bring up ONE
/// on-demand service of a RUNNING sandbox (currently code-server; native
/// and adopted rows alike - an adopted stack's compose may gate its own
/// code-server behind a profile, same mechanics). Synchronous: container
/// create+start of a pre-built image is seconds, no job (design §3.2).
///
/// The liveness guard is load-bearing (verified live on compose 5.2.0):
/// `up -d <svc>` also starts the service's DEPENDENCIES (app, via
/// network_mode: service:app), so against a stopped stack it would produce
/// a half-started sandbox (app + code-server up, gateway + vnc down). The
/// workspace tree greys stopped sandboxes out, but a pane restored from a
/// saved layout mounts without going through the tree - the guard holds the
/// invariant here instead. `up -d <svc>` itself is idempotent and heals a
/// stale container (docker.rs compose_service_up); probe-before-start is
/// the pane's UX concern, mgr does not double-guess.
///
/// No service-stop route by design (prd D4): pane close keeps the code-
/// server session; the explicit stop is the sandbox's own stop/restart,
/// which still carries the full profile set (契约 4) and takes code-server
/// down with it.
async fn service_start(
    State(state): State<Arc<AppState>>,
    Path((name, service)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let Some((_, profile)) = ON_DEMAND_SERVICES.iter().find(|(svc, _)| *svc == service) else {
        return Err(ApiError::with_status(
            StatusCode::NOT_FOUND,
            format!("unknown on-demand service {service:?}"),
        ));
    };
    // 404-shaped unknown-sandbox (design §3.2 "404 未知服务/沙箱"), not the
    // sibling lifecycle handlers' 400-shaped require_row: both path segments
    // name resources here, the same choice as the sandbox proxy's :name.
    let row = {
        let conn = state.db.lock().unwrap();
        db::get_sandbox(&conn, &name)?.ok_or_else(|| {
            ApiError::with_status(StatusCode::NOT_FOUND, format!("sandbox {name:?} not found"))
        })?
    };
    // S1: on-demand services are only on-demand when INSTALLED — a sandbox
    // created without code-server has no service block, no image, no pane;
    // starting it would 502 against a nonexistent service. (The whitelist
    // above currently admits only code-server; a future addition repeats
    // this installed-check.)
    if !row.adopted && service == "code-server" && !db::services_of(&row).code_server {
        return Err(ApiError::bad(format!(
            "{service:?} 未安装到此沙箱（创建时的服务开关已关闭）"
        )));
    }
    let out = if row.adopted {
        let compose_file = external_compose_file(&row)?;
        let live = ps_live(&docker::compose_ps_file(&compose_file).await?);
        if live != "running" {
            return Err(not_running(&name, &service, live));
        }
        docker::compose_service_up_file(&compose_file, profile, &service).await?
    } else {
        let compose_file = state.instance_dir(&name).join("compose.yml");
        if !compose_file.exists() {
            return Err(ApiError::bad(format!(
                "sandbox {name:?} has no compose file"
            )));
        }
        let project = envhash::project_name(&name);
        let live = ps_live(&docker::compose_ps(&project, &compose_file).await?);
        if live != "running" {
            return Err(not_running(&name, &service, live));
        }
        docker::compose_service_up(&project, &compose_file, profile, &service).await?
    };
    Ok(Json(
        json!({ "ok": true, "service": service, "output": out.trim() }),
    ))
}

/// PUT /api/sandboxes/:name/model_profile (D8): assign or unassign the
/// sandbox's model profile. Body `{profile: "<id>"}` assigns; `{profile:
/// null}` (or the field absent) UNASSIGNS — the same explicit-null-not-
/// absence discipline as the limits tri-state (types.ts), because "keep
/// current" has no meaning for a PUT that exists to change it.
///
/// Synchronous pure-kv write (models.rs set_assignment): the sandbox's 60s
/// pull picks the change up — NO recreate job, containers keep running.
/// Works for adopted rows too: the assignment is mgr-side state only (an
/// adopted sandbox without MGR_URL never pulls, so the assignment is inert
/// there — recorded anyway so re-registering the stack under a future
/// managed compose is seamless).
#[derive(Deserialize)]
struct ModelProfileBody {
    profile: Option<String>,
    /// S2 (D4c): the agent subset to render. Absent/null = ALL agents (also
    /// the legacy-client meaning); `[]` = none (sandbox keeps local).
    #[serde(default)]
    agents: Option<Vec<String>>,
}

async fn put_model_profile(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<ModelProfileBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // ONE db lock for check + write: two separate acquisitions would let a
    // concurrent delete drop the sandbox between them, recording an
    // assignment for a row that no longer exists.
    {
        let conn = state.db.lock().unwrap();
        if db::get_sandbox(&conn, &name)?.is_none() {
            return Err(ApiError::bad(format!("sandbox {name:?} not found")));
        }
        crate::models::set_assignment(
            &conn,
            &name,
            body.profile.as_deref(),
            body.agents.as_deref(),
        )?;
    }
    Ok(Json(json!({
        "ok": true, "name": name, "model_profile": body.profile,
        "model_agents": body.agents,
    })))
}

async fn entry_url(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    {
        let conn = state.db.lock().unwrap();
        if db::get_sandbox(&conn, &name)?.is_none() {
            return Err(ApiError::bad(format!("sandbox {name:?} not found")));
        }
    }
    // sbx- prefix: mirrors the total-gateway site blocks (caddy.rs render).
    // Port-less on purpose - see the adopt handler's note above: the browser
    // port is re-attached client-side by mgr-web's withMgrPort.
    Ok(Json(json!({
        "entry": format!("http://sbx-{name}.mgr.localhost/"),
        "piweb": format!("http://sbx-{name}-piweb.mgr.localhost/"),
    })))
}

// ── images / jobs ──────────────────────────────────────────────────

/// GET /api/images — one row per recorded image (S3: + combo description and
/// live size. `combo` is NULL on pre-S3 rows (the frontend falls back to the
/// env_hash); `size_bytes` is a live docker inspect that the frontend shows
/// as "—" when it fails / the image is gone, never an error here).
async fn list_images(State(state): State<Arc<AppState>>) -> ApiResult<Json<serde_json::Value>> {
    let rows = {
        let conn = state.db.lock().unwrap();
        db::list_images(&conn)?
    };
    let mut out = Vec::with_capacity(rows.len());
    for (env_hash, tag, built_at, build_log, combo) in rows {
        let refcount = {
            let conn = state.db.lock().unwrap();
            db::image_refcount(&conn, &env_hash)?
        };
        // Live size of the base image (the row's tag): best-effort, "—" on
        // failure (image deleted behind our back, docker down).
        let size_bytes = docker::image_size(&tag).await.ok();
        out.push(json!({
            "env_hash": env_hash,
            "tag": tag,
            "built_at": built_at,
            "refcount": refcount,
            "build_log": build_log,
            "combo": combo,
            "size_bytes": size_bytes,
        }));
    }
    Ok(Json(json!({ "images": out })))
}

/// POST /api/images/:env_hash/delete — delete one recorded image's tag group
/// (base/app/code-server), 202 + job (S3 R3: async, progress via GET
/// /api/jobs/:id). Pre-check is synchronous: unknown row 404, referenced
/// image 409 (refcount>0 — also covers a create/recreate in flight, whose
/// sandbox already wrote env_hash by the time it holds the image).
async fn delete_image(
    State(state): State<Arc<AppState>>,
    Path(h): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    if !valid_env_hash(&h) {
        return Err(ApiError::bad("env_hash must be 64 hex chars"));
    }
    {
        let conn = state.db.lock().unwrap();
        // row existence + refcount in one lock: check row first (404), then
        // the count (409). A missing row = already deleted = 404 (client
        // refreshes).
        let exists: bool = db::list_images(&conn)?
            .iter()
            .any(|(eh, _, _, _, _)| eh == &h);
        if !exists {
            return Err(ApiError::bad(format!("image {h} not found")));
        }
        let rc = db::image_refcount(&conn, &h)?;
        if rc > 0 {
            return Err(ApiError {
                status: StatusCode::CONFLICT,
                message: format!("image is referenced by {rc} sandbox(es)"),
            });
        }
    }
    let job = crate::jobs::spawn_image_delete(state.clone(), h).await?;
    Ok(Json(json!({ "ok": true, "job": job })))
}

/// POST /api/images/cleanup — delete every refcount=0 image group + builder
/// cache, 202 + job (S3 R4).
async fn cleanup_images(State(state): State<Arc<AppState>>) -> ApiResult<Json<serde_json::Value>> {
    let job = crate::jobs::spawn_image_cleanup(state.clone()).await?;
    Ok(Json(json!({ "ok": true, "job": job })))
}

/// env_hash is a full sha256 hex digest (64 chars).
fn valid_env_hash(h: &str) -> bool {
    h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit())
}

async fn get_job(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    let job = {
        let jobs = state.jobs.lock().unwrap();
        jobs.get(&id).cloned()
    };
    match job {
        // Snapshot the cell under its tokio lock (the runner task may be
        // appending to the log concurrently).
        Some(cell) => {
            let j = cell.lock().await.clone();
            Ok(Json(serde_json::to_value(&j).unwrap()))
        }
        None => Err(ApiError::bad(format!("job {id} not found"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_compose_path_relative_is_repo_rooted() {
        let repo = FsPath::new("/repo");
        assert_eq!(
            resolve_compose_path(repo, "docker-compose.yml"),
            PathBuf::from("/repo/docker-compose.yml")
        );
        // Nested relative paths keep their components.
        assert_eq!(
            resolve_compose_path(repo, "stacks/main.yml"),
            PathBuf::from("/repo/stacks/main.yml")
        );
    }

    #[test]
    fn resolve_compose_path_absolute_passes_through() {
        // Bare-metal mgr may point anywhere on the host.
        assert_eq!(
            resolve_compose_path(FsPath::new("/repo"), "/opt/stack/compose.yml"),
            PathBuf::from("/opt/stack/compose.yml")
        );
    }

    #[test]
    fn adopt_body_defaults_services_to_none() {
        // The wire contract: gateway_service/app_service are OPTIONAL
        // (defaults live in the handler); name/compose_path are required.
        let b: AdoptBody =
            serde_json::from_str(r#"{"name":"legacy","compose_path":"docker-compose.yml"}"#)
                .unwrap();
        assert_eq!(b.name, "legacy");
        assert_eq!(b.compose_path, "docker-compose.yml");
        assert_eq!(b.gateway_service, None);
        assert_eq!(b.app_service, None);

        let b: AdoptBody = serde_json::from_str(
            r#"{"name":"legacy","compose_path":"/x.yml","gateway_service":"gw","app_service":"web"}"#,
        )
        .unwrap();
        assert_eq!(b.gateway_service.as_deref(), Some("gw"));
        assert_eq!(b.app_service.as_deref(), Some("web"));

        assert!(serde_json::from_str::<AdoptBody>(r#"{"name":"legacy"}"#).is_err());
    }

    #[test]
    fn find_service_container_prefers_running() {
        // Scale>1 or a stale exited entry: the running container is the one
        // that can take a network alias.
        let ps = vec![
            entry("a-gateway-1", "gateway", "exited"),
            entry("a-gateway-2", "gateway", "running"),
            entry("a-app-1", "app", "running"),
        ];
        assert_eq!(find_service_container(&ps, "gateway"), Some("a-gateway-2"));
        assert_eq!(find_service_container(&ps, "app"), Some("a-app-1"));
        assert_eq!(find_service_container(&ps, "vnc"), None);
    }

    #[test]
    fn find_service_container_falls_back_to_any_state() {
        // Un-registration may run against a stopped stack: disconnecting a
        // stopped container from a network is still valid, so any entry of
        // the service beats None.
        let ps = vec![entry("a-gateway-1", "gateway", "exited")];
        assert_eq!(find_service_container(&ps, "gateway"), Some("a-gateway-1"));
    }

    // ── S1 normalize_services (four-switch -> env + Services) ────────

    fn env_with(scenarios: &[&str]) -> envhash::SandboxEnv {
        envhash::SandboxEnv {
            scenarios: scenarios.iter().map(|s| s.to_string()).collect(),
            versions: Default::default(),
        }
    }

    /// unwrap for ApiError (no Debug impl): panic with the message instead.
    fn norm(
        env: envhash::SandboxEnv,
        body: Option<&ServicesBody>,
    ) -> (envhash::SandboxEnv, db::Services) {
        match normalize_services(env, body) {
            Ok(v) => v,
            Err(e) => panic!("normalize_services failed: {} {}", e.status, e.message),
        }
    }

    #[test]
    fn normalize_none_body_defaults_all_on_and_merges_stored_scenarios() {
        // Absent services (old client / absent field) = the pre-S1
        // unconditional behavior: everything on, existing scenario list
        // keeps any pi/pi-web entries it already carries (they survive as
        // stored scenarios, unlike a fresh normalize that rewrites them).
        let env = env_with(&["node", "pi", "pi-web"]);
        let (env2, svc) = norm(env, None);
        assert_eq!(env2.scenarios, vec!["node", "pi", "pi-web"]);
        assert_eq!(
            svc,
            db::Services {
                code_server: true,
                vnc: true
            }
        );
    }

    #[test]
    fn normalize_pi_off_removes_pi_and_pi_web_from_scenarios() {
        let body = ServicesBody {
            code_server: true,
            vnc: true,
            pi: false,
            pi_web: false,
        };
        let (env2, _) = norm(env_with(&["node", "pi", "pi-web"]), Some(&body));
        // pi / pi-web dropped, other scenarios retained. Services still
        // carry the code_server/vnc switches untouched.
        assert_eq!(env2.scenarios, vec!["node"]);
        // Stored Services reflect only the two compose switches.
        let (_, svc) = norm(env_with(&["node"]), Some(&body));
        assert_eq!(
            svc,
            db::Services {
                code_server: true,
                vnc: true
            }
        );
    }

    #[test]
    fn normalize_pi_web_on_requires_pi_and_vnc() {
        // R1: pi_web=true with pi or vnc off must 400 (pi-web's config lives
        // under the pi install; its Chromium is the vnc sidecar's) - before
        // any scenario rewriting.
        for (pi, vnc) in [(false, true), (true, false), (false, false)] {
            let body = ServicesBody {
                code_server: false,
                vnc,
                pi,
                pi_web: true,
            };
            assert!(
                normalize_services(env_with(&[]), Some(&body)).is_err(),
                "pi_web with pi={pi} vnc={vnc} must 400"
            );
        }

        // Both deps on: scenarios carry pi + pi-web, switches pass through.
        let body = ServicesBody {
            code_server: false,
            vnc: true,
            pi: true,
            pi_web: true,
        };
        let (env2, svc) = norm(env_with(&[]), Some(&body));
        assert_eq!(env2.scenarios, vec!["pi", "pi-web"]);
        // pi-web does NOT imply code-server (independent switch).
        assert_eq!(
            svc,
            db::Services {
                code_server: false,
                vnc: true
            }
        );
    }

    #[test]
    fn normalize_old_body_without_services_field_defaults_all_on() {
        // The wire contract: an old mgr-web POST (create, no services key)
        // deserializes to all-on - Server default, matching pre-S1. All four
        // switches on means pi/pi-web ALSO join the scenario set (they are
        // surfaced through the services area, never two sources).
        let body: SandboxBody =
            serde_json::from_str(r#"{"name":"old","env":{"scenarios":["node"],"versions":{}}}"#)
                .unwrap();
        let (env2, svc) = norm(body.env, body.services.as_ref());
        assert_eq!(env2.scenarios, vec!["node", "pi", "pi-web"]);
        assert_eq!(
            svc,
            db::Services {
                code_server: true,
                vnc: true
            }
        );
    }

    // ── installed_services_of (S1 row read-back) ──────────────────────

    fn row_with(services_json: Option<String>, env_json: &str) -> db::SandboxRow {
        db::SandboxRow {
            name: "t".into(),
            created_at: 0,
            env_json: env_json.into(),
            env_hash: "h".into(),
            cpus: None,
            mem_mb: None,
            status: "running".into(),
            adopted: false,
            external_compose: None,
            services_json,
        }
    }

    #[test]
    fn installed_services_of_reads_all_on_for_pre_s1_rows() {
        // Pre-S1 rows (services_json NULL) predate the switches: everything
        // was built unconditionally (pi/pi-web were always_on scenarios,
        // never listed in env.scenarios), so pi/pi_web read ON even with an
        // empty scenario set. Deriving them from the empty set would make
        // the first PUT silently strip pi/pi-web from an old sandbox.
        let row = row_with(None, r#"{"scenarios":[],"versions":{"node":"22.23.2"}}"#);
        let svc = installed_services_of(&row);
        assert!(svc.code_server && svc.vnc && svc.pi && svc.pi_web);
    }

    #[test]
    fn installed_services_of_derives_pi_from_scenarios_once_set() {
        // Post-S1 rows: pi/pi_web ARE the scenario set; code_server/vnc come
        // from services_json.
        let row = row_with(
            Some(
                db::Services {
                    code_server: false,
                    vnc: true,
                }
                .canonical_json(),
            ),
            r#"{"scenarios":["node","pi"],"versions":{}}"#,
        );
        let svc = installed_services_of(&row);
        assert!(!svc.code_server && svc.vnc && svc.pi && !svc.pi_web);
    }

    // ── service_start (D4, unified Phase 3) ─────────────────────────

    fn insert_row(state: &AppState, name: &str) {
        let conn = state.db.lock().unwrap();
        db::insert_sandbox(
            &conn,
            &db::SandboxRow {
                name: name.into(),
                created_at: 0,
                env_json: "{}".into(),
                env_hash: String::new(),
                cpus: None,
                mem_mb: None,
                status: "running".into(),
                adopted: false,
                external_compose: None,
                services_json: None,
            },
        )
        .expect("insert test sandbox");
    }

    /// Serve the REAL top-level router on an ephemeral port (the proxy.rs
    /// test pattern - mgr has no tower dev-dependency for oneshot). The
    /// service_start tests only exercise branches that return BEFORE any
    /// docker invocation; the docker paths are locked by the argv-shape
    /// tests in docker.rs and the manual verification list.
    async fn serve(state: &Arc<AppState>) -> String {
        let app = crate::routes::router().with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn service_start_unknown_service_is_404_whitelist() {
        // Closed whitelist: vnc is profile-gated too but NOT on-demand (it
        // is a resident dependency). The ROUTE must answer (not the /api
        // seam - whose message differs).
        let state = Arc::new(AppState::new_for_test());
        insert_row(&state, "alpha");
        let base = serve(&state).await;

        let r = state
            .http
            .post(format!("{base}/api/sandboxes/alpha/service/vnc/start"))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        let v: serde_json::Value = r.json().await.unwrap();
        assert!(v["error"]
            .as_str()
            .unwrap()
            .contains("unknown on-demand service"));
    }

    #[tokio::test]
    async fn service_start_unknown_sandbox_is_404() {
        let state = Arc::new(AppState::new_for_test());
        let base = serve(&state).await;

        let r = state
            .http
            .post(format!(
                "{base}/api/sandboxes/ghost/service/code-server/start"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        let v: serde_json::Value = r.json().await.unwrap();
        assert_eq!(v["error"], "sandbox \"ghost\" not found");
    }

    #[tokio::test]
    async fn service_start_native_without_compose_file_is_400() {
        // The native branch refuses before any docker call when the
        // generated compose is missing (a failed create) - same guard shape
        // as start_sandbox.
        let state = Arc::new(AppState::new_for_test());
        insert_row(&state, "svcstart");
        assert!(
            !state.instance_dir("svcstart").join("compose.yml").exists(),
            "precondition: test data dir holds no compose file"
        );
        let base = serve(&state).await;

        let r = state
            .http
            .post(format!(
                "{base}/api/sandboxes/svcstart/service/code-server/start"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        let v: serde_json::Value = r.json().await.unwrap();
        assert!(v["error"].as_str().unwrap().contains("no compose file"));
    }

    #[test]
    fn ps_live_maps_running_gone_stopped() {
        // Mirrors sandbox_json's live mapping (drives the guard's message).
        let running = vec![entry("a-app-1", "app", "running")];
        assert_eq!(ps_live(&running), "running");
        assert_eq!(ps_live(&[]), "gone");
        let stopped = vec![entry("a-app-1", "app", "exited")];
        assert_eq!(ps_live(&stopped), "stopped");
    }

    #[test]
    fn on_demand_whitelist_is_code_server_only() {
        // The closed whitelist: an arbitrary service name must never
        // resolve to profile flags that reach a compose argv.
        assert_eq!(ON_DEMAND_SERVICES.len(), 1);
        assert_eq!(ON_DEMAND_SERVICES[0].0, "code-server");
        assert_eq!(ON_DEMAND_SERVICES[0].1, docker::CODE_SERVER_PROFILE);
    }

    fn entry(name: &str, service: &str, state: &str) -> docker::ComposePsEntry {
        docker::ComposePsEntry {
            name: name.into(),
            service: service.into(),
            state: state.into(),
            status: String::new(),
        }
    }
}
