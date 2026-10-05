// codex renderer: writes ~/.codex/auth.json + ~/.codex/config.toml
// (switch-style snapshot).
//
// codex is a switch-style agent in cc-switch mode: each provider is a COMPLETE
// config snapshot. The snapshot's settings_config contains:
//   { "config": "<config.toml string>", "auth": { ... auth.json ... } }
//
// Switching providers means atomically replacing both files. Write auth first;
// if config.toml fails, roll auth.json back from its backup. When there is no
// current snapshot, push an error and write nothing.

use std::path::Path;

use serde_json::Value;
use aio_models::snapshot::SwitchAgentConfig;

use crate::routes::models::render::common::{
    atomic_write, backup_file, read_json_object, ApplyResult, ReadError,
};

/// Apply the current codex snapshot to ~/.codex/. When there is no current
/// snapshot (codex block absent, `current` unset, or dangling), push an error
/// and write nothing.
pub fn apply_codex(home: &Path, config: &SwitchAgentConfig) -> ApplyResult {
    let mut result = ApplyResult::new();

    let auth_path = home.join(".codex/auth.json");
    let config_path = home.join(".codex/config.toml");

    let Some(snapshot) = config
        .current
        .as_ref()
        .and_then(|id| config.find(id))
    else {
        result.push_err(config_path.clone(), "no current codex snapshot".into());
        return result;
    };

    // Extract the config.toml string and auth.json object from the snapshot.
    let toml_str = snapshot
        .settings_config
        .get("config")
        .and_then(|c| c.as_str())
        .unwrap_or("");
    let auth_value = snapshot
        .settings_config
        .get("auth")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));

    // 1. auth.json — written first.
    let auth_backup = match backup_file(&auth_path) {
        Ok(b) => b,
        Err(e) => {
            result.push_err(auth_path.clone(), format!("backup auth.json: {e}"));
            return result;
        }
    };
    match write_auth_json(&auth_path, &auth_value, &auth_backup, &mut result) {
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

    if let Err(msg) = write_config_toml(&config_path, toml_str, &config_backup, &mut result) {
        rollback_auth(&auth_path, auth_backup.as_deref(), &mut result);
        result.push_err(config_path.clone(), msg);
    }

    result
}

enum AuthWriteOutcome {
    Written,
    Failed,
}

/// Write ~/.codex/auth.json from the snapshot's auth object.
fn write_auth_json(
    path: &Path,
    auth_value: &Value,
    backup: &Option<String>,
    result: &mut ApplyResult,
) -> AuthWriteOutcome {
    // Merge: preserve existing auth.json keys not in the snapshot.
    let mut root: Value = match read_json_object(path) {
        Ok(Some(v)) => v,
        Ok(None) => serde_json::json!({}),
        Err(ReadError::Corrupt(e)) => {
            result.push_err(path.to_path_buf(), format!("corrupt auth.json: {e}"));
            return AuthWriteOutcome::Failed;
        }
        Err(ReadError::Io(e)) => {
            result.push_err(path.to_path_buf(), format!("read auth.json: {e}"));
            return AuthWriteOutcome::Failed;
        }
    };

    // Merge snapshot auth keys into existing (snapshot wins).
    if let (Some(existing_obj), Some(snapshot_obj)) =
        (root.as_object_mut(), auth_value.as_object())
    {
        for (k, v) in snapshot_obj {
            existing_obj.insert(k.clone(), v.clone());
        }
    } else {
        root = auth_value.clone();
    }

    let bytes = match serde_json::to_vec_pretty(&root) {
        Ok(b) => b,
        Err(e) => {
            result.push_err(path.to_path_buf(), format!("serialize auth.json: {e}"));
            return AuthWriteOutcome::Failed;
        }
    };
    if let Err(e) = atomic_write(path, &bytes, 0o600) {
        result.push_err(path.to_path_buf(), format!("write auth.json: {e}"));
        return AuthWriteOutcome::Failed;
    }
    result.push_ok(path.to_path_buf(), backup.clone());
    AuthWriteOutcome::Written
}

/// Write ~/.codex/config.toml from the snapshot's config string.
fn write_config_toml(
    path: &Path,
    toml_str: &str,
    backup: &Option<String>,
    result: &mut ApplyResult,
) -> Result<(), String> {
    // Validate the TOML before writing (self-check).
    if !toml_str.is_empty() {
        if let Err(e) = toml_str.parse::<toml::Value>() {
            return Err(format!("corrupt config.toml in snapshot: {e}"));
        }
    }

    let content = if toml_str.is_empty() {
        String::new()
    } else {
        toml_str.to_string()
    };

    if let Err(e) = atomic_write(path, content.as_bytes(), 0o644) {
        return Err(format!("write config.toml: {e}"));
    }

    result.push_ok(path.to_path_buf(), backup.clone());
    Ok(())
}

