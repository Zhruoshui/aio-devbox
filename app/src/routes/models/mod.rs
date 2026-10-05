// Model config routes — cc-switch mode (task 10-04-model-config-cc-switch).
//
// Each sandbox owns its independent agent configuration (纯 sandbox 内部配置).
// The mgr's provider library serves only as a template source (方案A: 模板复制).
//
// Routes (mounted in main.rs, win over the `/api/*rest` catch-all):
//   GET  /api/models/config       — the sandbox's full SandboxAgentConfig
//   PUT  /api/models/config       — save the full SandboxAgentConfig
//   POST /api/models/apply/:agent — render one agent's config to native files
//   GET  /api/models/agents       — per-agent installed + live readback
//   GET  /api/models/usage        — per-(agent,model) token aggregation
//   GET  /api/models/templates    — cached mgr provider library (read-only)
//   POST /api/models/discover     — fetch /v1/models list from an endpoint
//   POST /api/models/test         — minimal completion probe
//
// REMOVED in the cc-switch refactor (prd "废弃的功能"):
//   - POST /api/models/import/pi  (pi import)
//   - GET  /api/models/catalog    (models.dev proxy)
//   - GET  /api/models/managed    (mgr managed marker)
//   - PUT/DELETE /api/models/agents/:agent/provider/:id (live provider mgmt)
//   - POST /api/models/agents/:agent/sync (sync to canonical)
//
// All writes are serialized by `state.models_lock`. Corrupt files are moved
// aside (config.json.corrupt-<ts>) and the error surfaced.

pub mod discover;
pub mod render;
pub mod test;
pub mod usage;

use std::path::Path;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use serde_json::{json, Value};

use aio_models::sandbox_config::SandboxAgentConfig;

use crate::state::AppState;
use render::{home_dir, Agent, ApplyResult};

/// GET /api/models/config — return the sandbox's full agent config.
pub async fn get_config(State(state): State<AppState>) -> Json<SandboxAgentConfig> {
    let config = read_sandbox_config(&state).unwrap_or_default();
    Json(config)
}

/// PUT /api/models/config — validate and save the full SandboxAgentConfig.
pub async fn put_config(
    State(state): State<AppState>,
    Json(incoming): Json<SandboxAgentConfig>,
) -> Result<Json<PutConfigResponse>, (StatusCode, String)> {
    let _guard = state.models_lock.lock().await;
    write_sandbox_config(&state, &incoming)?;
    Ok(Json(PutConfigResponse { ok: true }))
}

#[derive(Debug, Serialize)]
pub struct PutConfigResponse {
    pub ok: bool,
}

/// Read the sandbox config from disk. Corrupt → 422 with the file moved aside.
fn read_sandbox_config(state: &AppState) -> Result<SandboxAgentConfig, (StatusCode, String)> {
    let path = &state.models_file;
    match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(SandboxAgentConfig::default()),
        Ok(text) => serde_json::from_str(&text).map_err(|e| {
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("sandbox config corrupt: {e}"),
            )
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SandboxAgentConfig::default()),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("read sandbox config: {e}"),
        )),
    }
}

/// Write the sandbox config atomically (0600 — may contain apiKeys).
fn write_sandbox_config(
    state: &AppState,
    config: &SandboxAgentConfig,
) -> Result<(), (StatusCode, String)> {
    let bytes = serde_json::to_vec_pretty(config).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize sandbox config: {e}"),
        )
    })?;
    render::common::atomic_write(&state.models_file, &bytes, 0o600).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("write sandbox config: {e}"),
        )
    })
}

// ── agent status + apply ──────────────────────────────────────────

/// One agent's install + live-config snapshot. `installed` is
/// `command_exists(<bin>)` on the login-shell PATH. `live` reads the agent's
/// native config files and reports the currently-effective provider/model.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub installed: bool,
    pub bin: Option<String>,
    pub live: Option<Value>,
}

/// Response shape for GET /api/models/agents.
#[derive(Debug, Serialize)]
pub struct AgentsResponse {
    pub pi: AgentStatus,
    pub opencode: AgentStatus,
    pub claude: AgentStatus,
    pub codex: AgentStatus,
}

/// GET /api/models/agents — per-agent install status + live readback.
pub async fn get_agents(State(state): State<AppState>) -> Json<AgentsResponse> {
    let dirs = crate::config::resolve_path_dirs(&state.path_cache).await;
    let home = home_dir();

    Json(AgentsResponse {
        pi: agent_status("pi", &dirs, &home, Agent::Pi),
        opencode: agent_status("opencode", &dirs, &home, Agent::Opencode),
        claude: agent_status("claude", &dirs, &home, Agent::Claude),
        codex: agent_status("codex", &dirs, &home, Agent::Codex),
    })
}

