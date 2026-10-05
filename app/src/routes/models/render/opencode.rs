// opencode renderer: writes ~/.config/opencode/opencode.jsonc (incremental).
//
// opencode is an incremental agent in cc-switch mode: ALL providers coexist
// in the native config file. The IncrementalAgentConfig's providers map is
// merged into the existing opencode.jsonc (preserving unknown keys and other
// providers), and the top-level `model` pointer is set to
// "<defaultProvider>/<defaultModel>".
//
// Read via json5 (tolerant of comments — they're lost on rewrite, documented
// in design; backups preserve them). Write as pretty JSON.

use std::path::Path;

use serde_json::{json, Value};
use aio_models::incremental::{IncrementalAgentConfig, ProviderNode};

use crate::routes::models::render::common::{backup_write_verify_json, ApplyResult};

/// Apply the opencode incremental config to ~/.config/opencode/opencode.jsonc.
pub fn apply_opencode(home: &Path, config: &IncrementalAgentConfig) -> ApplyResult {
    let mut result = ApplyResult::new();
    let path = home.join(".config/opencode/opencode.jsonc");

    // Read via json5 (tolerant of comments / trailing commas / unquoted keys).
    let mut root: Value = match read_opencode_root(&path) {
        Ok(Some(v)) => v,
        Ok(None) => json!({}),
        Err(msg) => {
            result.push_err(path, msg);
            return result;
        }
    };

    let obj = root
        .as_object_mut()
        .expect("parsed root is guaranteed object");

    // Merge providers: replace provider.<id> fragments from the config,
    // preserving other providers and unknown keys.
    let providers = obj.entry("provider").or_insert_with(|| json!({}));
    if !providers.is_object() {
        *providers = json!({});
    }

    // Build the new provider fragments from the config.
    let mut new_providers = serde_json::Map::new();
    for (id, node) in &config.providers {
        new_providers.insert(id.clone(), render_opencode_provider(node));
    }
    *providers = Value::Object(new_providers);

    // Top-level model = "<defaultProvider>/<defaultModel>"
    match (&config.default_provider, &config.default_model) {
        (Some(p), Some(m)) => {
            obj.insert("model".into(), json!(format!("{p}/{m}")));
        }
        _ => {
            obj.remove("model");
        }
    }

    let bytes = serde_json::to_vec_pretty(&root).expect("serialize opencode.jsonc");
    match backup_write_verify_json(&path, &bytes, 0o600) {
        Ok(backup) => result.push_ok(path, backup),
        Err(msg) => result.push_err(path, msg),
    }

    result
}

/// Render a ProviderNode to opencode-native JSON shape.
fn render_opencode_provider(node: &ProviderNode) -> Value {
    let npm = if node.api == "anthropic-messages" {
        "@ai-sdk/anthropic"
    } else {
        "@ai-sdk/openai-compatible"
    };

    let mut options = serde_json::Map::new();
    options.insert("baseURL".into(), json!(node.base_url));
    if let Some(key) = &node.api_key {
        if !key.is_empty() {
            options.insert("apiKey".into(), json!(key));
        }
    }
    if !node.headers.is_empty() {
        options.insert("headers".into(), json!(node.headers));
    }

    let mut models = serde_json::Map::new();
    for m in &node.models {
        let name = m.name.as_deref().filter(|s| !s.is_empty()).unwrap_or(&m.id);
        models.insert(m.id.clone(), json!({ "name": name }));
    }

    let mut fragment = serde_json::Map::new();
    fragment.insert("npm".into(), json!(npm));
    if !node.name.is_empty() {
        fragment.insert("name".into(), json!(node.name));
    }
    fragment.insert("options".into(), Value::Object(options));
    fragment.insert("models".into(), Value::Object(models));

    Value::Object(fragment)
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

/// Read opencode.jsonc via json5.
fn read_opencode_root(path: &Path) -> Result<Option<Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(None),
        Ok(text) => match json5::from_str::<Value>(&text) {
            Ok(v) if v.is_object() => Ok(Some(v)),
            Ok(v) => Err(format!("corrupt: expected object, got {}", v_type(&v))),
            Err(e) => Err(format!("corrupt, not overwriting: {e}")),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("read: {e}")),
    }
}

// ── tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use aio_models::incremental::{IncrementalAgentConfig, IncrementalModelEntry, ProviderNode};
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp_home() -> std::path::PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut p = std::env::temp_dir();
        p.push(format!("aio-opencode-cc-{}-{}", std::process::id(), n));
        std::fs::create_dir_all(p.join(".config/opencode")).unwrap();
        p
    }

    fn sample_node(name: &str) -> ProviderNode {
        ProviderNode {
            name: name.into(),
            base_url: "https://api.example.com/v1".into(),
            api: "openai-completions".into(),
            api_key: Some("sk-test".into()),
            headers: BTreeMap::new(),
            compat: None,
            models: vec![
                IncrementalModelEntry {
                    id: "model-a".into(),
                    name: Some("Model A".into()),
                    reasoning: None,
                    context_window: None,
                    max_tokens: None,
                    cost: None,
                },
                IncrementalModelEntry {
                    id: "model-b".into(),
                    name: None,
                    reasoning: None,
                    context_window: None,
                    max_tokens: None,
                    cost: None,
                },
            ],
        }
    }

    fn sample_config() -> IncrementalAgentConfig {
        let mut config = IncrementalAgentConfig::default();
        config.providers.insert("prov-a".into(), sample_node("Prov A"));
        config.default_provider = Some("prov-a".into());
        config.default_model = Some("model-a".into());
        config
    }

    #[test]
    fn apply_opencode_writes_config() {
        let home = temp_home();
        let r = apply_opencode(&home, &sample_config());
        assert!(r.ok, "errors: {:?}", r.errors);

        let after: Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".config/opencode/opencode.jsonc")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            after["provider"]["prov-a"]["npm"],
            "@ai-sdk/openai-compatible"
        );
        assert_eq!(
            after["provider"]["prov-a"]["options"]["baseURL"],
            "https://api.example.com/v1"
        );
        assert_eq!(
            after["provider"]["prov-a"]["options"]["apiKey"],
            "sk-test"
        );
        assert_eq!(
            after["provider"]["prov-a"]["models"]["model-a"]["name"],
            "Model A"
        );
        assert_eq!(
            after["provider"]["prov-a"]["models"]["model-b"]["name"],
            "model-b"
        );
        assert_eq!(after["model"], "prov-a/model-a");
    }

    #[test]
    fn apply_opencode_preserves_unknown_keys() {
        let home = temp_home();
        std::fs::write(
            home.join(".config/opencode/opencode.jsonc"),
            r#"{"theme": "dark", "$schema": "https://opencode.ai/schema.json"}"#,
        )
        .unwrap();

        let r = apply_opencode(&home, &sample_config());
        assert!(r.ok);

        let after: Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".config/opencode/opencode.jsonc")).unwrap(),
        )
        .unwrap();
        assert_eq!(after["theme"], "dark");
        assert_eq!(after["$schema"], "https://opencode.ai/schema.json");
    }

    #[test]
    fn apply_opencode_anthropic_protocol_uses_anthropic_npm() {
        let home = temp_home();
        let mut config = IncrementalAgentConfig::default();
        let mut node = sample_node("Anthropic");
        node.api = "anthropic-messages".into();
        config.providers.insert("antp".into(), node);
        config.default_provider = Some("antp".into());
        config.default_model = Some("model-a".into());

        let r = apply_opencode(&home, &config);
        assert!(r.ok);

        let after: Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".config/opencode/opencode.jsonc")).unwrap(),
        )
        .unwrap();
        assert_eq!(after["provider"]["antp"]["npm"], "@ai-sdk/anthropic");
    }

    #[test]
    fn apply_opencode_no_default_removes_model_key() {
        let home = temp_home();
        std::fs::write(
            home.join(".config/opencode/opencode.jsonc"),
            r#"{"model": "old/old-model"}"#,
        )
        .unwrap();

        let mut config = sample_config();
        config.default_provider = None;
        config.default_model = None;

        let r = apply_opencode(&home, &config);
        assert!(r.ok);

        let after: Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".config/opencode/opencode.jsonc")).unwrap(),
        )
        .unwrap();
        assert!(after.get("model").is_none());
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
            "{{not even json5"
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
        assert_eq!(mode, 0o600);
    }
}
