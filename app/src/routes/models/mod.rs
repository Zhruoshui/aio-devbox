// Model config routes — canonical store + (M0/M1) GET/PUT config and pi
// import, (M2) discover + test, (M3) renderers + apply + agent status.
//
// Routes (mounted in main.rs, win over the `/api/*rest` catch-all):
//   GET  /api/models/config       — full canonical (apiKey masked)
//   PUT  /api/models/config       — full canonical (masked-echo merge, validate, write)
//   POST /api/models/import/pi    — import providers from ~/.pi/agent/models.json
//   POST /api/models/discover     — fetch /v1/models list from an endpoint (design §5)
//   POST /api/models/test         — minimal completion probe (design §5)
//   GET  /api/models/agents       — per-agent installed + live readback (design §3)
//   POST /api/models/apply/:agent — render canonical assignment to the agent's files
//   GET  /api/models/usage        — per-(agent,model) token aggregation (design §6)
//
// All writes are serialized by `state.models_lock` (per design §3). Corrupt
// files are moved aside (models.json.corrupt-<ts>) and the error surfaced;
// the next PUT succeeds on a fresh file.

pub mod discover;
pub mod render;
pub mod store;
pub mod test;
pub mod usage;

use std::path::{Path, PathBuf};

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use serde_json::{json, Value};

use crate::config::{command_exists, resolve_path_dirs};
use crate::state::AppState;
use render::{home_dir, ApplyResult, Agent};
use store::{
    ensure_preset_ids, merge_api_keys, mask_config, read_config, validate, write_config,
    CanonicalConfig, ImportResponse, PutResponse, StoreError,
};

/// GET /api/models/config — return the full canonical config with masked keys.
pub async fn get_config(State(state): State<AppState>) -> Json<CanonicalConfig> {
    let mut config = read_config(&state.models_file).unwrap_or_default();
    mask_config(&mut config);
    Json(config)
}

/// PUT /api/models/config — merge masked-echo keys, validate, atomic write.
pub async fn put_config(
    State(state): State<AppState>,
    Json(mut incoming): Json<CanonicalConfig>,
) -> Result<Json<PutResponse>, (StatusCode, String)> {
    let _guard = state.models_lock.lock().await;

    // Read stored config; handle corrupt file by moving it aside.
    let stored = match read_config(&state.models_file) {
        Ok(c) => c,
        Err(StoreError::Corrupt(e)) => {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let backup = state
                .models_file
                .with_extension(format!("json.corrupt-{ts}"));
            let _ = std::fs::rename(&state.models_file, &backup);
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                format!(
                    "models.json is corrupt ({e}); moved to {}. Retry to start fresh.",
                    backup.display()
                ),
            ));
        }
        Err(StoreError::Io(e)) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("read models.json: {e}"),
            ));
        }
    };

    // Merge masked-echo keys (frontend sends mask back when unchanged).
    merge_api_keys(&stored, &mut incoming);

    // Backfill preset ids for presets the frontend created without one
    // (design §2: backend owns id generation, frontend makes no assumptions).
    ensure_preset_ids(&mut incoming);

    // Validate.
    validate(&incoming).map_err(|errs| (StatusCode::BAD_REQUEST, errs.join("; ")))?;

    // Ensure version is always 1 on write.
    incoming.version = 1;

    write_config(&state.models_file, &incoming)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("write models.json: {e}")))?;

    Ok(Json(PutResponse {
        ok: true,
        warnings: vec![],
    }))
}

/// POST /api/models/import/pi — import providers from pi's models.json.
pub async fn import_pi(
    State(state): State<AppState>,
) -> Result<Json<ImportResponse>, (StatusCode, String)> {
    let _guard = state.models_lock.lock().await;

    let pi_path = pi_models_path();

    let mut config = match read_config(&state.models_file) {
        Ok(c) => c,
        Err(StoreError::Corrupt(e)) => {
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("models.json is corrupt: {e}"),
            ));
        }
        Err(StoreError::Io(e)) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("read models.json: {e}"),
            ));
        }
    };

    let result = store::import_from_pi(&pi_path, &config).map_err(|e| match e {
        StoreError::Io(e) if e.kind() == std::io::ErrorKind::NotFound => {
            (StatusCode::NOT_FOUND, "pi models.json not found".to_string())
        }
        StoreError::Io(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("read pi models.json: {e}"),
        ),
        StoreError::Corrupt(e) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("pi models.json is corrupt: {e}"),
        ),
    })?;

    for (id, provider) in result.providers {
        config.providers.insert(id, provider);
    }

    write_config(&state.models_file, &config)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("write models.json: {e}")))?;

    Ok(Json(ImportResponse {
        ok: true,
        imported: result.imported,
        skipped: result.skipped,
    }))
}

/// Resolve `~/.pi/agent/models.json` from $HOME (default /home/gem).
fn pi_models_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/gem".to_string());
    PathBuf::from(home).join(".pi/agent/models.json")
}

// ── M3: agent status + apply ──────────────────────────────────────

/// One agent's install + live-config snapshot (design §3 /api/models/agents).
/// `installed` is `command_exists(<bin>)` on the login-shell PATH. `bin` is
/// the resolved path (or null). `live` reads the agent's native config files
/// and reports the currently-effective provider/model (or null when the file
/// is missing/unparseable — never errors the whole request).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub installed: bool,
    pub bin: Option<String>,
    pub live: Option<Value>,
}

/// Response shape for GET /api/models/agents. The frontend decodes this
/// once in ModelsPane (cross-layer-thinking-guide: one boundary owner).
#[derive(Debug, Serialize)]
pub struct AgentsResponse {
    pub pi: AgentStatus,
    pub opencode: AgentStatus,
    pub claude: AgentStatus,
    pub codex: AgentStatus,
}