/// POST /api/models/apply/:agent — render the agent's config from
/// SandboxAgentConfig to its native config files.
pub async fn apply_agent(
    State(state): State<AppState>,
    axum::extract::Path(agent): axum::extract::Path<String>,
) -> Result<Json<ApplyResult>, (StatusCode, String)> {
    let _guard = state.models_lock.lock().await;

    let config = read_sandbox_config(&state)?;
    let agent_kind =
        Agent::from_str(&agent).ok_or_else(|| (StatusCode::BAD_REQUEST, format!("unknown agent '{agent}'")))?;

    let home = home_dir();
    let result = render_agent(agent_kind, &home, &config);

    Ok(Json(result))
}

/// Render one agent's config to its native files.
fn render_agent(agent_kind: Agent, home: &Path, config: &SandboxAgentConfig) -> ApplyResult {
    match agent_kind {
        Agent::Pi => match &config.pi {
            Some(pi_config) => render::pi::apply_pi(home, pi_config),
            None => {
                let mut r = ApplyResult::new();
                r.push_err(
                    home.join(".pi/agent/models.json"),
                    "no pi config in sandbox config".into(),
                );
                r
            }
        },
        Agent::Opencode => match &config.opencode {
            Some(oc_config) => render::opencode::apply_opencode(home, oc_config),
            None => {
                let mut r = ApplyResult::new();
                r.push_err(
                    home.join(".config/opencode/opencode.jsonc"),
                    "no opencode config in sandbox config".into(),
                );
                r
            }
        },
        Agent::Claude => match &config.claude {
            Some(claude_config) => render::claude::apply_claude(home, claude_config),
            None => {
                let mut r = ApplyResult::new();
                r.push_err(
                    home.join(".claude/settings.json"),
                    "no claude config in sandbox config".into(),
                );
                r
            }
        },
        Agent::Codex => match &config.codex {
            Some(codex_config) => render::codex::apply_codex(home, codex_config),
            None => {
                let mut r = ApplyResult::new();
                r.push_err(
                    home.join(".codex/config.toml"),
                    "no codex config in sandbox config".into(),
                );
                r
            }
        },
    }
}

/// Build an AgentStatus: probe the binary on PATH and read the agent's
/// current live config.
fn agent_status(bin: &str, dirs: &[std::path::PathBuf], home: &Path, agent: Agent) -> AgentStatus {
    let installed = crate::config::command_exists(bin, dirs);
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
/// config files.
fn read_live(agent: Agent, home: &Path) -> Option<Value> {
    match agent {
        Agent::Pi => {
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

fn read_file_optional(path: &Path) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(s) if s.trim().is_empty() => None,
        Ok(s) => Some(s),
        Err(_) => None,
    }
}

// ── templates (mgr provider library cache) ────────────────────────

/// GET /api/models/templates — the cached mgr provider library (read-only).
/// Returns a masked copy suitable for the template picker UI.
pub async fn get_templates(State(state): State<AppState>) -> Json<Value> {
    // For now, return an empty library. The mgr provider library is fetched
    // on-demand by the mgr-web frontend directly from the mgr API; the sandbox
    // app does not need to proxy it.
    let _ = state;
    Json(json!({ "providers": {} }))
}

// ── tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use aio_models::snapshot::{ProviderSnapshot, SwitchAgentConfig};
    use serde_json::json;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp_dir() -> std::path::PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut p = std::env::temp_dir();
        p.push(format!("aio-models-cc-{}-{}", std::process::id(), n));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn sandbox_config_round_trips_through_file() {
        let dir = temp_dir();
        let path = dir.join("sandbox_config.json");
        let state = AppState::new(
            Vec::new(),
            dir.join("buttons.toml"),
            path.clone(),
        );

        let mut config = SandboxAgentConfig::default();
        let mut claude = SwitchAgentConfig::default();
        claude.providers.push(ProviderSnapshot {
            id: "snap-1".into(),
            name: "Test".into(),
            settings_config: json!({"env": {}}),
            created_at: None,
            notes: None,
        });
        claude.current = Some("snap-1".into());
        config.claude = Some(claude);

        write_sandbox_config(&state, &config).unwrap();
        let back = read_sandbox_config(&state).unwrap();
        assert!(back.claude.is_some());
        assert_eq!(back.claude.unwrap().current.as_deref(), Some("snap-1"));
    }

    #[test]
    fn missing_config_returns_default() {
        let dir = temp_dir();
        let state = AppState::new(
            Vec::new(),
            dir.join("buttons.toml"),
            dir.join("nonexistent.json"),
        );
        let config = read_sandbox_config(&state).unwrap();
        assert!(config.claude.is_none());
        assert!(config.pi.is_none());
    }
}
