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
//   GET  /api/models/catalog      — models.dev metadata catalog (1h cache, 08-27-provider-form-piweb)
//   PUT    /api/models/agents/:agent/provider/:id — field-level edit of one live
//          provider node in the agent's native config (08-27-agent-tabs-live-config)
//   DELETE /api/models/agents/:agent/provider/:id — remove one live provider node
//   POST   /api/models/agents/:agent/sync          — absorb live provider(s) into canonical
//
// All writes are serialized by `state.models_lock` (per design §3). Corrupt
// files are moved aside (models.json.corrupt-<ts>) and the error surfaced;
// the next PUT succeeds on a fresh file.

pub mod catalog;
pub mod discover;
pub mod render;
pub mod store;
pub mod test;
pub mod usage;

use std::path::{Path, PathBuf};

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::{command_exists, resolve_path_dirs};
use crate::state::AppState;
use render::{home_dir, ApplyResult, Agent, ProviderPatch};
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

/// Resolve `~/.pi/agent/models.json` from $HOME (default /root).
fn pi_models_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
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

// ── live provider management (08-27-agent-tabs-live-config) ────────

/// Parse the `:agent` segment for the live-manage routes: incremental
/// agents (pi/opencode) only — switch-style agents (claude/codex) manage
/// presets through PUT /api/models/config, not through their native files.
fn incremental_agent(agent: &str) -> Result<Agent, (StatusCode, String)> {
    match Agent::from_str(agent) {
        Some(a @ (Agent::Pi | Agent::Opencode)) => Ok(a),
        Some(_) => Err((
            StatusCode::BAD_REQUEST,
            format!("'{agent}' manages presets in the canonical config, not a live provider list"),
        )),
        None => Err((
            StatusCode::BAD_REQUEST,
            format!("unknown agent '{agent}'"),
        )),
    }
}

/// PUT /api/models/agents/:agent/provider/:id — field-level edit of one
/// live provider node in the agent's native config (design §2). Only the
/// patched keys are merged into the node; models and everything else are
/// preserved by the renderers' key-level merge. Best-effort ApplyResult
/// (200 with ok=false + errors[] when the per-file op failed — same
/// contract as apply).
pub async fn edit_live_provider(
    State(state): State<AppState>,
    axum::extract::Path((agent, provider_id)): axum::extract::Path<(String, String)>,
    Json(patch): Json<ProviderPatch>,
) -> Result<Json<ApplyResult>, (StatusCode, String)> {
    let agent_kind = incremental_agent(&agent)?;
    // Serialize with apply: both write the same native files.
    let _guard = state.models_lock.lock().await;

    let home = home_dir();
    let result = match agent_kind {
        Agent::Pi => render::pi::edit_pi_provider(&home, &provider_id, &patch),
        Agent::Opencode => render::opencode::edit_opencode_provider(&home, &provider_id, &patch),
        _ => unreachable!("incremental_agent filters"),
    };
    Ok(Json(result))
}

/// DELETE /api/models/agents/:agent/provider/:id — remove one live provider
/// node from the agent's native config (dangling default/model keys are
/// cleaned up by the renderers; design §2). Best-effort ApplyResult.
pub async fn delete_live_provider(
    State(state): State<AppState>,
    axum::extract::Path((agent, provider_id)): axum::extract::Path<(String, String)>,
) -> Result<Json<ApplyResult>, (StatusCode, String)> {
    let agent_kind = incremental_agent(&agent)?;
    let _guard = state.models_lock.lock().await;

    let home = home_dir();
    let result = match agent_kind {
        Agent::Pi => render::pi::delete_pi_provider(&home, &provider_id),
        Agent::Opencode => render::opencode::delete_opencode_provider(&home, &provider_id),
        _ => unreachable!("incremental_agent filters"),
    };
    Ok(Json(result))
}

/// Body of POST /api/models/agents/:agent/sync — `id` omitted = sync all.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SyncRequest {
    pub id: Option<String>,
}

