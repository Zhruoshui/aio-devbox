// codex renderer: writes ~/.codex/auth.json + ~/.codex/config.toml.
//
// Applies the *current* codex preset (design §3): codex is a switch-style
// agent with N presets, only the one `agents.codex.current` points at takes
// effect. auth.json: set only OPENAI_API_KEY, preserve others. config.toml:
// set model_provider="aio", model, optional model_reasoning_effort, and
// [model_providers.aio]{name, base_url(/v1 normalized), wire_api, requires_openai_auth}.
// Preserve every other table/key. Write auth first; if config.toml fails
// (backup / read / serialize / write), roll auth.json back from its backup
// (design §4). codex binary may be absent — files are still written.

use std::path::Path;

use serde_json::{json, Value};
use toml::Value as TomlValue;

use crate::routes::models::render::common::{
    atomic_write, backup_file, read_json_object, ApplyResult, ReadError,
};
use crate::routes::models::store::{CanonicalConfig, CodexPreset, ProviderEntry};

/// Apply the current codex preset to ~/.codex/. When there is no current
/// preset (codex block absent, `current` unset, or dangling), push an error
/// and write nothing (design §3: never a half-applied file).
pub fn apply_codex(home: &Path, canonical: &CanonicalConfig) -> ApplyResult {
    let mut result = ApplyResult::new();

    let auth_path = home.join(".codex/auth.json");
    let config_path = home.join(".codex/config.toml");

    let Some(assignment) = canonical
        .agents
        .codex
        .as_ref()
        .and_then(|p| p.current_preset())
    else {
        result.push_err(config_path.clone(), "no current codex preset".into());
        return result;
    };
    let Some(provider) = canonical.providers.get(&assignment.provider) else {
        result.push_err(
            config_path.clone(),
            format!("provider '{}' not found", assignment.provider),
        );
        return result;
    };

    // 1. auth.json — written first.
    let auth_backup = match backup_file(&auth_path) {
        Ok(b) => b,
        Err(e) => {
            result.push_err(auth_path.clone(), format!("backup auth.json: {e}"));
            return result;
        }
    };
    match write_auth_json(&auth_path, provider, &auth_backup, &mut result) {
        AuthWriteOutcome::Written => {}
        AuthWriteOutcome::Failed => return result,
    }

    // 2. config.toml — any failure rolls back auth.json.
    let config_backup = match backup_file(&config_path) {
        Ok(b) => b,
        Err(e) => {
            rollback_auth(&auth_path, auth_backup.as_deref(), &mut result);
            result.push_err(config_path.clone(), format!("backup config.toml: {e}"));
            return result;
        }
    };

    if let Err(msg) = write_config_toml(&config_path, provider, assignment, &config_backup, &mut result) {
        rollback_auth(&auth_path, auth_backup.as_deref(), &mut result);
        result.push_err(config_path.clone(), msg);
    }

    result
}

enum AuthWriteOutcome {
    Written,
    Failed,
}

/// Write ~/.codex/auth.json with OPENAI_API_KEY set, preserving others.
fn write_auth_json(
    path: &Path,
    provider: &ProviderEntry,
    backup: &Option<String>,
    result: &mut ApplyResult,
) -> AuthWriteOutcome {
    let mut root: Value = match read_json_object(path) {
        Ok(Some(v)) => v,
        Ok(None) => json!({}),
        Err(ReadError::Corrupt(e)) => {
            result.push_err(path.to_path_buf(), format!("corrupt auth.json: {e}"));
            return AuthWriteOutcome::Failed;
        }
        Err(ReadError::Io(e)) => {
            result.push_err(path.to_path_buf(), format!("read auth.json: {e}"));
            return AuthWriteOutcome::Failed;
        }
    };

    let obj = root
        .as_object_mut()
        .expect("read_json_object guarantees object");
    if let Some(key) = &provider.api_key {
        if !key.is_empty() {
            obj.insert("OPENAI_API_KEY".into(), json!(key));
        }
    }

    let bytes = serde_json::to_vec_pretty(&root).expect("serialize auth.json");
    if let Err(e) = atomic_write(path, &bytes, 0o600) {
        result.push_err(path.to_path_buf(), format!("write auth.json: {e}"));
        return AuthWriteOutcome::Failed;
    }
    result.push_ok(path.to_path_buf(), backup.clone());
    AuthWriteOutcome::Written
}