/// Roll back an already-written auth.json to its pre-apply state.
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
    result.remove_written(&auth_str);
    result.push_err(
        auth_path.to_path_buf(),
        "rolled back: config.toml failed".into(),
    );
}

// ── tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use aio_models::snapshot::{ProviderSnapshot, SwitchAgentConfig};
    use serde_json::{json, Value};
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp_home() -> std::path::PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut p = std::env::temp_dir();
        p.push(format!("aio-codex-cc-{}-{}", std::process::id(), n));
        std::fs::create_dir_all(p.join(".codex")).unwrap();
        p
    }

    fn sample_config() -> SwitchAgentConfig {
        let mut config = SwitchAgentConfig::default();
        config.providers.push(ProviderSnapshot {
            id: "snap-1".into(),
            name: "Default".into(),
            settings_config: json!({
                "config": "model_provider = \"aio\"\nmodel = \"deepseek-v4-pro\"\n",
                "auth": { "OPENAI_API_KEY": "sk-real-key-xxxx" }
            }),
            created_at: Some(1728000000),
            notes: None,
        });
        config.current = Some("snap-1".into());
        config
    }

    #[test]
    fn apply_codex_writes_both_files() {
        let home = temp_home();
        let r = apply_codex(&home, &sample_config());
        assert!(r.ok, "errors: {:?}", r.errors);
        assert_eq!(r.written.len(), 2);

        // auth.json
        let auth: Value =
            serde_json::from_str(&std::fs::read_to_string(home.join(".codex/auth.json")).unwrap())
                .unwrap();
        assert_eq!(auth["OPENAI_API_KEY"], "sk-real-key-xxxx");

        // config.toml
        let toml_str = std::fs::read_to_string(home.join(".codex/config.toml")).unwrap();
        assert!(toml_str.contains("model_provider = \"aio\""));
        assert!(toml_str.contains("model = \"deepseek-v4-pro\""));
    }

    #[test]
    fn apply_codex_preserves_existing_auth_keys() {
        let home = temp_home();
        std::fs::write(
            home.join(".codex/auth.json"),
            r#"{"otherKey": "keep"}"#,
        )
        .unwrap();

        let r = apply_codex(&home, &sample_config());
        assert!(r.ok);

        let auth: Value =
            serde_json::from_str(&std::fs::read_to_string(home.join(".codex/auth.json")).unwrap())
                .unwrap();
        assert_eq!(auth["OPENAI_API_KEY"], "sk-real-key-xxxx");
        assert_eq!(auth["otherKey"], "keep");
    }

    #[test]
    fn apply_codex_creates_files_with_correct_permissions() {
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
    fn apply_codex_no_current_errors() {
        let home = temp_home();
        let mut config = sample_config();
        config.current = None;
        let r = apply_codex(&home, &config);
        assert!(!r.ok);
        assert!(r.errors[0].message.contains("no current codex snapshot"));
        assert!(!home.join(".codex/auth.json").exists());
        assert!(!home.join(".codex/config.toml").exists());
    }

    #[test]
    fn apply_codex_dangling_current_errors() {
        let home = temp_home();
        let mut config = sample_config();
        config.current = Some("nope".into());
        let r = apply_codex(&home, &config);
        assert!(!r.ok);
        assert!(r.errors[0].message.contains("no current codex snapshot"));
    }

    #[test]
    fn apply_codex_multi_snapshot_applies_current() {
        let home = temp_home();
        let mut config = sample_config();
        config.providers.push(ProviderSnapshot {
            id: "snap-2".into(),
            name: "Backup".into(),
            settings_config: json!({
                "config": "model_provider = \"aio\"\nmodel = \"gpt-4o\"\n",
                "auth": { "OPENAI_API_KEY": "sk-other-key" }
            }),
            created_at: None,
            notes: None,
        });
        config.current = Some("snap-2".into());

        let r = apply_codex(&home, &config);
        assert!(r.ok, "errors: {:?}", r.errors);
        let toml_str = std::fs::read_to_string(home.join(".codex/config.toml")).unwrap();
        assert!(toml_str.contains("model = \"gpt-4o\""));
        let auth: Value =
            serde_json::from_str(&std::fs::read_to_string(home.join(".codex/auth.json")).unwrap())
                .unwrap();
        assert_eq!(auth["OPENAI_API_KEY"], "sk-other-key");
    }
}
