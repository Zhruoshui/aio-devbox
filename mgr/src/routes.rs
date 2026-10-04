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
        // R3: the FULL build log of an image, read from the create job's
        // on-disk log (falling back to the images.build_log column tail).
        .route("/api/images/:env_hash/log", get(image_log))
        .route("/api/jobs/:id", get(get_job))
        // R3: paged access to a job's full on-disk log
        // (?offset=<byte>&limit=<byte>&tail=<byte>).
        .route("/api/jobs/:id/log", get(job_log))
        // R2 + 修正 2/3: the user-tools fragment (the one repo subtree mgr
        // WRITES - the WebUI editor). Static path, so it wins over the /api
        // seam regardless of order.
        .route(
            "/api/user-tools",
            get(get_user_tools).put(put_user_tools),
        )
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
    // 修正 3 (R4): custom tools are NOT scenario-catalog entries (they live
    // under usertools/<id>/ with metadata in mgr-data) - but the create
    // wizard's EnvPicker needs them as L5 (category "service") rows. Synthesize
    // the same wire shape from picker_tools so the frontend needs zero
    // special-casing. id-sorted within the layer by the sort below (stable).
    for t in crate::usertools::picker_tools(&state.repo) {
        list.push(json!({
            "id": t.id,
            "name": t.name,
            "description": "用户自定义工具（usertools/<id>/fragment.Dockerfile）",
            "category": "service",
            "installer": "custom",
            "always_on": false,
            "default_version": null,
            "versions": Vec::<String>::new(),
        }));
    }
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
        // 修正 3: custom tools are not service switches - they pass through untouched.
        user_tools: env.user_tools,
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
/// as "—" when it fails / the image is gone, never an error here). R3 adds
/// `env` (the build-time SandboxEnv) and `services` (the build-time
/// code-server/vnc switches) as structured combo-detail sources; both are
/// NULL on pre-R3 rows (never break on the unknown: unparseable values fall
/// back to NULL the same way).
async fn list_images(State(state): State<Arc<AppState>>) -> ApiResult<Json<serde_json::Value>> {
    let rows = {
        let conn = state.db.lock().unwrap();
        db::list_images(&conn)?
    };
    let mut out = Vec::with_capacity(rows.len());
    for (env_hash, tag, built_at, build_log, combo, env_json, services_json, job_id) in rows {
        let refcount = {
            let conn = state.db.lock().unwrap();
            db::image_refcount(&conn, &env_hash)?
        };
        // Live size of the base image (the row's tag): best-effort, "—" on
        // failure (image deleted behind our back, docker down).
        let size_bytes = docker::image_size(&tag).await.ok();
        let env = env_json
            .as_deref()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
        let services = services_json
            .as_deref()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
        out.push(json!({
            "env_hash": env_hash,
            "tag": tag,
            "built_at": built_at,
            "refcount": refcount,
            "build_log": build_log,
            "combo": combo,
            "env": env,
            "services": services,
            "size_bytes": size_bytes,
            // R3: the build job whose FULL log is on disk (GET
            // /api/images/:env_hash/log). null on pre-R3 rows.
            "job_id": job_id,
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
            .any(|row| row.0 == h.as_str());
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

// ── R3 full build logs ─────────────────────────────────────────────

/// Default page size of the log endpoints (256KB); the frontend asks for
/// more when prefetching a whole log.
const LOG_PAGE_DEFAULT: u64 = 256 * 1024;

/// Hard per-request window cap. Logs are read by paging (the frontend walks
/// `next_offset`); without a ceiling a single `limit` could pin a huge
/// response body in memory for one request.
const LOG_PAGE_MAX: u64 = 4 * 1024 * 1024;

/// The effective window of a log request: defaulted + clamped.
fn page_limit(q: &LogQuery) -> u64 {
    q.limit.unwrap_or(LOG_PAGE_DEFAULT).clamp(1, LOG_PAGE_MAX)
}

/// Query of GET /api/jobs/:id/log and /api/images/:env_hash/log. All fields
/// optional: `offset`/`limit` page through the log, `tail=N` asks for the
/// last N bytes instead (the "just show me the end" case).
#[derive(Debug, Deserialize)]
struct LogQuery {
    offset: Option<u64>,
    limit: Option<u64>,
    tail: Option<u64>,
}

/// What a log read produced: the file's total byte size plus the requested
/// window. `data` is byte-offset-addressed, so `next_offset` (not the string
/// length) is what a pager advances by - see jobs::log_page.
struct LogChunk {
    data: String,
    total: u64,
    next_offset: u64,
}

/// Read a window out of a job's FULL on-disk log (`<data>/jobs/<id>.log`).
/// `None` when no file is known / readable - the caller falls back to the
/// in-memory tail (a running short job) or the build_log column.
///
/// The read runs on the blocking pool: a 4MB+ log read on the async reactor
/// would stall every other request for the duration of the IO.
async fn read_job_log_file(path: &str, q: &LogQuery) -> Option<LogChunk> {
    let path = PathBuf::from(path);
    let offset = q.offset;
    let limit = page_limit(q);
    let tail_n = q.tail;
    match tokio::task::spawn_blocking(move || -> std::io::Result<LogChunk> {
        let bytes = std::fs::read(&path)?;
        // The byte window is decoded lossily and re-walked on char
        // boundaries: the file is build output (valid UTF-8 in practice) but
        // a window can land mid-char, and a split char must never 500 a log
        // view. The offset SPACE is the decoded string's, so `total` is its
        // length, not the file's: a lossy substitution of invalid bytes makes
        // the two differ, and a pager that mixed them would stop short of the
        // real end. (Identical for well-formed output.)
        let s = String::from_utf8_lossy(&bytes).into_owned();
        let total = s.len() as u64;
        // A `tail` request is a window at the END: resolve it to an offset up
        // front so the rest of the logic is one path.
        let start = match tail_n {
            Some(n) => total.saturating_sub(n),
            None => offset.unwrap_or(0),
        };
        let (data, _, next) = crate::jobs::log_page(&s, start, limit);
        Ok(LogChunk {
            data,
            total,
            next_offset: next,
        })
    })
    .await
    {
        Ok(Ok(chunk)) => Some(chunk),
        // Missing file (a job that never opened one) is the CALLER's
        // fallback signal, not an error; a genuine IO failure is warned and
        // treated the same way so the page still shows the column tail.
        Ok(Err(e)) => {
            tracing::debug!(error = %e, "job log file unreadable, falling back");
            None
        }
        Err(e) => {
            tracing::warn!(error = %e, "job log read task failed");
            None
        }
    }
}

/// Build the shared log JSON body (both log endpoints).
fn log_json(
    data: &str,
    total: u64,
    next_offset: u64,
    log_path: Option<&str>,
    complete: bool,
) -> serde_json::Value {
    json!({
        "data": data,
        "total_size": total,
        "next_offset": next_offset,
        "log_path": log_path,
        // false = the caller is looking at the capped memory tail / column
        // fallback rather than the full on-disk log (the UI labels it "tail").
        "complete": complete,
    })
}

/// GET /api/jobs/:id/log?offset=&limit=&tail= - R3: the full build log of a
/// create/recreate job, read from disk so nothing is truncated at the 64KB
/// memory tail. Falls back to that tail when the job has no on-disk log
/// (a short lifecycle job) or the file is gone. Unknown job -> 404-shaped 400
/// (the same ApiError::bad shape get_job uses).
async fn job_log(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Query(q): Query<LogQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    // The on-disk path is the durable source; the in-memory cell is the
    // fallback (and the only source for a job that never opened a file).
    let cell = {
        let jobs = state.jobs.lock().unwrap();
        jobs.get(&id).cloned()
    };
    let memory: Option<(String, Option<String>)> = match &cell {
        Some(c) => {
            let j = c.lock().await;
            Some((j.log.clone(), j.log_path.clone()))
        }
        None => None,
    };
    let path = match &memory {
        Some((_, Some(p))) => Some(p.clone()),
        // Legacy/in-memory job: the jobs row may still know the path (set
        // before the build, survives an mgr restart).
        _ => db::job_log_path(&state.db.lock().unwrap(), id)?,
    };
    if let Some(path) = &path {
        if let Some(chunk) = read_job_log_file(path, &q).await {
            return Ok(Json(log_json(
                &chunk.data,
                chunk.total,
                chunk.next_offset,
                Some(path),
                true,
            )));
        }
    }
    match memory {
        Some((log, _)) => {
            // Honour `tail` here too: the memory tail is one fixed buffer, so
            // a tail request is the same slice as for a file.
            let total = log.len() as u64;
            let start = match q.tail {
                Some(n) => total.saturating_sub(n),
                None => q.offset.unwrap_or(0),
            };
            let (data, _, next) = crate::jobs::log_page(&log, start, page_limit(&q));
            Ok(Json(log_json(&data, total, next, None, false)))
        }
        None => Err(ApiError::bad(format!("job {id} not found"))),
    }
}

/// GET /api/images/:env_hash/log?offset=&limit=&tail= - R3: the FULL build
/// log behind an image. Sources in order: the create job's on-disk log
/// (images.job_id -> jobs.log_path), then the images.build_log column tail
/// (pre-R3 rows / pruned files). Paging reads the whole string and windows
/// it, so image logs and job logs share one contract.
async fn image_log(
    State(state): State<Arc<AppState>>,
    Path(h): Path<String>,
    Query(q): Query<LogQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    if !valid_env_hash(&h) {
        return Err(ApiError::bad("env_hash must be 64 hex chars"));
    }
    let row = {
        let conn = state.db.lock().unwrap();
        db::list_images(&conn)?
            .into_iter()
            .find(|r| r.0 == h)
            .map(|r| (r.3, r.7))
    };
    let (build_log, job_id) = match row {
        Some(v) => v,
        None => return Err(ApiError::bad(format!("image {h} not found"))),
    };
    // Prefer the full on-disk log via the recorded job.
    if let Some(job_id) = job_id {
        let path = db::job_log_path(&state.db.lock().unwrap(), job_id)?;
        if let Some(path) = &path {
            if let Some(chunk) = read_job_log_file(path, &q).await {
                return Ok(Json(log_json(
                    &chunk.data,
                    chunk.total,
                    chunk.next_offset,
                    Some(path),
                    true,
                )));
            }
        }
    }
    // Fallback: the column tail (capped, no log_path).
    let total = build_log.len() as u64;
    let start = match q.tail {
        Some(n) => total.saturating_sub(n),
        None => q.offset.unwrap_or(0),
    };
    let (data, _, next) = crate::jobs::log_page(&build_log, start, page_limit(&q));
    Ok(Json(log_json(&data, total, next, None, false)))
}

// ── R2 user-tools fragment ─────────────────────────────────────────
//
// mgr is otherwise read-only against `state.repo` (contract 1: the repo is
// mounted read-only in the containerized form). These two handlers are the
// deliberate exception: the user-tools fragment is the user's own L2 escape
// hatch, and the deployed mgr has no code-server-only workaround. The
// container MUST therefore mount the repo read-write for this route to work;
// a read-only mount surfaces as a 500 with the OS error in the anyhow chain
// (the honest failure - silently accepting the write is worse).

/// GET /api/user-tools - the current fragment + the tool list it encodes +
/// the picker tool list. `tools` (from the fragment's `# --- tool:` blocks)
/// drives the editor's "contains N tools" label; `picker` is the L5 tool list
/// (meta ∩ on-disk fragment) offered by the create wizard.
async fn get_user_tools(State(state): State<Arc<AppState>>) -> ApiResult<Json<serde_json::Value>> {
    let path = crate::usertools::fragment_path(&state.repo);
    let content = std::fs::read_to_string(&path)
        // A missing fragment is NOT an error: the editor starts empty and the
        // first save creates the file.
        .unwrap_or_else(|_| String::new());
    let tools: Vec<crate::usertools::UserTool> = crate::usertools::list_tools(&content)
        .into_iter()
        .map(|name| crate::usertools::UserTool {
            id: crate::usertools::slug(&name).unwrap_or_else(|| name.to_ascii_lowercase()),
            name,
        })
        .collect();
    let picker = crate::usertools::picker_tools(&state.repo);
    Ok(Json(json!({
        "content": content,
        "tools": tools,
        "picker": picker,
        "path": path.display().to_string(),
    })))
}

#[derive(Deserialize)]
struct UserToolsContentBody {
    content: String,
}

/// PUT /api/user-tools - direct-editor save. Static validation (the `/root`
/// hard rule + marker-block well-formedness), then the content is stored
/// verbatim (hand-written comments / blank lines round-trip exactly) and the
/// per-tool fragment dirs + metadata are re-synced so the L5 picker sees the
/// new tool set immediately.
async fn put_user_tools(
    State(state): State<Arc<AppState>>,
    Json(body): Json<UserToolsContentBody>,
) -> ApiResult<Json<serde_json::Value>> {
    if let Err(msg) = crate::usertools::validate(&body.content) {
        return Err(ApiError::bad(msg));
    }
    if let Err(msg) = crate::usertools::blocks_well_formed(&body.content) {
        return Err(ApiError::bad(msg));
    }
    let tools = write_fragment_and_sync(&state, &body.content)?;
    Ok(Json(json!({ "ok": true, "tools": tools })))
}

/// Atomic write of the fragment + re-sync of the per-tool fragment dirs and
/// the tool metadata. The repo-write failure is made explicit (the container's
/// read-only repo mount is the expected cause).
fn write_fragment_and_sync(
    state: &Arc<AppState>,
    content: &str,
) -> Result<Vec<crate::usertools::UserTool>, ApiError> {
    let path = crate::usertools::fragment_path(&state.repo);
    crate::usertools::write_atomic(&path, content).map_err(|e| {
        ApiError::from(anyhow::anyhow!(
            "write {} failed ({e:#}) - in the containerized form the repo must be mounted read-write for this route",
            path.display()
        ))
    })?;
    let tools = crate::usertools::sync_scenario_dirs(&state.repo, content).map_err(|e| {
        ApiError::from(anyhow::anyhow!("sync usertools dirs: {e:#}"))
    })?;
    let meta = crate::usertools::ToolMeta { tools: tools.clone() };
    crate::usertools::save_meta(&state.data, &meta)
        .map_err(|e| ApiError::from(anyhow::anyhow!("write usertools meta: {e:#}")))?;
    Ok(tools)
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
            user_tools: vec![],
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

    // ── R3 full-log endpoints ───────────────────────────────────────

    /// A state whose data root is a fresh temp dir (the log endpoints read
    /// `<data>/jobs/<id>.log`; new_for_test's shared /tmp would let parallel
    /// tests collide on the same path).
    fn log_state(tag: &str) -> Arc<AppState> {
        let mut state = AppState::new_for_test();
        let dir = std::env::temp_dir().join(format!("mgr-r3-log-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp data dir");
        state.data = dir;
        Arc::new(state)
    }

    /// A 64-hex env_hash (valid_env_hash's alphabet).
    fn hex_hash(byte: u8) -> String {
        format!("{byte:02x}").repeat(32)
    }

    /// Write `<data>/jobs/<id>.log`, exactly as run_create does.
    fn write_job_log(state: &AppState, job_id: i64, body: &str) -> String {
        let dir = state.data.join("jobs");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{job_id}.log"));
        std::fs::write(&path, body).unwrap();
        path.display().to_string()
    }

    #[tokio::test]
    async fn job_log_serves_the_full_file_beyond_the_memory_tail() {
        // AC6/AC8: the file holds the WHOLE build output; the endpoint pages
        // it by byte offset instead of the 64KB memory tail.
        let state = log_state("job-full");
        let body = format!("{}\nBUILD FAILED HERE\n", "line\n".repeat(20_000));
        assert!(body.len() > 64 * 1024, "fixture exceeds LOG_TAIL");
        let job_id = {
            let conn = state.db.lock().unwrap();
            db::insert_job(&conn, "create", Some("sbx-a")).unwrap()
        };
        let path = write_job_log(&state, job_id, &body);
        db::set_job_log_path(&state.db.lock().unwrap(), job_id, &path).unwrap();
        let base = serve(&state).await;

        let r = state
            .http
            .get(format!("{base}/api/jobs/{job_id}/log?limit=1000000"))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
        let v: serde_json::Value = r.json().await.unwrap();
        assert_eq!(v["data"].as_str().unwrap(), body, "nothing is truncated");
        assert_eq!(v["total_size"].as_u64().unwrap(), body.len() as u64);
        assert_eq!(v["next_offset"].as_u64().unwrap(), body.len() as u64);
        assert_eq!(v["complete"], true);
        assert_eq!(v["log_path"].as_str().unwrap(), path);

        // `tail=N` is the last-N-bytes convenience.
        let r = state
            .http
            .get(format!("{base}/api/jobs/{job_id}/log?tail=18"))
            .send()
            .await
            .unwrap();
        let v: serde_json::Value = r.json().await.unwrap();
        assert_eq!(v["data"].as_str().unwrap(), "BUILD FAILED HERE\n");

        // Unknown job: the same 400-shaped not-found as GET /api/jobs/:id.
        let r = state
            .http
            .get(format!("{base}/api/jobs/987654/log"))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn job_log_pages_across_multibyte_boundaries() {
        // AC8 + AC9 over HTTP: a log well past the old 256KB column cap, paged
        // in odd-sized windows whose edges land inside CJK / emoji chars, must
        // reconstruct byte-for-byte, never panic, and never stall.
        let state = log_state("job-page");
        let body = "中文🎉ok\n".repeat(40_000); // ~400KB, 4 bytes/char groups
        let job_id = {
            let conn = state.db.lock().unwrap();
            db::insert_job(&conn, "create", Some("sbx-a")).unwrap()
        };
        let path = write_job_log(&state, job_id, &body);
        db::set_job_log_path(&state.db.lock().unwrap(), job_id, &path).unwrap();
        let base = serve(&state).await;

        let mut out = String::new();
        let mut off = 0u64;
        loop {
            let r = state
                .http
                .get(format!("{base}/api/jobs/{job_id}/log?offset={off}&limit=1021"))
                .send()
                .await
                .unwrap();
            assert!(r.status().is_success(), "mid-char offset must not 500");
            let v: serde_json::Value = r.json().await.unwrap();
            out.push_str(v["data"].as_str().unwrap());
            let total = v["total_size"].as_u64().unwrap();
            let next = v["next_offset"].as_u64().unwrap();
            assert!(next > off || next >= total, "page must make progress");
            if next >= total {
                break;
            }
            off = next;
        }
        assert_eq!(out, body, "pages concatenate into the whole log");
        assert!(body.len() > 256 * 1024, "fixture exceeds the old column cap");

        // The hand-made request that used to panic the handler: an offset that
        // splits a char with a tiny limit.
        let r = state
            .http
            .get(format!("{base}/api/jobs/{job_id}/log?offset=1&limit=1"))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
        let v: serde_json::Value = r.json().await.unwrap();
        assert_eq!(v["data"].as_str().unwrap(), "文");
    }

    #[tokio::test]
    async fn job_log_falls_back_to_the_memory_tail() {
        // A short lifecycle job (delete / image-*) opens no file: the endpoint
        // still serves its in-memory log, flagged complete=false so the UI can
        // label it a tail.
        let state = log_state("job-mem");
        let job_id = {
            let conn = state.db.lock().unwrap();
            db::insert_job(&conn, "delete", Some("sbx-a")).unwrap()
        };
        let cell = Arc::new(tokio::sync::Mutex::new(crate::state::JobShared {
            id: job_id,
            kind: "delete".into(),
            sandbox: Some("sbx-a".into()),
            status: "running".into(),
            error: None,
            log: "compose down output\n".into(),
            log_path: None,
        }));
        state.jobs.lock().unwrap().insert(job_id, cell);
        let base = serve(&state).await;

        let r = state
            .http
            .get(format!("{base}/api/jobs/{job_id}/log"))
            .send()
            .await
            .unwrap();
        let v: serde_json::Value = r.json().await.unwrap();
        assert_eq!(v["data"].as_str().unwrap(), "compose down output\n");
        assert_eq!(v["complete"], false);
        assert!(v["log_path"].is_null());
    }

    #[tokio::test]
    async fn image_log_prefers_the_job_file_and_falls_back_to_the_column() {
        // AC8: the images page loads the FULL build output of the job behind
        // the row; a pre-R3 row (no job_id) still serves its column tail.
        let state = log_state("img-log");
        // > 256KB (the pre-R3 build_log cap) on purpose: AC8 is precisely
        // "the images page can load more than the old column kept".
        let big = format!("{}\nboom\n", "step\n".repeat(60_000));
        let job_id = {
            let conn = state.db.lock().unwrap();
            db::insert_job(&conn, "create", Some("sbx-a")).unwrap()
        };
        let path = write_job_log(&state, job_id, &big);
        {
            let conn = state.db.lock().unwrap();
            db::set_job_log_path(&conn, job_id, &path).unwrap();
            db::upsert_image(
                &conn,
                &hex_hash(0xaa),
                "sandbox-base-aa",
                "column tail",
                None,
                "",
                "",
                Some(job_id),
            )
            .unwrap();
            db::upsert_image(
                &conn,
                &hex_hash(0xbb),
                "sandbox-base-bb",
                "legacy column tail",
                None,
                "",
                "",
                None,
            )
            .unwrap();
        }
        let base = serve(&state).await;

        let r = state
            .http
            .get(format!("{base}/api/images/{}/log?limit=1000000", hex_hash(0xaa)))
            .send()
            .await
            .unwrap();
        let v: serde_json::Value = r.json().await.unwrap();
        assert_eq!(v["data"].as_str().unwrap(), big, "full file, not the column");
        assert_eq!(v["complete"], true);
        assert_eq!(v["total_size"].as_u64().unwrap(), big.len() as u64);

        // No job_id (pre-R3 row): the build_log column is the source.
        let r = state
            .http
            .get(format!("{base}/api/images/{}/log", hex_hash(0xbb)))
            .send()
            .await
            .unwrap();
        let v: serde_json::Value = r.json().await.unwrap();
        assert_eq!(v["data"].as_str().unwrap(), "legacy column tail");
        assert_eq!(v["complete"], false);

        // Malformed / unknown env_hash stay in the ApiError shape.
        let r = state
            .http
            .get(format!("{base}/api/images/nothex/log"))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        let r = state
            .http
            .get(format!("{base}/api/images/{}/log", hex_hash(0xcc)))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn images_payload_carries_job_id() {
        // The frontend mirror (types.ts Image.job_id) drives the "is there a
        // full log to load?" decision - the field must survive the row
        // destructuring in list_images.
        let state = log_state("img-payload");
        {
            let conn = state.db.lock().unwrap();
            db::upsert_image(
                &conn,
                &hex_hash(0x11),
                "sandbox-base-11",
                "log",
                None,
                "",
                "",
                Some(42),
            )
            .unwrap();
            db::upsert_image(&conn, &hex_hash(0x22), "sandbox-base-22", "log", None, "", "", None)
                .unwrap();
        }
        let base = serve(&state).await;
        let r = state
            .http
            .get(format!("{base}/api/images"))
            .send()
            .await
            .unwrap();
        let v: serde_json::Value = r.json().await.unwrap();
        let rows = v["images"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        let with_job = rows
            .iter()
            .find(|r| r["env_hash"] == hex_hash(0x11).as_str())
            .unwrap();
        assert_eq!(with_job["job_id"], 42);
        let without = rows
            .iter()
            .find(|r| r["env_hash"] == hex_hash(0x22).as_str())
            .unwrap();
        assert!(without["job_id"].is_null(), "pre-R3 row: no job link");
    }

    // ── R2 + 修正 2/3 user-tools endpoints ──────────────────────────

    /// A temp repo + data dir; the repo's `usertools/` holds `body` (or nothing).
    fn usertools_state(tag: &str, body: Option<&str>) -> Arc<AppState> {
        let dir = std::env::temp_dir().join(format!("mgr-usertools-rt-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let repo = dir.join("repo");
        let data = dir.join("data");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        let mut state = AppState::new_for_test();
        state.repo = repo;
        state.data = data;
        if let Some(b) = body {
            let path = crate::usertools::fragment_path(&state.repo);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b).unwrap();
        }
        Arc::new(state)
    }

    fn fragment_of(state: &AppState) -> String {
        std::fs::read_to_string(crate::usertools::fragment_path(&state.repo)).unwrap()
    }

    #[tokio::test]
    async fn user_tools_get_returns_content_tools_and_picker() {
        let content = "# --- tool: RipGrep ---\nRUN apt-get install -y ripgrep\n# --- /tool ---\n";
        let state = usertools_state("get", Some(content));
        let base = serve(&state).await;

        let r = state
            .http
            .get(format!("{base}/api/user-tools"))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
        let v: serde_json::Value = r.json().await.unwrap();
        assert_eq!(v["content"].as_str().unwrap(), content);
        assert_eq!(v["tools"].as_array().unwrap().len(), 1);
        assert_eq!(v["tools"][0]["name"], "RipGrep");
        assert_eq!(v["tools"][0]["id"], "ripgrep");
        // No save has run yet, so the picker (meta ∩ on-disk fragment) is empty.
        assert_eq!(v["picker"].as_array().unwrap().len(), 0);
        assert!(v["path"].as_str().unwrap().ends_with("usertools/fragment.Dockerfile"));
    }

    #[tokio::test]
    async fn user_tools_get_missing_file_is_empty_not_an_error() {
        let state = usertools_state("missing", None);
        let base = serve(&state).await;
        let r = state
            .http
            .get(format!("{base}/api/user-tools"))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
        let v: serde_json::Value = r.json().await.unwrap();
        assert_eq!(v["content"], "");
        assert_eq!(v["tools"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn user_tools_put_stores_content_verbatim() {
        // Hand-written comments / blank lines survive a save (no reformatting).
        let state = usertools_state("raw-save", Some("old\n"));
        let base = serve(&state).await;
        let hand = "# my note\n\nRUN install -m 0755 /tmp/x /usr/local/bin/x\n\n# tail note\n";
        let r = state
            .http
            .put(format!("{base}/api/user-tools"))
            .json(&serde_json::json!({ "content": hand }))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
        assert_eq!(fragment_of(&state), hand);
    }

    #[tokio::test]
    async fn user_tools_put_syncs_tool_dirs_and_meta() {
        // 修正 3 (AC7): a save makes the new tool set visible to the picker
        // immediately (per-tool dirs + meta, no mgr restart).
        let state = usertools_state("sync", Some("old\n"));
        let base = serve(&state).await;
        let content = "# --- tool: RipGrep ---\nRUN apt-get install -y ripgrep\n# --- /tool ---\n\
                       # --- tool: fd ---\nRUN curl -fsSL https://example.com/fd.tgz | tar -xz -C /opt\n# --- /tool ---\n";
        let r = state
            .http
            .put(format!("{base}/api/user-tools"))
            .json(&serde_json::json!({ "content": content }))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
        let v: serde_json::Value = r.json().await.unwrap();
        assert_eq!(v["tools"].as_array().unwrap().len(), 2);
        assert!(crate::usertools::tool_dir(&state.repo, "ripgrep")
            .join("fragment.Dockerfile")
            .is_file());
        assert!(crate::usertools::tool_dir(&state.repo, "fd")
            .join("fragment.Dockerfile")
            .is_file());
        assert_eq!(crate::usertools::load_meta(&state.data).tools.len(), 2);

        // Re-save with one tool removed: its dir is pruned, the picker shrinks.
        let trimmed = "# --- tool: RipGrep ---\nRUN apt-get install -y ripgrep\n# --- /tool ---\n";
        let r = state
            .http
            .put(format!("{base}/api/user-tools"))
            .json(&serde_json::json!({ "content": trimmed }))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
        assert!(!crate::usertools::tool_dir(&state.repo, "fd").exists());
        let picker = crate::usertools::picker_tools(&state.repo);
        assert_eq!(picker.len(), 1);
        assert_eq!(picker[0].id, "ripgrep");
    }

    #[tokio::test]
    async fn user_tools_put_rejects_root_writes() {
        let state = usertools_state("raw-root", Some("old\n"));
        let base = serve(&state).await;
        for bad in [
            "WORKDIR /root\n",
            "RUN mkdir -p /root/bin\n",
            "RUN set -eux; \\\n    install -d /root/bin\n",
            "RUN echo a \\\n/root\n",
        ] {
            let r = state
                .http
                .put(format!("{base}/api/user-tools"))
                .json(&serde_json::json!({ "content": bad }))
                .send()
                .await
                .unwrap();
            assert_eq!(r.status(), StatusCode::BAD_REQUEST, "{bad}");
            let v: serde_json::Value = r.json().await.unwrap();
            assert!(v["error"].as_str().unwrap().contains("/root"));
        }
        assert_eq!(fragment_of(&state), "old\n", "rejected saves never touch disk");
    }

    #[tokio::test]
    async fn user_tools_put_rejects_malformed_marker_blocks() {
        // A save must not produce per-tool fragments from an unterminated /
        // nested marker block (the picker would be lying about what builds).
        let state = usertools_state("malformed", Some("old\n"));
        let base = serve(&state).await;
        for bad in [
            "# --- tool: x ---\nRUN true\n",                       // unterminated
            "# --- /tool ---\n",                                   // close without open
            "# --- tool: x ---\n# --- tool: y ---\n# --- /tool ---\n# --- /tool ---\n", // nested
        ] {
            let r = state
                .http
                .put(format!("{base}/api/user-tools"))
                .json(&serde_json::json!({ "content": bad }))
                .send()
                .await
                .unwrap();
            assert_eq!(r.status(), StatusCode::BAD_REQUEST, "{bad}");
        }
        assert_eq!(fragment_of(&state), "old\n", "a rejected save never touches disk");
    }

    #[tokio::test]
    async fn user_tools_routes_beat_the_api_seam() {
        // Static segments, so the /api/*rest 404 seam must not swallow them.
        let state = usertools_state("seam", Some(""));
        let base = serve(&state).await;
        let r = state.http.get(format!("{base}/api/user-tools")).send().await.unwrap();
        let v: serde_json::Value = r.json().await.unwrap();
        assert_ne!(v["error"], "no such API route", "GET fell through");
    }

    #[tokio::test]
    async fn log_routes_beat_the_api_seam() {
        // Both routes are static-suffixed (:env_hash/log, :id/log), so they
        // must be matched by the router - never by the /api/*rest 404 seam.
        let state = log_state("seam");
        let base = serve(&state).await;
        for path in ["/api/jobs/1/log", "/api/images/aa/log"] {
            let r = state
                .http
                .get(format!("{base}{path}"))
                .send()
                .await
                .unwrap();
            let v: serde_json::Value = r.json().await.unwrap();
            assert_ne!(
                v["error"], "no such API route",
                "{path} fell through to the seam"
            );
        }
    }
}