/// Write ~/.codex/config.toml. Returns Err(message) on any failure
/// (corrupt read, parse failure, serialize failure, write failure) so the
/// caller can roll back auth.json. On success, the file is pushed to
/// `result.written`.
fn write_config_toml(
    path: &Path,
    provider: &ProviderEntry,
    assignment: &CodexPreset,
    backup: &Option<String>,
    result: &mut ApplyResult,
) -> Result<(), String> {
    let mut toml_root: TomlValue = match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => TomlValue::Table(Default::default()),
        Ok(text) => match text.parse::<TomlValue>() {
            Ok(v) if v.is_table() => v,
            Ok(_) => {
                return Err("corrupt config.toml: expected table".into());
            }
            Err(e) => {
                return Err(format!("corrupt config.toml, not overwriting: {e}"));
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            TomlValue::Table(Default::default())
        }
        Err(e) => {
            return Err(format!("read config.toml: {e}"));
        }
    };

    let table = toml_root
        .as_table_mut()
        .expect("toml_root is a table at this point");

    // Top-level keys.
    table.insert(
        "model_provider".into(),
        TomlValue::String("aio".into()),
    );
    table.insert("model".into(), TomlValue::String(assignment.model.clone()));
    if let Some(effort) = &assignment.reasoning_effort {
        if !effort.is_empty() {
            table.insert(
                "model_reasoning_effort".into(),
                TomlValue::String(effort.clone()),
            );
        }
    }

    // [model_providers.aio] — replace the "aio" subtable only, preserve siblings.
    let mut mp_outer = match table.remove("model_providers") {
        Some(TomlValue::Table(t)) => t,
        Some(_) => toml::value::Table::new(), // was a non-table — reset
        None => toml::value::Table::new(),
    };

    let mut aio_table = toml::value::Table::new();
    aio_table.insert("name".into(), TomlValue::String(provider.name.clone()));
    aio_table.insert(
        "base_url".into(),
        TomlValue::String(normalize_codex_base_url(&provider.base_url)),
    );
    aio_table.insert(
        "wire_api".into(),
        TomlValue::String(wire_api_for(&assignment.wire_api, &provider.api)),
    );
    aio_table.insert("requires_openai_auth".into(), TomlValue::Boolean(true));

    mp_outer.insert("aio".into(), TomlValue::Table(aio_table));
    table.insert("model_providers".into(), TomlValue::Table(mp_outer));

    // Serialize (self-check before write — design §4).
    let toml_str = match toml::to_string_pretty(&toml_root) {
        Ok(s) => s,
        Err(e) => return Err(format!("serialize config.toml: {e}")),
    };

    if let Err(e) = atomic_write(path, toml_str.as_bytes(), 0o644) {
        return Err(format!("write config.toml: {e}"));
    }

    result.push_ok(path.to_path_buf(), backup.clone());
    Ok(())
}

/// Roll back an already-written auth.json to its pre-apply state. If there
/// was a backup, rename it back (overwriting our write). If there was no
/// backup (file didn't exist before), remove the file we created.
fn rollback_auth(auth_path: &Path, backup: Option<&str>, result: &mut ApplyResult) {
    let auth_str = auth_path.display().to_string();
    match backup {
        Some(b) => {
            let _ = std::fs::rename(b, auth_path);
        }
        None => {
            let _ = std::fs::remove_file(auth_path);
        }
    }
    // Drop auth.json from `written` since it's no longer in its post-apply state.
    result.remove_written(&auth_str);
    result.push_err(
        auth_path.to_path_buf(),
        "rolled back: config.toml failed".into(),
    );
}

// ── pure helpers ──────────────────────────────────────────────────

/// Normalize a base URL for codex's `base_url` field (cc-switch rule,
/// `provider.rs:822-834`): origin-only URLs (no path beyond host) get
/// `/v1` appended; URLs with a path or ending in `/v1` are used as-is
/// (trailing slash trimmed).
pub fn normalize_codex_base_url(base: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    let after_scheme = trimmed
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(trimmed);
    let path = after_scheme
        .split_once('/')
        .map(|(_, p)| p)
        .unwrap_or("");
    if path.is_empty() && !trimmed.ends_with("/v1") {
        format!("{trimmed}/v1")
    } else {
        trimmed.to_string()
    }
}

