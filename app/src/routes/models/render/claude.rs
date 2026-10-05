// claude renderer: writes ~/.claude/settings.json (switch-style snapshot).
//
// claude is a switch-style agent in cc-switch mode: each provider is a
// COMPLETE config snapshot. Switching providers means atomically replacing
// ~/.claude/settings.json with the snapshot's `settings_config` content.
// The snapshot's settings_config is an opaque JSON blob whose shape is the
// full ~/.claude/settings.json (env, permissions, hooks, ...).
//
// When there is no current snapshot (claude block absent, `current` unset,
// or dangling), push an error and write nothing (never a half-applied file).

use std::path::Path;

use aio_models::snapshot::SwitchAgentConfig;

use crate::routes::models::render::common::{backup_write_verify_json, ApplyResult};

/// Apply the current claude snapshot to ~/.claude/settings.json.
pub fn apply_claude(home: &Path, config: &SwitchAgentConfig) -> ApplyResult {
    let mut result = ApplyResult::new();
    let path = home.join(".claude/settings.json");

    let Some(snapshot) = config
        .current
        .as_ref()
        .and_then(|id| config.find(id))
    else {
        result.push_err(path, "no current claude snapshot".into());
        return result;
    };

    // The snapshot's settings_config IS the complete settings.json content.
    // Serialize it pretty and write atomically with 0600 (may contain keys).
    let bytes = match serde_json::to_vec_pretty(&snapshot.settings_config) {
        Ok(b) => b,
        Err(e) => {
            result.push_err(path, format!("serialize settings.json: {e}"));
            return result;
        }
    };

    match backup_write_verify_json(&path, &bytes, 0o600) {
        Ok(backup) => result.push_ok(path, backup),
        Err(msg) => result.push_err(path, msg),
    }

    result
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
        p.push(format!("aio-claude-cc-{}-{}", std::process::id(), n));
        std::fs::create_dir_all(p.join(".claude")).unwrap();
        p
    }

    fn sample_config() -> SwitchAgentConfig {
        let mut config = SwitchAgentConfig::default();
        config.providers.push(ProviderSnapshot {
            id: "snap-1".into(),
            name: "Moonshot".into(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.moonshot.cn/v1",
                    "ANTHROPIC_AUTH_TOKEN": "sk-real-key-xxxx",
                    "ANTHROPIC_MODEL": "kimi-k2"
                },
                "permissions": { "allow": ["Bash(ls)"] }
            }),
            created_at: Some(1728000000),
            notes: None,
        });
        config.current = Some("snap-1".into());
        config
    }

    #[test]
    fn apply_claude_writes_snapshot_contents() {
        let home = temp_home();
        let r = apply_claude(&home, &sample_config());
        assert!(r.ok, "errors: {:?}", r.errors);

        let after: Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".claude/settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            after["env"]["ANTHROPIC_BASE_URL"],
            "https://api.moonshot.cn/v1"
        );
        assert_eq!(after["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-real-key-xxxx");
        assert_eq!(after["env"]["ANTHROPIC_MODEL"], "kimi-k2");
        assert_eq!(after["permissions"]["allow"][0], "Bash(ls)");
    }

    #[test]
    fn apply_claude_replaces_existing_file_entirely() {
        let home = temp_home();
        // Existing settings with different content — snapshot replaces it.
        std::fs::write(
            home.join(".claude/settings.json"),
            r#"{"env": {"OLD_KEY": "old_val"}, "theme": "light"}"#,
        )
        .unwrap();

        let r = apply_claude(&home, &sample_config());
        assert!(r.ok);

        let after: Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".claude/settings.json")).unwrap(),
        )
        .unwrap();
        // Old content gone, snapshot content present.
        assert!(after.get("env").and_then(|e| e.get("OLD_KEY")).is_none());
        assert!(after.get("theme").is_none());
        assert_eq!(after["env"]["ANTHROPIC_MODEL"], "kimi-k2");
    }

    #[test]
    fn apply_claude_creates_file_0600() {
        let home = temp_home();
        let r = apply_claude(&home, &sample_config());
        assert!(r.ok);
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(home.join(".claude/settings.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn apply_claude_no_current_errors() {
        let home = temp_home();
        let mut config = sample_config();
        config.current = None;
        let r = apply_claude(&home, &config);
        assert!(!r.ok);
        assert!(r.errors[0].message.contains("no current claude snapshot"));
        assert!(!home.join(".claude/settings.json").exists());
    }

    #[test]
    fn apply_claude_dangling_current_errors() {
        let home = temp_home();
        let mut config = sample_config();
        config.current = Some("nope".into());
        let r = apply_claude(&home, &config);
        assert!(!r.ok);
        assert!(r.errors[0].message.contains("no current claude snapshot"));
        assert!(!home.join(".claude/settings.json").exists());
    }

    #[test]
    fn apply_claude_empty_config_errors() {
        let home = temp_home();
        let config = SwitchAgentConfig::default();
        let r = apply_claude(&home, &config);
        assert!(!r.ok);
        assert!(r.errors[0].message.contains("no current claude snapshot"));
    }

    #[test]
    fn apply_claude_multi_snapshot_applies_current() {
        let home = temp_home();
        let mut config = sample_config();
        config.providers.push(ProviderSnapshot {
            id: "snap-2".into(),
            name: "OpenRouter".into(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://openrouter.ai/api/v1",
                    "ANTHROPIC_MODEL": "gpt-4o"
                }
            }),
            created_at: None,
            notes: None,
        });
        config.current = Some("snap-2".into());

        let r = apply_claude(&home, &config);
        assert!(r.ok, "errors: {:?}", r.errors);
        let after: Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".claude/settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(after["env"]["ANTHROPIC_BASE_URL"], "https://openrouter.ai/api/v1");
        assert_eq!(after["env"]["ANTHROPIC_MODEL"], "gpt-4o");
        // snap-1's model must NOT leak.
        assert!(after["env"].get("ANTHROPIC_AUTH_TOKEN").is_none());
    }
}