/// POST /api/models/agents/:agent/sync — absorb live provider(s) from the
/// agent's native config into the canonical provider library (design §3).
/// Idempotent: ids already canonical land in `skipped`. Single-id sync of a
/// provider absent from the native file → 404.
pub async fn sync_live_provider(
    State(state): State<AppState>,
    axum::extract::Path(agent): axum::extract::Path<String>,
    body: Option<Json<SyncRequest>>,
) -> Result<Json<ImportResponse>, (StatusCode, String)> {
    let agent_kind = incremental_agent(&agent)?;
    let req = body.map(|Json(b)| b).unwrap_or_default();
    let _guard = state.models_lock.lock().await;

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

    let (native_path, result) = match agent_kind {
        Agent::Pi => {
            let p = pi_models_path();
            let r = match &req.id {
                Some(id) => store::import_pi_provider(&p, &config, id),
                None => store::import_from_pi(&p, &config),
            };
            (p, r)
        }
        Agent::Opencode => {
            let p = opencode_jsonc_path();
            let r = match &req.id {
                Some(id) => store::import_opencode_provider(&p, &config, id),
                None => store::import_from_opencode(&p, &config),
            };
            (p, r)
        }
        _ => unreachable!("incremental_agent filters"),
    };

    let result = result.map_err(|e| match e {
        StoreError::Io(err) if err.kind() == std::io::ErrorKind::NotFound => (
            StatusCode::NOT_FOUND,
            format!("{} native config not found", native_path.display()),
        ),
        StoreError::Io(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("read {}: {err}", native_path.display()),
        ),
        StoreError::Corrupt(err) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("{} is corrupt: {err}", native_path.display()),
        ),
    })?;

    // Single-id sync that matched nothing in the native file → 404 (a
    // matched-but-skipped id stays 200 with skipped[], that's idempotency).
    if let Some(id) = &req.id {
        if result.imported.is_empty() && result.skipped.is_empty() {
            return Err((
                StatusCode::NOT_FOUND,
                format!("provider '{id}' not found in {agent} native config"),
            ));
        }
    }

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