/// Resolve the effective wire_api: explicit assignment wins; otherwise
/// derive from provider.api (responses for openai-responses, chat
/// otherwise — design §2 compat matrix).
pub fn wire_api_for(wire_api: &str, provider_api: &str) -> String {
    if !wire_api.is_empty() {
        return wire_api.to_string();
    }
    if provider_api == "openai-responses" {
        "responses".to_string()
    } else {
        "chat".to_string()
    }
}

// ── tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::models::store::{
        CanonicalConfig, CodexPreset, CodexPresets, ModelEntry, ProviderEntry,
    };
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp_home() -> std::path::PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut p = std::env::temp_dir();
        p.push(format!("aio-codex-{}-{}", std::process::id(), n));
        std::fs::create_dir_all(p.join(".codex")).unwrap();
        p
    }

    fn sample_config() -> CanonicalConfig {
        let mut c = CanonicalConfig::default();
        c.providers.insert(
            "aruoshui".into(),
            ProviderEntry {
                name: "Aruoshui".into(),
                base_url: "https://ai.aruoshui.com/v1".into(),
                api: "openai-completions".into(),
                api_key: Some("sk-real-key-xxxx".into()),
                models: vec![ModelEntry {
                    id: "deepseek-v4-pro".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        c.agents.codex = Some(CodexPresets {
            presets: vec![CodexPreset {
                id: "p1".into(),
                name: "默认配置".into(),
                provider: "aruoshui".into(),
                model: "deepseek-v4-pro".into(),
                reasoning_effort: Some("medium".into()),
                wire_api: "responses".into(),
            }],
            current: Some("p1".into()),
        });
        c
    }

    // --- pure helpers ---

    #[test]
    fn normalize_origin_only_appends_v1() {
        assert_eq!(
            normalize_codex_base_url("https://api.openai.com"),
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn normalize_with_v1_keeps_as_is() {
        assert_eq!(
            normalize_codex_base_url("https://ai.aruoshui.com/v1"),
            "https://ai.aruoshui.com/v1"
        );
    }

    #[test]
    fn normalize_with_path_keeps_path() {
        assert_eq!(
            normalize_codex_base_url("https://gateway.example/openai"),
            "https://gateway.example/openai"
        );
    }

    #[test]
    fn normalize_trims_trailing_slash() {
        assert_eq!(
            normalize_codex_base_url("https://api.openai.com/v1/"),
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn wire_api_explicit_wins() {
        assert_eq!(wire_api_for("chat", "openai-responses"), "chat");
    }

    #[test]
    fn wire_api_derived_from_provider_api() {
        assert_eq!(wire_api_for("", "openai-responses"), "responses");
        assert_eq!(wire_api_for("", "openai-completions"), "chat");
        assert_eq!(wire_api_for("", "anthropic-messages"), "chat");
    }

    // --- apply_codex golden path ---

    #[test]
    fn apply_codex_preserves_other_keys_and_mcp_servers() {
        let home = temp_home();
        std::fs::write(
            home.join(".codex/auth.json"),
            r#"{"OPENAI_API_KEY":"sk-old","otherKey":"keep"}"#,
        )
        .unwrap();
        std::fs::write(
            home.join(".codex/config.toml"),
            r#"top_level = 7

[model_providers.openai]
name = "OpenAI"
base_url = "https://api.openai.com/v1"

[mcp_servers.filesystem]
command = ["npx"]
args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
"#,
        )
        .unwrap();

        let r = apply_codex(&home, &sample_config());
        assert!(r.ok, "errors: {:?}", r.errors);
        assert_eq!(r.written.len(), 2);

        // auth.json: OPENAI_API_KEY replaced, other key preserved.
        let auth: Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".codex/auth.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(auth["OPENAI_API_KEY"], "sk-real-key-xxxx");
        assert_eq!(auth["otherKey"], "keep");

        // config.toml: our keys set; existing model_providers.openai and
        // mcp_servers preserved.
        let toml_str = std::fs::read_to_string(home.join(".codex/config.toml")).unwrap();
        let parsed: TomlValue = toml_str.parse().unwrap();
        let tbl = parsed.as_table().unwrap();
        assert_eq!(tbl.get("model_provider").and_then(|v| v.as_str()), Some("aio"));
        assert_eq!(
            tbl.get("model").and_then(|v| v.as_str()),
            Some("deepseek-v4-pro")
        );
        assert_eq!(
            tbl.get("model_reasoning_effort").and_then(|v| v.as_str()),
            Some("medium")
        );
        assert_eq!(tbl.get("top_level").and_then(|v| v.as_integer()), Some(7));
        // Other model_providers entry preserved.
        assert_eq!(
            tbl.get("model_providers")
                .and_then(|v| v.as_table())
                .unwrap()
                .get("openai")
                .and_then(|v| v.as_table())
                .unwrap()
                .get("name")
                .and_then(|v| v.as_str()),
            Some("OpenAI")
        );
        // [aio] table set correctly.
        let aio = tbl
            .get("model_providers")
            .and_then(|v| v.as_table())
            .unwrap()
            .get("aio")
            .and_then(|v| v.as_table())
            .unwrap();
        assert_eq!(aio.get("name").and_then(|v| v.as_str()), Some("Aruoshui"));
        assert_eq!(
            aio.get("base_url").and_then(|v| v.as_str()),
            Some("https://ai.aruoshui.com/v1")
        );
        assert_eq!(aio.get("wire_api").and_then(|v| v.as_str()), Some("responses"));
        assert_eq!(
            aio.get("requires_openai_auth").and_then(|v| v.as_bool()),
            Some(true)
        );
        // mcp_servers preserved.
        assert!(tbl.get("mcp_servers").is_some());
    }

    #[test]
    fn apply_codex_creates_files_when_missing() {
        let home = temp_home();
        let r = apply_codex(&home, &sample_config());
        assert!(r.ok);
        use std::os::unix::fs::PermissionsExt;
        let auth_mode = std::fs::metadata(home.join(".codex/auth.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(auth_mode, 0o600);
        let cfg_mode = std::fs::metadata(home.join(".codex/config.toml"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(cfg_mode, 0o644);
    }

    #[test]
    fn apply_codex_origin_only_base_url_normalizes() {
        let home = temp_home();
        let mut cfg = sample_config();
        cfg.providers.get_mut("aruoshui").unwrap().base_url = "https://api.openai.com".into();
        let r = apply_codex(&home, &cfg);
        assert!(r.ok);
        let toml_str = std::fs::read_to_string(home.join(".codex/config.toml")).unwrap();
        assert!(toml_str.contains("base_url = \"https://api.openai.com/v1\""));
    }

    #[test]
    fn apply_codex_wire_api_derived_when_empty() {
        let home = temp_home();
        let mut cfg = sample_config();
        let preset = &mut cfg.agents.codex.as_mut().unwrap().presets[0];
        preset.wire_api = String::new();
        preset.reasoning_effort = None;
        apply_codex(&home, &cfg);
        let toml_str = std::fs::read_to_string(home.join(".codex/config.toml")).unwrap();
        // openai-completions -> chat
        assert!(toml_str.contains("wire_api = \"chat\""));
        // reasoning_effort None -> key absent
        assert!(!toml_str.contains("model_reasoning_effort"));
    }

    #[test]
    fn apply_codex_empty_api_key_omits_openai_key() {
        let home = temp_home();
        let mut cfg = sample_config();
        cfg.providers.get_mut("aruoshui").unwrap().api_key = Some(String::new());
        apply_codex(&home, &cfg);
        let auth: Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".codex/auth.json")).unwrap(),
        )
        .unwrap();
        assert!(auth.get("OPENAI_API_KEY").is_none());
    }

    #[test]
    fn apply_codex_corrupt_auth_aborts() {
        let home = temp_home();
        std::fs::write(home.join(".codex/auth.json"), "{{not json").unwrap();
        let r = apply_codex(&home, &sample_config());
        assert!(!r.ok);
        assert_eq!(
            std::fs::read_to_string(home.join(".codex/auth.json")).unwrap(),
            "{{not json"
        );
        // config.toml never written (auth failed first).
        assert!(!home.join(".codex/config.toml").exists());
    }

    #[test]
    fn apply_codex_corrupt_config_aborts_and_rolls_back_auth() {
        let home = temp_home();
        let original_auth = r#"{"OPENAI_API_KEY":"sk-old"}"#;
        std::fs::write(home.join(".codex/auth.json"), original_auth).unwrap();
        std::fs::write(home.join(".codex/config.toml"), "{{not toml").unwrap();
        let r = apply_codex(&home, &sample_config());

        // config.toml error + auth.json rollback error.
        assert!(!r.ok);
        assert!(r.errors.iter().any(|e| e.path.contains("config.toml")));
        assert!(r.errors.iter().any(|e| e.message.contains("rolled back")));
        // auth.json rolled back to original content (no new key written).
        assert_eq!(
            std::fs::read_to_string(home.join(".codex/auth.json")).unwrap(),
            original_auth
        );
        // auth.json NOT in `written` (rolled back).
        assert!(!r.written.iter().any(|w| w.path.contains("auth.json")));
        // config.toml NOT overwritten (still corrupt).
        assert_eq!(
            std::fs::read_to_string(home.join(".codex/config.toml")).unwrap(),
            "{{not toml"
        );
    }

    #[test]
    fn apply_codex_config_is_directory_rolls_back_auth() {
        // Cheap failure injection: make config.toml a directory so backup_file
        // fails (copy from a directory is an io error) -> auth.json rolls back.
        let home = temp_home();
        let original_auth = r#"{"OPENAI_API_KEY":"sk-old"}"#;
        std::fs::write(home.join(".codex/auth.json"), original_auth).unwrap();
        std::fs::create_dir(home.join(".codex/config.toml")).unwrap();
        let r = apply_codex(&home, &sample_config());

        assert!(!r.ok);
        assert!(r.errors.iter().any(|e| e.path.contains("auth.json") && e.message.contains("rolled back")));
        assert!(r.errors.iter().any(|e| e.path.contains("config.toml")));
        // auth.json restored to original.
        assert_eq!(
            std::fs::read_to_string(home.join(".codex/auth.json")).unwrap(),
            original_auth
        );
        // auth.json NOT in written.
        assert!(!r.written.iter().any(|w| w.path.contains("auth.json")));
    }

    #[test]
    fn apply_codex_missing_assignment_errors() {
        let home = temp_home();
        let mut cfg = sample_config();
        cfg.agents.codex = None;
        let r = apply_codex(&home, &cfg);
        assert!(!r.ok);
        assert!(r.errors[0].message.contains("no current codex preset"));
    }

    #[test]
    fn apply_codex_unset_current_errors() {
        // Presets exist but `current` is None — refuse to write either file.
        let home = temp_home();
        let mut cfg = sample_config();
        cfg.agents.codex.as_mut().unwrap().current = None;
        let r = apply_codex(&home, &cfg);
        assert!(!r.ok);
        assert!(r.errors[0].message.contains("no current codex preset"));
        assert!(!home.join(".codex/auth.json").exists());
        assert!(!home.join(".codex/config.toml").exists());
    }

    #[test]
    fn apply_codex_dangling_current_errors() {
        // `current` points at a preset id that doesn't exist — refuse.
        let home = temp_home();
        let mut cfg = sample_config();
        cfg.agents.codex.as_mut().unwrap().current = Some("nope".into());
        let r = apply_codex(&home, &cfg);
        assert!(!r.ok);
        assert!(r.errors[0].message.contains("no current codex preset"));
        assert!(!home.join(".codex/auth.json").exists());
    }

    #[test]
    fn apply_codex_multi_preset_applies_current() {
        // Two presets; current points at the second — its model/effort win.
        let home = temp_home();
        let mut cfg = sample_config();
        let presets = cfg.agents.codex.as_mut().unwrap();
        presets.presets.push(CodexPreset {
            id: "p2".into(),
            name: "备用".into(),
            provider: "aruoshui".into(),
            model: "deepseek-v4-lite".into(),
            reasoning_effort: None,
            wire_api: "chat".into(),
        });
        presets.current = Some("p2".into());

        let r = apply_codex(&home, &cfg);
        assert!(r.ok, "errors: {:?}", r.errors);
        let toml_str = std::fs::read_to_string(home.join(".codex/config.toml")).unwrap();
        assert!(toml_str.contains("model = \"deepseek-v4-lite\""));
        assert!(toml_str.contains("wire_api = \"chat\""));
        // p1's reasoning effort must not leak (p2 has none).
        assert!(!toml_str.contains("model_reasoning_effort"));
    }
}