/// GET /api/models/agents — per-agent install status + live readback.
pub async fn get_agents(State(state): State<AppState>) -> Json<AgentsResponse> {
    let dirs = resolve_path_dirs(&state.path_cache).await;
    let home = home_dir();

    Json(AgentsResponse {
        pi: agent_status("pi", &dirs, &home, Agent::Pi),
        opencode: agent_status("opencode", &dirs, &home, Agent::Opencode),
        claude: agent_status("claude", &dirs, &home, Agent::Claude),
        codex: agent_status("codex", &dirs, &home, Agent::Codex),
    })
}

/// POST /api/models/apply/:agent — render the canonical assignment for the
/// given agent to its native config files (design §3). Returns 200 with an
/// ApplyResult (ok=false when any per-file error happened; errors[] lists
/// them). 400 when the agent has no assignment or the path segment is
/// unknown; 500 on catastrophic canonical-read failure.
pub async fn apply_agent(
    State(state): State<AppState>,
    axum::extract::Path(agent): axum::extract::Path<String>,
) -> Result<Json<ApplyResult>, (StatusCode, String)> {
    let _guard = state.models_lock.lock().await;

    let canonical = match read_config(&state.models_file) {
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

    // 400 when the assignment is absent (design §3). The renderers also
    // defend against this, but returning 400 here gives a clean contract
    // to the frontend (the response body distinguishes "no assignment"
    // from "file write failed").
    let agent_kind = Agent::from_str(&agent).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!("unknown agent '{agent}'"),
        )
    })?;
    let has_assignment = match agent_kind {
        Agent::Pi => canonical.agents.pi.is_some(),
        Agent::Opencode => canonical.agents.opencode.is_some(),
        Agent::Claude => canonical.agents.claude.is_some(),
        Agent::Codex => canonical.agents.codex.is_some(),
    };
    if !has_assignment {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("no {agent} assignment in canonical config"),
        ));
    }

    let home = home_dir();
    let result = match agent_kind {
        Agent::Pi => render::pi::apply_pi(&home, &canonical),
        Agent::Opencode => render::opencode::apply_opencode(&home, &canonical),
        Agent::Claude => render::claude::apply_claude(&home, &canonical),
        Agent::Codex => render::codex::apply_codex(&home, &canonical),
    };

    Ok(Json(result))
}

/// Build an AgentStatus: probe the binary on PATH and read the agent's
/// current live config. Missing/unparseable files => live: null (never
/// errors the whole request — design §3).
fn agent_status(bin: &str, dirs: &[PathBuf], home: &Path, agent: Agent) -> AgentStatus {
    let installed = command_exists(bin, dirs);
    let bin_path = if installed {
        dirs.iter().find_map(|d| {
            let p = d.join(bin);
            if p.is_file() {
                Some(p.display().to_string())
            } else {
                None
            }
        })
    } else {
        None
    };
    let live = read_live(agent, home);
    AgentStatus {
        installed,
        bin: bin_path,
        live,
    }
}

/// Read the currently-effective provider/model for an agent from its native
/// config files. Each agent maps its own shape into a uniform `{provider?,
/// model?, baseUrl?, modelProvider?}` JSON. `None` when the file is missing
/// or unparseable (design §3: "live:null (never error the whole request)").
fn read_live(agent: Agent, home: &Path) -> Option<Value> {
    match agent {
        Agent::Pi => {
            let path = home.join(".pi/agent/settings.json");
            let text = read_file_optional(&path)?;
            let v: Value = serde_json::from_str(&text).ok()?;
            let provider = v
                .get("defaultProvider")
                .and_then(|x| x.as_str())
                .map(String::from);
            let model = v
                .get("defaultModel")
                .and_then(|x| x.as_str())
                .map(String::from);
            Some(json!({ "provider": provider, "model": model }))
        }
        Agent::Opencode => {
            let path = home.join(".config/opencode/opencode.jsonc");
            let text = read_file_optional(&path)?;
            let v: Value = json5::from_str(&text).ok()?;
            let model = v.get("model").and_then(|x| x.as_str()).map(String::from);
            Some(json!({ "model": model }))
        }
        Agent::Claude => {
            let path = home.join(".claude/settings.json");
            let text = read_file_optional(&path)?;
            let v: Value = serde_json::from_str(&text).ok()?;
            let base_url = v
                .get("env")
                .and_then(|e| e.get("ANTHROPIC_BASE_URL"))
                .and_then(|x| x.as_str())
                .map(String::from);
            let model = v
                .get("env")
                .and_then(|e| e.get("ANTHROPIC_MODEL"))
                .and_then(|x| x.as_str())
                .map(String::from);
            Some(json!({ "baseUrl": base_url, "model": model }))
        }
        Agent::Codex => {
            let path = home.join(".codex/config.toml");
            let text = read_file_optional(&path)?;
            let v: toml::Value = text.parse().ok()?;
            let model_provider = v
                .get("model_provider")
                .and_then(|x| x.as_str())
                .map(String::from);
            let model = v.get("model").and_then(|x| x.as_str()).map(String::from);
            Some(json!({ "modelProvider": model_provider, "model": model }))
        }
    }
}

/// Read a file's contents, returning None on missing-file (so callers can
/// treat "no file" as "no live config" without distinguishing io errors).
fn read_file_optional(path: &Path) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(s) if s.trim().is_empty() => None,
        Ok(s) => Some(s),
        Err(_) => None,
    }
}
