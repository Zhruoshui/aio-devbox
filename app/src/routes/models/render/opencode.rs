// opencode renderer: writes ~/.config/opencode/opencode.jsonc.
//
// Read via json5 (tolerant of comments — they're lost on rewrite, documented
// in design §9.6; backups preserve them). Merge provider fragment under
// `provider[<id>]` and set top-level `model = "<providerId>/<modelId>"`,
// preserving every other key (including other providers, theme, mcp, etc).
// Write as pretty JSON (JSONC comments lost — design §9.6).

use std::path::Path;

use serde_json::{json, Value};

use crate::routes::models::render::common::{backup_write_verify_json, ApplyResult};
use crate::routes::models::store::CanonicalConfig;

/// Apply the opencode assignment to ~/.config/opencode/opencode.jsonc.
pub fn apply_opencode(home: &Path, canonical: &CanonicalConfig) -> ApplyResult {
    let mut result = ApplyResult::new();

    let Some(assignment) = &canonical.agents.opencode else {
        result.push_err(
            home.join(".config/opencode/opencode.jsonc"),
            "no opencode assignment".into(),
        );
        return result;
    };
    let Some(provider) = canonical.providers.get(&assignment.provider) else {
        result.push_err(
            home.join(".config/opencode/opencode.jsonc"),
            format!("provider '{}' not found", assignment.provider),
        );
        return result;
    };

    let path = home.join(".config/opencode/opencode.jsonc");

    // Read via json5 (tolerant of comments / trailing commas / unquoted keys).
    let mut root: Value = match std::fs::read_to_string(&path) {
        Ok(text) if text.trim().is_empty() => json!({}),
        Ok(text) => match json5::from_str::<Value>(&text) {
            Ok(v) if v.is_object() => v,
            Ok(v) => {
                // Non-object JSON is invalid for an opencode config — treat as
                // corrupt so we don't clobber whatever the user had.
                result.push_err(
                    path.clone(),
                    format!("corrupt: expected object, got {}", v_type(&v)),
                );
                return result;
            }
            Err(e) => {
                result.push_err(path.clone(), format!("corrupt, not overwriting: {e}"));
                return result;
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => {
            result.push_err(path.clone(), format!("read: {e}"));
            return result;
        }
    };

    let obj = root
        .as_object_mut()
        .expect("parsed root is guaranteed object");

    // provider.<id> = {npm, name, options:{baseURL, apiKey}, models:{<id>:{name}}}
    let npm = if provider.api == "anthropic-messages" {
        "@ai-sdk/anthropic"
    } else {
        "@ai-sdk/openai-compatible"
    };

    let mut options = serde_json::Map::new();
    options.insert("baseURL".into(), json!(provider.base_url));
    if let Some(key) = &provider.api_key {
        if !key.is_empty() {
            options.insert("apiKey".into(), json!(key));
        }
    }
    // Carry provider.headers into options.headers when non-empty (task spec:
    // "also carry headers if provider.headers non-empty into options.headers").
    if !provider.headers.is_empty() {
        options.insert("headers".into(), json!(provider.headers));
    }

    let mut models = serde_json::Map::new();
    for m in &provider.models {
        let name = m
            .name
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&m.id);
        models.insert(m.id.clone(), json!({ "name": name }));
    }

    let provider_fragment = json!({
        "npm": npm,
        "name": provider.name,
        "options": Value::Object(options),
        "models": Value::Object(models),
    });

    let providers = obj
        .entry("provider")
        .or_insert_with(|| json!({}));
    if !providers.is_object() {
        *providers = json!({});
    }
    providers
        .as_object_mut()
        .unwrap()
        .insert(assignment.provider.clone(), provider_fragment);

    // Top-level model = "<providerId>/<modelId>"
    obj.insert(
        "model".into(),
        json!(format!("{}/{}", assignment.provider, assignment.model)),
    );

    let bytes = serde_json::to_vec_pretty(&root).expect("serialize opencode.jsonc");
    match backup_write_verify_json(&path, &bytes, 0o600) {
        Ok(backup) => result.push_ok(path, backup),
        Err(msg) => result.push_err(path, msg),
    }

    result
}

fn v_type(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

// ── tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::models::store::{
        AgentAssignment, CanonicalConfig, ModelEntry, ProviderEntry,
    };
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp_home() -> std::path::PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut p = std::env::temp_dir();
        p.push(format!("aio-opencode-{}-{}", std::process::id(), n));
        std::fs::create_dir_all(p.join(".config/opencode")).unwrap();
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
                models: vec![
                    ModelEntry {
                        id: "deepseek-v4-pro".into(),
                        name: Some("DeepSeek V4 Pro".into()),
                        ..Default::default()
                    },
                    ModelEntry {
                        id: "qwen-max".into(),
                        name: None,
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
        );
        c.agents.opencode = Some(AgentAssignment {
            provider: "aruoshui".into(),
            model: "deepseek-v4-pro".into(),
        });
        c
    }

    #[test]
    fn apply_opencode_merges_and_preserves_user_keys() {
        let home = temp_home();
        // Existing config with $schema, theme, another provider, comments.
        // json5 parses away the comments (lost on rewrite — design §9.6).
        std::fs::write(
            home.join(".config/opencode/opencode.jsonc"),
            r#"{
  // top-level comment — will be lost on rewrite
  "$schema": "https://opencode.ai/schema.json",
  "theme": "dark",
  "provider": {
    "other": {
      "npm": "@ai-sdk/openai",
      "name": "Other",
      "options": { "baseURL": "https://other.example/v1" }
    }
  },
  "mcp": { "foo": { "type": "local", "command": ["echo"] } }
}"#,
        )
        .unwrap();

        let r = apply_opencode(&home, &sample_config());
        assert!(r.ok, "errors: {:?}", r.errors);
        assert_eq!(r.written.len(), 1);

        let after: Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".config/opencode/opencode.jsonc")).unwrap(),
        )
        .unwrap();

        // User keys preserved.
        assert_eq!(after["$schema"], "https://opencode.ai/schema.json");
        assert_eq!(after["theme"], "dark");
        assert_eq!(after["mcp"]["foo"]["type"], "local");

        // Existing provider preserved.
        assert_eq!(after["provider"]["other"]["name"], "Other");

        // New provider merged in.
        assert_eq!(
            after["provider"]["aruoshui"]["npm"],
            "@ai-sdk/openai-compatible"
        );
        assert_eq!(
            after["provider"]["aruoshui"]["options"]["baseURL"],
            "https://ai.aruoshui.com/v1"
        );
        assert_eq!(
            after["provider"]["aruoshui"]["options"]["apiKey"],
            "sk-real-key-xxxx"
        );
        assert_eq!(
            after["provider"]["aruoshui"]["models"]["deepseek-v4-pro"]["name"],
            "DeepSeek V4 Pro"
        );
        // Model without display name falls back to id.
        assert_eq!(after["provider"]["aruoshui"]["models"]["qwen-max"]["name"], "qwen-max");

        // Top-level model = "<providerId>/<modelId>".
        assert_eq!(after["model"], "aruoshui/deepseek-v4-pro");
    }

    #[test]
    fn apply_opencode_anthropic_protocol_uses_anthropic_npm() {
        let home = temp_home();
        let mut cfg = CanonicalConfig::default();
        cfg.providers.insert(
            "antp".into(),
            ProviderEntry {
                name: "Ant".into(),
                base_url: "https://api.anthropic.com".into(),
                api: "anthropic-messages".into(),
                api_key: Some("sk-ant-key-xxxx".into()),
                models: vec![ModelEntry {
                    id: "claude-3".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        cfg.agents.opencode = Some(AgentAssignment {
            provider: "antp".into(),
            model: "claude-3".into(),
        });
        let r = apply_opencode(&home, &cfg);
        assert!(r.ok);
        let after: Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".config/opencode/opencode.jsonc")).unwrap(),
        )
        .unwrap();
        assert_eq!(after["provider"]["antp"]["npm"], "@ai-sdk/anthropic");
    }

    #[test]
    fn apply_opencode_omits_empty_api_key() {
        let home = temp_home();
        let mut cfg = sample_config();
        cfg.providers
            .get_mut("aruoshui")
            .unwrap()
            .api_key = Some(String::new());
        let r = apply_opencode(&home, &cfg);
        assert!(r.ok);
        let after: Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".config/opencode/opencode.jsonc")).unwrap(),
        )
        .unwrap();
        assert!(after["provider"]["aruoshui"]["options"].get("apiKey").is_none());
    }

    #[test]
    fn apply_opencode_corrupt_file_aborts() {
        let home = temp_home();
        std::fs::write(
            home.join(".config/opencode/opencode.jsonc"),
            "{{not even json5",
        )
        .unwrap();
        let r = apply_opencode(&home, &sample_config());
        assert!(!r.ok);
        assert!(r.errors[0].message.contains("corrupt"));
        assert_eq!(
            std::fs::read_to_string(home.join(".config/opencode/opencode.jsonc")).unwrap(),
            "{{not even json5",
            "corrupt file untouched"
        );
    }

    #[test]
    fn apply_opencode_creates_when_missing() {
        let home = temp_home();
        let r = apply_opencode(&home, &sample_config());
        assert!(r.ok);
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(home.join(".config/opencode/opencode.jsonc"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600); // 0600: opencode.jsonc embeds the apiKey (design §4)
        assert!(r.written[0].backup.is_none());
    }

    #[test]
    fn apply_opencode_missing_assignment_errors() {
        let home = temp_home();
        let mut cfg = sample_config();
        cfg.agents.opencode = None;
        let r = apply_opencode(&home, &cfg);
        assert!(!r.ok);
        assert!(r.errors[0].message.contains("no opencode assignment"));
    }

    #[test]
    fn jsonc_comments_are_lost_but_backup_preserves_them() {
        let home = temp_home();
        let original = "// my comment\n{\"$schema\":\"x\"}\n";
        std::fs::write(home.join(".config/opencode/opencode.jsonc"), original).unwrap();
        let r = apply_opencode(&home, &sample_config());
        assert!(r.ok);
        let after = std::fs::read_to_string(home.join(".config/opencode/opencode.jsonc")).unwrap();
        // Comment is gone in the rewrite (design §9.6 — acceptable).
        assert!(!after.contains("my comment"));
        // But the backup still holds it.
        let backup = r.written[0].backup.as_ref().unwrap();
        let backup_content = std::fs::read_to_string(backup).unwrap();
        assert!(backup_content.contains("my comment"));
    }
}
