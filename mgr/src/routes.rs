// HTTP API surface (sandbox-mgr Phase 1, design §3.4).
//
// Shape mirrors the sandbox app's /api style: JSON in/out, axum State,
// handlers thin over the db/docker modules. Long operations (create,
// recreate, delete) return 202 + {job} immediately; the client polls
// GET /api/jobs/:id (no SSE - personal-scale polling is enough, design §3.4).

use std::sync::Arc;

use anyhow::Result;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::db;
use crate::docker;
use crate::envhash;
use crate::jobs;
use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/scenarios", get(scenarios))
        .route("/api/sandboxes", get(list_sandboxes).post(create_sandbox))
        .route("/api/sandboxes/:name", get(get_sandbox).put(put_sandbox).delete(delete_sandbox))
        .route("/api/sandboxes/:name/start", post(start_sandbox))
        .route("/api/sandboxes/:name/stop", post(stop_sandbox))
        .route("/api/sandboxes/:name/restart", post(restart_sandbox))
        .route("/api/sandboxes/:name/entry_url", get(entry_url))
        .route("/api/images", get(list_images))
        .route("/api/jobs/:id", get(get_job))
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

fn validate_name(name: &str) -> Result<(), String> {
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

    let row = db::SandboxRow {
        name: body.name.clone(),
        created_at: now_secs(),
        env_json: body.env.canonical_json(),
        env_hash: String::new(), // filled by the job on success
        cpus: body.cpus,
        mem_mb: body.mem_mb,
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
        body.env,
        body.cpus,
        body.mem_mb,
        false,
    )
    .await?;
    Ok(Json(json!({ "job": job_id, "name": body.name })))
}

/// Live compose state merged onto a DB row. The DB status is the intent
/// (creating/running/error); `live` reports what compose actually says:
/// "running" / "stopped" / "gone" (no containers) / "unknown" (compose ps
/// itself failed - docker down, stale compose file: shown, never hidden).
async fn sandbox_json(state: &Arc<AppState>, row: &db::SandboxRow) -> serde_json::Value {
    let compose_file = state.instance_dir(&row.name).join("compose.yml");
    let ps = match docker::compose_ps(&envhash::project_name(&row.name), &compose_file).await {
        Ok(ps) => ps,
        Err(e) => {
            tracing::warn!(sandbox = %row.name, error = %format!("{e:#}"), "compose ps failed");
            Vec::new()
        }
    };
    let running = ps.iter().any(|e| e.state.eq_ignore_ascii_case("running"));
    let short_hash = if row.env_hash.len() >= 12 { &row.env_hash[..12] } else { "" };
    json!({
        "name": row.name,
        "status": row.status,
        "live": if ps.is_empty() { "gone" } else if running { "running" } else { "stopped" },
        "adopted": row.adopted,
        "created_at": row.created_at,
        "cpus": row.cpus,
        "mem_mb": row.mem_mb,
        "env": serde_json::from_str::<serde_json::Value>(&row.env_json).unwrap_or(json!({})),
        "image": format!("sandbox-app-{short_hash}"),
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
    // Merge: absent fields keep current values; a new env hash drives the
    // recreate flow (design §3.6 PUT).
    let env = body
        .env
        .unwrap_or_else(|| serde_json::from_str(&row.env_json).expect("env_json roundtrips"));
    let cpus = body.cpus.or(row.cpus);
    let mem_mb = body.mem_mb.or(row.mem_mb);
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
    {
        let conn = state.db.lock().unwrap();
        if db::get_sandbox(&conn, &name)?.is_none() {
            return Err(ApiError::bad(format!("sandbox {name:?} not found")));
        }
    }
    // Row removal happens inside the job on success.
    let job_id = jobs::spawn_delete(state.clone(), name.clone(), volumes).await?;
    Ok(Json(json!({ "job": job_id, "name": name, "volumes": volumes })))
}

/// Start = compose up -d (idempotent; images already exist for a registered
/// sandbox). Short enough to be synchronous.
async fn start_sandbox(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let compose_file = state.instance_dir(&name).join("compose.yml");
    if !compose_file.exists() {
        return Err(ApiError::bad(format!("sandbox {name:?} has no compose file")));
    }
    docker::ensure_network("aio-mgr-net").await?;
    let out = docker::compose_up(&envhash::project_name(&name), &compose_file, false).await?;
    db::update_sandbox_status(&state.db.lock().unwrap(), &name, "running")?;
    Ok(Json(json!({ "ok": true, "output": out.trim() })))
}

async fn stop_sandbox(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let compose_file = state.instance_dir(&name).join("compose.yml");
    let out = docker::compose_stop(&envhash::project_name(&name), &compose_file).await?;
    db::update_sandbox_status(&state.db.lock().unwrap(), &name, "stopped")?;
    Ok(Json(json!({ "ok": true, "output": out.trim() })))
}

async fn restart_sandbox(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let compose_file = state.instance_dir(&name).join("compose.yml");
    let out = docker::compose_restart(&envhash::project_name(&name), &compose_file).await?;
    db::update_sandbox_status(&state.db.lock().unwrap(), &name, "running")?;
    Ok(Json(json!({ "ok": true, "output": out.trim() })))
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
