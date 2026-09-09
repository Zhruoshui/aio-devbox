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
use axum::routing::{any, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::caddy;
use crate::db;
use crate::docker;
use crate::envhash;
use crate::jobs;
use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/scenarios", get(scenarios))
        .route("/api/sandboxes", get(list_sandboxes).post(create_sandbox))
        // Adopt (Phase 5, design §3.8): register an external compose stack.
        // Static segment, so it wins over the :name routes regardless of
        // registration order (same rule as /api/scenarios).
        .route("/api/sandboxes/adopt", post(adopt_sandbox))
        .route("/api/sandboxes/:name", get(get_sandbox).put(put_sandbox).delete(delete_sandbox))
        .route("/api/sandboxes/:name/start", post(start_sandbox))
        .route("/api/sandboxes/:name/stop", post(stop_sandbox))
        .route("/api/sandboxes/:name/restart", post(restart_sandbox))
        // On-demand single-service start (D4, unified Phase 3): the one
        // profile-gated sidecar `up` deliberately does not carry. Synchronous
        // (container create+start of a pre-built image, seconds), so no job.
        .route("/api/sandboxes/:name/service/:service/start", post(service_start))
        .route("/api/sandboxes/:name/entry_url", get(entry_url))
        .route("/api/images", get(list_images))
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
        ApiError { status: StatusCode::BAD_REQUEST, message: msg.into() }
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
        ApiError { status: StatusCode::INTERNAL_SERVER_ERROR, message: format!("{e:#}") }
    }
}

type ApiResult<T> = Result<T, ApiError>;

// ── scenarios ──────────────────────────────────────────────────────

