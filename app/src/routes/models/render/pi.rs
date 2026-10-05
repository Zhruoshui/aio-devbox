// pi renderer: writes ~/.pi/agent/models.json (incremental, read-modify-merge)
// and ~/.pi/agent/settings.json (defaultProvider / defaultModel).
//
// pi is an incremental agent in cc-switch mode: ALL providers coexist in the
// native config file. "Switching" means changing the default provider/model
// pointer. The IncrementalAgentConfig's providers map is merged into the
// existing models.json (preserving unknown keys and other providers), and
// defaultProvider/defaultModel are set in settings.json.
//
// A corrupt target file aborts that file's write but lets the other file
// proceed.

use std::path::Path;

use serde_json::{json, Value};
use aio_models::incremental::{IncrementalAgentConfig, IncrementalModelEntry, ProviderNode};

use crate::routes::models::render::common::{
    backup_write_verify_json, read_json_object, ApplyResult, ReadError,
};

/// Apply the pi incremental config to ~/.pi/agent/.
pub fn apply_pi(home: &Path, config: &IncrementalAgentConfig) -> ApplyResult {
    let mut result = ApplyResult::new();

    write_pi_models(home, config, &mut result);
    write_pi_settings(home, config, &mut result);

    result
}

/// ~/.pi/agent/models.json: read (missing => {}); merge `providers` from the
/// config into the existing providers map, preserving every other provider
/// node and unknown keys at every level. Backup first; atomic write 0600.
fn write_pi_models(home: &Path, config: &IncrementalAgentConfig, result: &mut ApplyResult) {
    let path = home.join(".pi/agent/models.json");

    let mut root: Value = match read_json_object(&path) {
        Ok(Some(v)) => v,
        Ok(None) => json!({}),
        Err(ReadError::Corrupt(e)) => {
            result.push_err(path, format!("corrupt, not overwriting: {e}"));
            return;
        }
        Err(ReadError::Io(e)) => {
            result.push_err(path, format!("read: {e}"));
            return;
        }
    };

    // Merge: replace providers map with the config's providers (rendered to
    // pi-native shape), preserving unknown top-level keys.
    let providers = root
        .as_object_mut()
        .expect("read_json_object guarantees object")
        .entry("providers")
        .or_insert_with(|| json!({}));
    if !providers.is_object() {
        *providers = json!({});
    }

    // Build the new providers object from the config.
    let mut new_providers = serde_json::Map::new();
    for (id, node) in &config.providers {
        new_providers.insert(id.clone(), render_pi_provider(node));
    }
    *providers = Value::Object(new_providers);

    let bytes = serde_json::to_vec_pretty(&root).expect("serialize pi models.json");
    match backup_write_verify_json(&path, &bytes, 0o600) {
        Ok(backup) => result.push_ok(path, backup),
        Err(msg) => result.push_err(path, msg),
    }
}

/// ~/.pi/agent/settings.json: set defaultProvider + defaultModel, preserve
/// everything else. Backup first; atomic write 0600.
fn write_pi_settings(home: &Path, config: &IncrementalAgentConfig, result: &mut ApplyResult) {
    let path = home.join(".pi/agent/settings.json");

    let mut root: Value = match read_json_object(&path) {
        Ok(Some(v)) => v,
        Ok(None) => json!({}),
        Err(ReadError::Corrupt(e)) => {
            result.push_err(path, format!("corrupt, not overwriting: {e}"));
            return;
        }
        Err(ReadError::Io(e)) => {
            result.push_err(path, format!("read: {e}"));
            return;
        }
    };

    let obj = root
        .as_object_mut()
        .expect("read_json_object guarantees object");

    // Set or remove defaultProvider/defaultModel.
    match &config.default_provider {
        Some(p) => {
            obj.insert("defaultProvider".into(), json!(p));
        }
        None => {
            obj.remove("defaultProvider");
        }
    }
    match &config.default_model {
        Some(m) => {
            obj.insert("defaultModel".into(), json!(m));
        }
        None => {
            obj.remove("defaultModel");
        }
    }

    let bytes = serde_json::to_vec_pretty(&root).expect("serialize pi settings.json");
    match backup_write_verify_json(&path, &bytes, 0o600) {
        Ok(backup) => result.push_ok(path, backup),
        Err(msg) => result.push_err(path, msg),
    }
}

