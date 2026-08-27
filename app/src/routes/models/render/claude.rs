// claude renderer: writes ~/.claude/settings.json (env-key merge).
//
// Applies the *current* claude preset (design §3): claude is a switch-style
// agent with N presets, only the one `agents.claude.current` points at takes
// effect. Sets ONLY the env keys: ANTHROPIC_BASE_URL (provider.baseUrl —
// protocol selection decides the endpoint; an anthropic-messages provider's
// baseUrl IS the anthropic URL), ANTHROPIC_AUTH_TOKEN or ANTHROPIC_API_KEY
// (per the preset's authField), ANTHROPIC_MODEL, and optional
// ANTHROPIC_DEFAULT_{HAIKU,SONNET,OPUS}_MODEL. Preserves every other key at
// every level (permissions, hooks, unrelated env vars). claude binary may be
// absent — the file is still written so it activates the moment claude is
// installed (design §4).

use std::path::Path;

use serde_json::{json, Value};

use crate::routes::models::render::common::{
    backup_write_verify_json, read_json_object, ApplyResult, ReadError,
};
use crate::routes::models::store::CanonicalConfig;

/// Apply the current claude preset to ~/.claude/settings.json. When there
/// is no current preset (claude block absent, `current` unset, or dangling),
/// push an error and write nothing (design §3: never a half-applied file).
pub fn apply_claude(home: &Path, canonical: &CanonicalConfig) -> ApplyResult {
    let mut result = ApplyResult::new();

    let path = home.join(".claude/settings.json");

    let Some(assignment) = canonical
        .agents
        .claude
        .as_ref()
        .and_then(|p| p.current_preset())
    else {
        result.push_err(path, "no current claude preset".into());
        return result;
    };
    let Some(provider) = canonical.providers.get(&assignment.provider) else {
        result.push_err(
            path,
            format!("provider '{}' not found", assignment.provider),
        );
        return result;
    };

    let mut root: Value = match read_json_object(&path) {
        Ok(Some(v)) => v,
        Ok(None) => json!({}),
        Err(ReadError::Corrupt(e)) => {
            result.push_err(path, format!("corrupt, not overwriting: {e}"));
            return result;
        }
        Err(ReadError::Io(e)) => {
            result.push_err(path, format!("read: {e}"));
            return result;
        }
    };

    let obj = root
        .as_object_mut()
        .expect("read_json_object guarantees object");

    // Get or create the env object.
    let env = obj.entry("env").or_insert_with(|| json!({}));
    if !env.is_object() {
        *env = json!({});
    }
    let env_obj = env.as_object_mut().unwrap();

    // Anthropic baseUrl: the provider's baseUrl IS the anthropic endpoint
    // (protocol selection decides compatibility — R1 removed the separate
    // anthropic override block).
    env_obj.insert("ANTHROPIC_BASE_URL".into(), json!(provider.base_url));

    let auth_key = match assignment.auth_field.as_str() {
        "API_KEY" => "ANTHROPIC_API_KEY",
        _ => "ANTHROPIC_AUTH_TOKEN", // default AUTH_TOKEN
    };
    if let Some(key) = &provider.api_key {
        if !key.is_empty() {
            env_obj.insert(auth_key.to_string(), json!(key));
        }
    }

    env_obj.insert("ANTHROPIC_MODEL".into(), json!(assignment.model));

    // Three-tier model overrides. When non-null, set the env key. When null,
    // DELETE any existing key so a stale old value doesn't point at a model
    // the user no longer wants (design §4 / task spec: "when null, DELETE
    // any existing ANTHROPIC_DEFAULT_*_MODEL keys so they don't stale-point").
    if let Some(h) = &assignment.haiku_model {
        env_obj.insert("ANTHROPIC_DEFAULT_HAIKU_MODEL".into(), json!(h));
    } else {
        env_obj.remove("ANTHROPIC_DEFAULT_HAIKU_MODEL");
    }
    if let Some(s) = &assignment.sonnet_model {
        env_obj.insert("ANTHROPIC_DEFAULT_SONNET_MODEL".into(), json!(s));
    } else {
        env_obj.remove("ANTHROPIC_DEFAULT_SONNET_MODEL");
    }
    if let Some(o) = &assignment.opus_model {
        env_obj.insert("ANTHROPIC_DEFAULT_OPUS_MODEL".into(), json!(o));
    } else {
        env_obj.remove("ANTHROPIC_DEFAULT_OPUS_MODEL");
    }

    let bytes = serde_json::to_vec_pretty(&root).expect("serialize claude settings.json");
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
    use crate::routes::models::store::{
        CanonicalConfig, ClaudePreset, ClaudePresets, ModelEntry, ProviderEntry,
    };
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp_home() -> std::path::PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut p = std::env::temp_dir();
        p.push(format!("aio-claude-{}-{}", std::process::id(), n));
        std::fs::create_dir_all(p.join(".claude")).unwrap();
        p
    }

    fn sample_config() -> CanonicalConfig {
        let mut c = CanonicalConfig::default();
        c.providers.insert(
            "aruoshui".into(),
            ProviderEntry {
                name: "Aruoshui".into(),
                base_url: "https://ai.aruoshui.com/v1".into(),
                api: "anthropic-messages".into(),
                api_key: Some("sk-real-key-xxxx".into()),
                models: vec![ModelEntry {
                    id: "claude-sonnet-4".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        c.agents.claude = Some(ClaudePresets {
            presets: vec![ClaudePreset {
                id: "p1".into(),
                name: "默认配置".into(),
                provider: "aruoshui".into(),
                model: "claude-sonnet-4".into(),
                haiku_model: Some("claude-haiku-4".into()),
                sonnet_model: None,
                opus_model: Some("claude-opus-4".into()),
                auth_field: "AUTH_TOKEN".into(),
            }],
            current: Some("p1".into()),
        });
        c
    }

    #[test]
    fn apply_claude_preserves_permissions_hooks_unrelated_env() {
        let home = temp_home();
        std::fs::write(
            home.join(".claude/settings.json"),
            r#"{
  "env": { "FOO": "1", "ANTHROPIC_MODEL": "old-model" },
  "permissions": { "allow": ["Bash(ls)"] },
  "hooks": { "PreToolUse": [{"matcher":"*","hooks":[{"type":"command","command":"echo"}]}] }
}"#,
        )
        .unwrap();

        let r = apply_claude(&home, &sample_config());
        assert!(r.ok);
        let after: Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".claude/settings.json")).unwrap(),
        )
        .unwrap();

        // Env keys set correctly.
        assert_eq!(after["env"]["ANTHROPIC_BASE_URL"], "https://ai.aruoshui.com/v1");
        assert_eq!(after["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-real-key-xxxx");
        assert_eq!(after["env"]["ANTHROPIC_MODEL"], "claude-sonnet-4");
        assert_eq!(after["env"]["ANTHROPIC_DEFAULT_HAIKU_MODEL"], "claude-haiku-4");
        assert_eq!(after["env"]["ANTHROPIC_DEFAULT_OPUS_MODEL"], "claude-opus-4");
        // sonnetModel is None -> key absent.
        assert!(after["env"].get("ANTHROPIC_DEFAULT_SONNET_MODEL").is_none());

        // Unrelated env var preserved.
        assert_eq!(after["env"]["FOO"], "1");
        // Other top-level keys preserved.
        assert_eq!(after["permissions"]["allow"][0], "Bash(ls)");
        assert_eq!(after["hooks"]["PreToolUse"][0]["matcher"], "*");
    }

    #[test]
    fn auth_field_api_key_writes_anthropic_api_key() {
        let home = temp_home();
        let mut cfg = sample_config();
        cfg.agents.claude.as_mut().unwrap().presets[0].auth_field = "API_KEY".into();
        apply_claude(&home, &cfg);
        let after: Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".claude/settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(after["env"]["ANTHROPIC_API_KEY"], "sk-real-key-xxxx");
        assert!(after["env"].get("ANTHROPIC_AUTH_TOKEN").is_none());
    }

    #[test]
    fn empty_api_key_omits_auth_field() {
        let home = temp_home();
        let mut cfg = sample_config();
        cfg.providers
            .get_mut("aruoshui")
            .unwrap()
            .api_key = Some(String::new());
        apply_claude(&home, &cfg);
        let after: Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".claude/settings.json")).unwrap(),
        )
        .unwrap();
        assert!(after["env"].get("ANTHROPIC_AUTH_TOKEN").is_none());
        assert!(after["env"].get("ANTHROPIC_API_KEY").is_none());
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
    fn apply_claude_corrupt_aborts() {
        let home = temp_home();
        std::fs::write(home.join(".claude/settings.json"), "{{not json").unwrap();
        let r = apply_claude(&home, &sample_config());
        assert!(!r.ok);
        assert!(r.errors[0].message.contains("corrupt"));
        assert_eq!(
            std::fs::read_to_string(home.join(".claude/settings.json")).unwrap(),
            "{{not json"
        );
    }

    #[test]
    fn apply_claude_missing_assignment_errors() {
        let home = temp_home();
        let mut cfg = sample_config();
        cfg.agents.claude = None;
        let r = apply_claude(&home, &cfg);
        assert!(!r.ok);
        assert!(r.errors[0].message.contains("no current claude preset"));
    }

    #[test]
    fn apply_claude_unset_current_errors() {
        // Presets exist but `current` is None — nothing takes effect, refuse
        // to write (design §3: current None -> err, no half-applied file).
        let home = temp_home();
        let mut cfg = sample_config();
        cfg.agents.claude.as_mut().unwrap().current = None;
        let r = apply_claude(&home, &cfg);
        assert!(!r.ok);
        assert!(r.errors[0].message.contains("no current claude preset"));
        assert!(!home.join(".claude/settings.json").exists());
    }

    #[test]
    fn apply_claude_dangling_current_errors() {
        // `current` points at a preset id that doesn't exist — refuse.
        let home = temp_home();
        let mut cfg = sample_config();
        cfg.agents.claude.as_mut().unwrap().current = Some("nope".into());
        let r = apply_claude(&home, &cfg);
        assert!(!r.ok);
        assert!(r.errors[0].message.contains("no current claude preset"));
        assert!(!home.join(".claude/settings.json").exists());
    }

    #[test]
    fn apply_claude_multi_preset_applies_current() {
        // Two presets; current points at the second — its model/authField win
        // and the first preset's values must NOT leak into the file.
        let home = temp_home();
        let mut cfg = sample_config();
        let presets = cfg.agents.claude.as_mut().unwrap();
        presets.presets.push(ClaudePreset {
            id: "p2".into(),
            name: "备用".into(),
            provider: "aruoshui".into(),
            model: "claude-opus-4".into(),
            haiku_model: None,
            sonnet_model: None,
            opus_model: None,
            auth_field: "API_KEY".into(),
        });
        presets.current = Some("p2".into());

        let r = apply_claude(&home, &cfg);
        assert!(r.ok, "errors: {:?}", r.errors);
        let after: Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".claude/settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(after["env"]["ANTHROPIC_MODEL"], "claude-opus-4");
        assert_eq!(after["env"]["ANTHROPIC_API_KEY"], "sk-real-key-xxxx");
        // p2 has no three-tier overrides -> stale keys from p1 must not exist
        // (file is fresh here, but assert the None-deletion behavior anyway).
        assert!(after["env"].get("ANTHROPIC_DEFAULT_HAIKU_MODEL").is_none());
        assert!(after["env"].get("ANTHROPIC_AUTH_TOKEN").is_none());
    }

    #[test]
    fn apply_claude_deletes_stale_default_models_when_null() {
        // Existing settings.json has all three ANTHROPIC_DEFAULT_*_MODEL keys.
        // Applying with haikuModel/sonnetModel/opusModel = None must DELETE
        // them so they don't stale-point at old models (task spec).
        let home = temp_home();
        std::fs::write(
            home.join(".claude/settings.json"),
            r#"{"env": {
  "ANTHROPIC_DEFAULT_HAIKU_MODEL": "old-haiku",
  "ANTHROPIC_DEFAULT_SONNET_MODEL": "old-sonnet",
  "ANTHROPIC_DEFAULT_OPUS_MODEL": "old-opus",
  "OTHER": "keep"
}}"#,
        )
        .unwrap();

        let mut cfg = sample_config();
        let preset = &mut cfg.agents.claude.as_mut().unwrap().presets[0];
        preset.haiku_model = None;
        preset.sonnet_model = None;
        preset.opus_model = None;
        let r = apply_claude(&home, &cfg);
        assert!(r.ok, "errors: {:?}", r.errors);

        let after: Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".claude/settings.json")).unwrap(),
        )
        .unwrap();
        assert!(
            after["env"].get("ANTHROPIC_DEFAULT_HAIKU_MODEL").is_none(),
            "stale haiku key must be deleted"
        );
        assert!(
            after["env"].get("ANTHROPIC_DEFAULT_SONNET_MODEL").is_none(),
            "stale sonnet key must be deleted"
        );
        assert!(
            after["env"].get("ANTHROPIC_DEFAULT_OPUS_MODEL").is_none(),
            "stale opus key must be deleted"
        );
        // Unrelated env preserved.
        assert_eq!(after["env"]["OTHER"], "keep");
    }
}