async fn scenarios(State(state): State<Arc<AppState>>) -> ApiResult<Json<serde_json::Value>> {
    let known = aio_config::scenario::scan(&state.repo.join("scenarios"))
        .map_err(ApiError::from)?;
    let mut list: Vec<serde_json::Value> = known
        .iter()
        .map(|s| {
            json!({
                "id": s.meta.id,
                "name": s.meta.name,
                "description": s.meta.description,
                "category": s.meta.category,
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

#[derive(Deserialize)]
struct SandboxBody {
    name: String,
    #[serde(default)]
    env: envhash::SandboxEnv,
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
        Err(format!("invalid name {name:?}: slug of [a-z0-9-], max 32 chars, must start alnum"))
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
            return Err(ApiError::bad(format!("sandbox {:?} already exists", body.name)));
        }
    }
    // Validate env against the catalog before accepting the job.
    body.env
        .to_manifest_checked(&state.repo)
        .map_err(|e| ApiError::bad(format!("{e:#}")))?;
    check_limits(body.cpus, body.mem_mb)?;
    let (cpus, mem_mb) = (limit_or_none(body.cpus), limit_or_none_mb(body.mem_mb));

    let row = db::SandboxRow {
        name: body.name.clone(),
        created_at: now_secs(),
        env_json: body.env.canonical_json(),
        env_hash: String::new(), // filled by the job on success
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
    let job_id = jobs::spawn_create(state.clone(), body.name.clone(), body.env, cpus, mem_mb, false)
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
            return Err(ApiError::bad(format!("sandbox {:?} already exists", body.name)));
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
    let gateway_service = body.gateway_service.unwrap_or_else(|| DEFAULT_GATEWAY_SERVICE.into());
    let app_service = body.app_service.unwrap_or_else(|| DEFAULT_APP_SERVICE.into());
    let gateway_ct = find_service_container(&ps, &gateway_service).ok_or_else(|| {
        ApiError::bad(format!("service {gateway_service:?} not found in {}", compose_file.display()))
    })?;
    let app_ct = find_service_container(&ps, &app_service).ok_or_else(|| {
        ApiError::bad(format!("service {app_service:?} not found in {}", compose_file.display()))
    })?;

    // Shared network + the two mgr aliases. The alias names are LOAD-BEARING
    // and must mirror caddy.rs render's upstreams exactly (the total gateway
    // dials sbx-<name>:8080 and sbx-<name>-piweb:30141 by DNS name): for
    // mgr-created sandboxes composegen bakes them in as network aliases of
    // the same names - adopting an external stack reproduces that membership
    // by hand because its own compose knows nothing of aio-mgr-net.
    docker::ensure_network("aio-mgr-net").await?;
    docker::network_connect_alias("aio-mgr-net", gateway_ct, &format!("sbx-{}", body.name)).await?;
    docker::network_connect_alias("aio-mgr-net", app_ct, &format!("sbx-{}-piweb", body.name)).await?;

    // Register (BEFORE regenerate: the Caddyfile renders from the table).
    // env stays empty by design - inferring it from the running stack is
    // explicitly out of scope (design §3.8 "不强制").
    let row = db::SandboxRow {
        name: body.name.clone(),
        created_at: now_secs(),
        env_json: "{}".into(),
        env_hash: String::new(),
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
async fn reconnect_adopted_aliases(name: &str, ps: &[docker::ComposePsEntry]) -> Result<(), ApiError> {
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
async fn sandbox_json(state: &Arc<AppState>, row: &db::SandboxRow) -> serde_json::Value {
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
            let live = if ps.is_empty() { "gone" } else if running { "running" } else { "stopped" };
            (live, ps)
        }
        Err(e) => {
            tracing::warn!(sandbox = %row.name, error = %format!("{e:#}"), "compose ps failed");
            ("unknown", Vec::new())
        }
    };
    let short_hash = if row.env_hash.len() >= 12 { &row.env_hash[..12] } else { "" };
    // Adopted rows own no image: they run whatever the external compose
    // pins. The literal keeps the list page's image column meaningful.
    let image = if row.adopted { "external".to_string() } else { format!("sandbox-app-{short_hash}") };
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
        // The sbx- prefix must mirror the total-gateway site blocks exactly
        // (caddy.rs render) - the sandbox-net alias is also sbx-<name>/
        // sbx-<name>-piweb (composegen), so the prefix is the shared identity.
        "entry_url": format!("http://sbx-{}.mgr.localhost/", row.name),
        "piweb_url": format!("http://sbx-{}-piweb.mgr.localhost/", row.name),
        "services": ps.iter().map(|e| json!({
            "service": e.service, "name": e.name, "state": e.state, "status": e.status,
        })).collect::<Vec<_>>(),
    })
}

async fn list_sandboxes(State(state): State<Arc<AppState>>) -> ApiResult<Json<serde_json::Value>> {
    let rows = {
        let conn = state.db.lock().unwrap();
        db::list_sandboxes(&conn)?
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(sandbox_json(&state, row).await);
    }
    Ok(Json(json!({ "sandboxes": out })))
}

async fn get_sandbox(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let row = {
        let conn = state.db.lock().unwrap();
        db::get_sandbox(&conn, &name)?.ok_or_else(|| ApiError::bad(format!("sandbox {name:?} not found")))?
    };
    Ok(Json(sandbox_json(&state, &row).await))
}

#[derive(Deserialize)]
struct PutBody {
    env: Option<envhash::SandboxEnv>,
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
        db::get_sandbox(&conn, &name)?.ok_or_else(|| ApiError::bad(format!("sandbox {name:?} not found")))?
    };
    if row.adopted {
        return Err(ApiError::bad("adopted external stack: env/resource changes unsupported (design §3.8)"));
    }
    check_limits(body.cpus, body.mem_mb)?;
    // Merge: absent fields keep current values; an EXPLICIT 0 CLEARS a
    // resource limit (the UI's "empty field = unlimited" - a plain null
    // would silently keep the old limit, which is not what a cleared input
    // means). A new env hash drives the recreate flow (design §3.6 PUT).
    let env = body
        .env
        .unwrap_or_else(|| serde_json::from_str(&row.env_json).expect("env_json roundtrips"));
    let cpus = match body.cpus {
        Some(c) if c > 0.0 => Some(c),
        Some(_) => None, // explicit 0 = clear
        None => row.cpus, // absent = keep
    };
    let mem_mb = match body.mem_mb {
        Some(m) if m > 0 => Some(m),
        Some(_) => None,
        None => row.mem_mb,
    };
    env.to_manifest_checked(&state.repo).map_err(|e| ApiError::bad(format!("{e:#}")))?;

    db::update_sandbox_status(&state.db.lock().unwrap(), &name, "creating")?;
    let job_id = jobs::spawn_create(state.clone(), name.clone(), env, cpus, mem_mb, true).await?;
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
    Ok(Json(json!({ "job": job_id, "name": name, "volumes": volumes })))
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
    }
    if let Some(file) = row.external_compose.as_deref().map(PathBuf::from).filter(|p| p.exists()) {
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
            return Err(ApiError::bad(format!("sandbox {name:?} has no compose file")));
        }
        docker::ensure_network("aio-mgr-net").await?;
        docker::compose_up(&envhash::project_name(&name), &compose_file, false).await?
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
    ApiError::bad(format!("sandbox {name:?} is {live} - start the sandbox before starting {service:?}"))
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
            return Err(ApiError::bad(format!("sandbox {name:?} has no compose file")));
        }
        let project = envhash::project_name(&name);
        let live = ps_live(&docker::compose_ps(&project, &compose_file).await?);
        if live != "running" {
            return Err(not_running(&name, &service, live));
        }
        docker::compose_service_up(&project, &compose_file, profile, &service).await?
    };
    Ok(Json(json!({ "ok": true, "service": service, "output": out.trim() })))
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
    Ok(Json(json!({
        "entry": format!("http://sbx-{name}.mgr.localhost/"),
        "piweb": format!("http://sbx-{name}-piweb.mgr.localhost/"),
    })))
}

// ── images / jobs ──────────────────────────────────────────────────

async fn list_images(State(state): State<Arc<AppState>>) -> ApiResult<Json<serde_json::Value>> {
    let rows = {
        let conn = state.db.lock().unwrap();
        db::list_images(&conn)?
    };
    let mut out = Vec::with_capacity(rows.len());
    for (env_hash, tag, built_at, build_log) in rows {
        let refcount = {
            let conn = state.db.lock().unwrap();
            db::image_refcount(&conn, &env_hash)?
        };
        out.push(json!({
            "env_hash": env_hash,
            "tag": tag,
            "built_at": built_at,
            "refcount": refcount,
            "build_log": build_log,
        }));
    }
    Ok(Json(json!({ "images": out })))
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
        let b: AdoptBody = serde_json::from_str(
            r#"{"name":"legacy","compose_path":"docker-compose.yml"}"#,
        )
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
        assert!(v["error"].as_str().unwrap().contains("unknown on-demand service"));
    }

    #[tokio::test]
    async fn service_start_unknown_sandbox_is_404() {
        let state = Arc::new(AppState::new_for_test());
        let base = serve(&state).await;

        let r = state
            .http
            .post(format!("{base}/api/sandboxes/ghost/service/code-server/start"))
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
            .post(format!("{base}/api/sandboxes/svcstart/service/code-server/start"))
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