/// Resolve `~/.config/opencode/opencode.jsonc` from $HOME (default /root).
fn opencode_jsonc_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home).join(".config/opencode/opencode.jsonc")
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
/// config files. Each agent maps its own shape into a uniform `live` JSON.
/// pi/opencode additionally carry a `providers[]` summary of every provider
/// configured in the native file (08-27-agent-tabs-live-config design §1 —
/// tolerant per-node extraction, never fails the whole read). `None` when
/// nothing is readable (design §3: "live:null (never error the whole
/// request)").
fn read_live(agent: Agent, home: &Path) -> Option<Value> {
    match agent {
        Agent::Pi => {
            // Two files read independently: settings.json defaults +
            // models.json providers summary. Either being readable keeps
            // live non-null.
            let defaults = read_pi_defaults(home);
            let providers = read_pi_providers_summary(home);
            if defaults.is_none() && providers.is_none() {
                return None;
            }
            let (provider, model) = defaults.unwrap_or((None, None));
            Some(json!({
                "provider": provider,
                "model": model,
                "providers": providers.unwrap_or_else(|| json!([])),
            }))
        }
        Agent::Opencode => {
            let path = home.join(".config/opencode/opencode.jsonc");
            let text = read_file_optional(&path)?;
            let v: Value = json5::from_str(&text).ok()?;
            let model = v.get("model").and_then(|x| x.as_str()).map(String::from);
            Some(json!({
                "model": model,
                "providers": opencode_live_providers(&v),
            }))
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

/// (defaultProvider, defaultModel) from ~/.pi/agent/settings.json; None when
/// the file is missing/unparseable.
fn read_pi_defaults(home: &Path) -> Option<(Option<String>, Option<String>)> {
    let text = read_file_optional(&home.join(".pi/agent/settings.json"))?;
    let v: Value = serde_json::from_str(&text).ok()?;
    Some((
        v.get("defaultProvider")
            .and_then(|x| x.as_str())
            .map(String::from),
        v.get("defaultModel")
            .and_then(|x| x.as_str())
            .map(String::from),
    ))
}

/// Tolerant providers summary from ~/.pi/agent/models.json: per provider
/// `{id, name?, api?, baseUrl?, models:[ids]}` — best-effort per node, a
/// non-object node is skipped. None = file missing/unparseable (distinct
/// from a valid file with no providers = empty array). pi nodes carry no
/// `name` field (keyed by id), so name is typically null and the UI falls
/// back to the id.
fn read_pi_providers_summary(home: &Path) -> Option<Value> {
    let text = read_file_optional(&home.join(".pi/agent/models.json"))?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let providers = match v.get("providers") {
        Some(p) if p.is_object() => p.as_object().expect("checked is_object"),
        _ => return Some(json!([])),
    };
    let list: Vec<Value> = providers
        .iter()
        .filter_map(|(id, node)| {
            let obj = node.as_object()?;
            Some(json!({
                "id": id,
                "name": obj.get("name").and_then(|x| x.as_str()),
                "api": obj.get("api").and_then(|x| x.as_str()),
                "baseUrl": obj.get("baseUrl").and_then(|x| x.as_str()),
                "models": obj
                    .get("models")
                    .and_then(|x| x.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|m| m.get("id").and_then(|x| x.as_str()))
                            .collect::<Vec<&str>>()
                    })
                    .unwrap_or_default(),
            }))
        })
        .collect();
    Some(Value::Array(list))
}

/// opencode live provider summary from the parsed opencode.jsonc root:
/// `{id, name?, api? (derived from the npm package — inverse of the
/// renderer's protocol→npm choice), baseUrl?, models:[ids]}`. Non-object
/// fragments are skipped.
fn opencode_live_providers(root: &Value) -> Value {
    let Some(providers) = root.get("provider").and_then(|p| p.as_object()) else {
        return json!([]);
    };
    let list: Vec<Value> = providers
        .iter()
        .filter_map(|(id, frag)| {
            let obj = frag.as_object()?;
            let api = obj.get("npm").and_then(|x| x.as_str()).map(|n| {
                if n.contains("anthropic") {
                    "anthropic-messages"
                } else {
                    "openai-completions"
                }
            });
            Some(json!({
                "id": id,
                "name": obj.get("name").and_then(|x| x.as_str()),
                "api": api,
                "baseUrl": frag.pointer("/options/baseURL").and_then(|x| x.as_str()),
                "models": obj
                    .get("models")
                    .and_then(|x| x.as_object())
                    .map(|m| m.keys().cloned().collect::<Vec<String>>())
                    .unwrap_or_default(),
            }))
        })
        .collect();
    Value::Array(list)
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

// ── tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp_home() -> std::path::PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut p = std::env::temp_dir();
        p.push(format!("aio-models-live-{}-{}", std::process::id(), n));
        std::fs::create_dir_all(p.join(".pi/agent")).unwrap();
        std::fs::create_dir_all(p.join(".config/opencode")).unwrap();
        p
    }

    fn seed_pi(home: &Path) {
        std::fs::write(
            home.join(".pi/agent/models.json"),
            r#"{
  "providers": {
    "prov-a": {
      "baseUrl": "https://a.example/v1",
      "api": "openai-completions",
      "apiKey": "sk-key-xxxx",
      "models": [
        {"id": "model-a", "name": "Model A"},
        {"id": "model-b"}
      ]
    },
    "prov-b": {"baseUrl": "https://b.example/v1"}
  }
}"#,
        )
        .unwrap();
        std::fs::write(
            home.join(".pi/agent/settings.json"),
            r#"{"defaultProvider":"prov-a","defaultModel":"model-a"}"#,
        )
        .unwrap();
    }

    // --- read_live: pi ---

    #[test]
    fn read_live_pi_full() {
        let home = temp_home();
        seed_pi(&home);
        let live = read_live(Agent::Pi, &home).expect("live present");
        assert_eq!(live["provider"], "prov-a");
        assert_eq!(live["model"], "model-a");
        let providers = live["providers"].as_array().unwrap();
        assert_eq!(providers.len(), 2);
        let a = &providers[0];
        assert_eq!(a["id"], "prov-a");
        assert_eq!(a["api"], "openai-completions");
        assert_eq!(a["baseUrl"], "https://a.example/v1");
        // apiKey is NOT part of the summary (secret stays out of readback).
        assert!(a.get("apiKey").is_none());
        assert_eq!(
            a["models"].as_array().unwrap().len(),
            2,
            "model ids listed for the expandable row"
        );
        // pi nodes carry no name -> null, UI falls back to id.
        assert!(a["name"].is_null());
        // Minimal sibling node still summarized.
        assert_eq!(providers[1]["id"], "prov-b");
        assert_eq!(providers[1]["models"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn read_live_pi_models_missing_keeps_defaults() {
        let home = temp_home();
        std::fs::write(
            home.join(".pi/agent/settings.json"),
            r#"{"defaultProvider":"x","defaultModel":"y"}"#,
        )
        .unwrap();
        let live = read_live(Agent::Pi, &home).expect("live from settings alone");
        assert_eq!(live["provider"], "x");
        assert_eq!(live["providers"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn read_live_pi_models_corrupt_degrades_to_defaults() {
        let home = temp_home();
        seed_pi(&home);
        std::fs::write(home.join(".pi/agent/models.json"), "{{not json").unwrap();
        let live = read_live(Agent::Pi, &home).expect("defaults still readable");
        assert_eq!(live["provider"], "prov-a");
        assert_eq!(
            live["providers"].as_array().unwrap().len(),
            0,
            "corrupt models.json -> empty summary, not an error"
        );
    }

    #[test]
    fn read_live_pi_settings_missing_keeps_providers() {
        let home = temp_home();
        seed_pi(&home);
        std::fs::remove_file(home.join(".pi/agent/settings.json")).unwrap();
        let live = read_live(Agent::Pi, &home).expect("providers still readable");
        assert!(live["provider"].is_null());
        assert!(live["model"].is_null());
        assert_eq!(live["providers"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn read_live_pi_both_missing_is_null() {
        let home = temp_home();
        assert!(read_live(Agent::Pi, &home).is_none());
    }

    // --- read_live: opencode ---

    fn seed_opencode(home: &Path, body: &str) {
        std::fs::write(home.join(".config/opencode/opencode.jsonc"), body).unwrap();
    }

    #[test]
    fn read_live_opencode_full() {
        let home = temp_home();
        seed_opencode(
            &home,
            r#"{
  "model": "prov-a/model-a",
  "provider": {
    "prov-a": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "Prov A",
      "options": { "baseURL": "https://a.example/v1", "apiKey": "sk" },
      "models": { "model-a": {"name": "Model A"}, "model-b": {} }
    },
    "antp": {
      "npm": "@ai-sdk/anthropic",
      "options": { "baseURL": "https://api.anthropic.com" }
    }
  }
}"#,
        );
        let live = read_live(Agent::Opencode, &home).expect("live present");
        assert_eq!(live["model"], "prov-a/model-a");
        let providers = live["providers"].as_array().unwrap();
        assert_eq!(providers.len(), 2);
        // serde_json maps are key-sorted — look entries up by id, not index.
        let find = |id: &str| providers.iter().find(|p| p["id"] == id).unwrap();
        let a = find("prov-a");
        assert_eq!(a["name"], "Prov A");
        assert_eq!(a["api"], "openai-completions");
        assert_eq!(a["baseUrl"], "https://a.example/v1");
        assert_eq!(a["models"].as_array().unwrap().len(), 2);
        // api derived from the npm package (inverse of the renderer mapping).
        assert_eq!(find("antp")["api"], "anthropic-messages");
        // apiKey is NOT part of the summary.
        assert!(a.get("apiKey").is_none());
    }

    #[test]
    fn read_live_opencode_json5_comments() {
        let home = temp_home();
        seed_opencode(
            &home,
            r#"{
  // hand-maintained config
  "model": "prov-a/model-a",
  "provider": {
    "prov-a": { "options": { "baseURL": "https://a/v1", }, },
  },
}"#,
        );
        let live = read_live(Agent::Opencode, &home).expect("json5 tolerated");
        assert_eq!(live["model"], "prov-a/model-a");
        assert_eq!(live["providers"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn read_live_opencode_missing_is_null() {
        let home = temp_home();
        assert!(read_live(Agent::Opencode, &home).is_none());
    }

    #[test]
    fn read_live_opencode_corrupt_is_null() {
        let home = temp_home();
        seed_opencode(&home, "{{not even json5");
        assert!(read_live(Agent::Opencode, &home).is_none());
    }

    // --- incremental_agent ---

    #[test]
    fn incremental_agent_accepts_pi_opencode_only() {
        assert!(matches!(incremental_agent("pi"), Ok(Agent::Pi)));
        assert!(matches!(incremental_agent("opencode"), Ok(Agent::Opencode)));
        for bad in ["claude", "codex", "nope"] {
            assert!(incremental_agent(bad).is_err(), "'{bad}' must be rejected");
        }
    }
}