/// Render a ProviderNode to pi-native JSON shape (baseUrl/api/apiKey/headers/
/// compat/models+cost). Empty/null fields are omitted.
fn render_pi_provider(node: &ProviderNode) -> Value {
    let mut p = serde_json::Map::new();

    if !node.name.is_empty() {
        p.insert("name".into(), json!(node.name));
    }
    if !node.base_url.is_empty() {
        p.insert("baseUrl".into(), json!(node.base_url));
    }
    if !node.api.is_empty() {
        p.insert("api".into(), json!(node.api));
    }
    if let Some(key) = &node.api_key {
        if !key.is_empty() {
            p.insert("apiKey".into(), json!(key));
        }
    }
    if !node.headers.is_empty() {
        p.insert("headers".into(), json!(node.headers));
    }
    if let Some(compat) = &node.compat {
        if !compat.is_null() {
            p.insert("compat".into(), compat.clone());
        }
    }

    let models: Vec<Value> = node.models.iter().map(render_pi_model).collect();
    p.insert("models".into(), Value::Array(models));

    Value::Object(p)
}

/// Render an IncrementalModelEntry to pi-native JSON.
fn render_pi_model(m: &IncrementalModelEntry) -> Value {
    let mut o = serde_json::Map::new();

    o.insert("id".into(), json!(m.id));

    if let Some(name) = &m.name {
        if !name.is_empty() {
            o.insert("name".into(), json!(name));
        }
    }
    if let Some(reasoning) = m.reasoning {
        if reasoning {
            o.insert("reasoning".into(), json!(true));
        }
    }
    if let Some(cw) = m.context_window {
        o.insert("contextWindow".into(), json!(cw));
    }
    if let Some(mt) = m.max_tokens {
        o.insert("maxTokens".into(), json!(mt));
    }
    if let Some(cost) = &m.cost {
        let c = render_pi_cost(cost);
        if !c.is_empty() {
            o.insert("cost".into(), Value::Object(c));
        }
    }

    Value::Object(o)
}

/// Render cost: pi's ModelCostSchema requires all four fields whenever cost
/// is present; fill missing fields with 0.
fn render_pi_cost(cost: &aio_models::incremental::IncrementalCost) -> serde_json::Map<String, Value> {
    let mut c = serde_json::Map::new();
    if let Some(v) = cost.input {
        c.insert("input".into(), json!(v));
    }
    if let Some(v) = cost.output {
        c.insert("output".into(), json!(v));
    }
    if let Some(v) = cost.cache_read {
        c.insert("cacheRead".into(), json!(v));
    }
    if let Some(v) = cost.cache_write {
        c.insert("cacheWrite".into(), json!(v));
    }
    if !c.is_empty() {
        for key in ["input", "output", "cacheRead", "cacheWrite"] {
            c.entry(key.to_string()).or_insert(json!(0));
        }
    }
    c
}

// ── tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use aio_models::incremental::{IncrementalAgentConfig, IncrementalCost, IncrementalModelEntry, ProviderNode};
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp_home() -> std::path::PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut p = std::env::temp_dir();
        p.push(format!("aio-pi-cc-{}-{}", std::process::id(), n));
        std::fs::create_dir_all(p.join(".pi/agent")).unwrap();
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
            models: vec![IncrementalModelEntry {
                id: "model-a".into(),
                name: Some("Model A".into()),
                reasoning: Some(true),
                context_window: Some(128_000),
                max_tokens: Some(8_192),
                cost: Some(IncrementalCost {
                    input: Some(0.5),
                    output: Some(2.0),
                    cache_read: None,
                    cache_write: None,
                }),
            }],
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
    fn apply_pi_writes_models_and_settings() {
        let home = temp_home();
        let r = apply_pi(&home, &sample_config());
        assert!(r.ok, "errors: {:?}", r.errors);
        assert_eq!(r.written.len(), 2);

        // models.json
        let models: Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".pi/agent/models.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(models["providers"]["prov-a"]["baseUrl"], "https://api.example.com/v1");
        assert_eq!(models["providers"]["prov-a"]["apiKey"], "sk-test");
        assert_eq!(models["providers"]["prov-a"]["models"][0]["id"], "model-a");
        assert_eq!(models["providers"]["prov-a"]["models"][0]["cost"]["input"], 0.5);
        // cacheWrite filled with 0.
        assert_eq!(models["providers"]["prov-a"]["models"][0]["cost"]["cacheWrite"], 0);

        // settings.json
        let settings: Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".pi/agent/settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(settings["defaultProvider"], "prov-a");
        assert_eq!(settings["defaultModel"], "model-a");
    }

    #[test]
    fn apply_pi_preserves_unknown_top_level_keys() {
        let home = temp_home();
        std::fs::write(
            home.join(".pi/agent/models.json"),
            r#"{"version": 1, "unknownKey": 42, "providers": {}}"#,
        )
        .unwrap();

        let r = apply_pi(&home, &sample_config());
        assert!(r.ok);

        let models: Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".pi/agent/models.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(models["unknownKey"], 42);
        assert_eq!(models["version"], 1);
    }

    #[test]
    fn apply_pi_replaces_providers_map() {
        let home = temp_home();
        // Existing models.json with an old provider that is NOT in the new config.
        std::fs::write(
            home.join(".pi/agent/models.json"),
            r#"{"providers": {"old-prov": {"baseUrl": "https://old.example.com"}}}"#,
        )
        .unwrap();

        let r = apply_pi(&home, &sample_config());
        assert!(r.ok);

        let models: Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".pi/agent/models.json")).unwrap(),
        )
        .unwrap();
        // Old provider gone (replaced by config's providers).
        assert!(models["providers"].get("old-prov").is_none());
        assert!(models["providers"].get("prov-a").is_some());
    }

    #[test]
    fn apply_pi_settings_preserves_extra_keys() {
        let home = temp_home();
        std::fs::write(
            home.join(".pi/agent/settings.json"),
            r#"{"extra": "keep"}"#,
        )
        .unwrap();

        let r = apply_pi(&home, &sample_config());
        assert!(r.ok);

        let settings: Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".pi/agent/settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(settings["defaultProvider"], "prov-a");
        assert_eq!(settings["extra"], "keep");
    }

    #[test]
    fn apply_pi_no_default_clears_keys() {
        let home = temp_home();
        std::fs::write(
            home.join(".pi/agent/settings.json"),
            r#"{"defaultProvider": "old", "defaultModel": "old-model", "extra": "keep"}"#,
        )
        .unwrap();

        let mut config = sample_config();
        config.default_provider = None;
        config.default_model = None;

        let r = apply_pi(&home, &config);
        assert!(r.ok);

        let settings: Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".pi/agent/settings.json")).unwrap(),
        )
        .unwrap();
        assert!(settings.get("defaultProvider").is_none());
        assert!(settings.get("defaultModel").is_none());
        assert_eq!(settings["extra"], "keep");
    }

    #[test]
    fn apply_pi_corrupt_models_aborts_models_but_writes_settings() {
        let home = temp_home();
        std::fs::write(home.join(".pi/agent/models.json"), "{{not json").unwrap();

        let r = apply_pi(&home, &sample_config());
        assert!(!r.ok);
        assert!(r.errors.iter().any(|e| e.path.contains("models.json")));
        assert_eq!(
            std::fs::read_to_string(home.join(".pi/agent/models.json")).unwrap(),
            "{{not json"
        );
        // settings.json still written.
        assert!(r.written.iter().any(|w| w.path.contains("settings.json")));
    }

    #[test]
    fn apply_pi_creates_files_when_missing() {
        let home = temp_home();
        let r = apply_pi(&home, &sample_config());
        assert!(r.ok);
        use std::os::unix::fs::PermissionsExt;
        let m = std::fs::metadata(home.join(".pi/agent/models.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(m, 0o600);
        let s = std::fs::metadata(home.join(".pi/agent/settings.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(s, 0o600);
    }
}
