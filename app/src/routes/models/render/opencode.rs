// opencode renderer: writes ~/.config/opencode/opencode.jsonc.
//
// Read via json5 (tolerant of comments — they're lost on rewrite, documented
// in design §9.6; backups preserve them). Merge provider fragment under
// `provider[<id>]` and set top-level `model = "<providerId>/<modelId>"`,
// preserving every other key (including other providers, theme, mcp, etc).
// Write as pretty JSON (JSONC comments lost — design §9.6).

use std::path::Path;

use serde_json::{json, Value};

use crate::routes::models::render::common::{backup_write_verify_json, ApplyResult, ProviderPatch};
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

/// Read opencode.jsonc via json5 (tolerant of comments / trailing commas /
/// unquoted keys). Ok(None) = missing/empty file; Err = corrupt (non-object
/// root or parse failure) or io error. Callers decide what "missing" means
/// (apply starts fresh {}; live edit/delete has nothing to work on).
fn read_opencode_root(path: &Path) -> Result<Option<Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(None),
        Ok(text) => match json5::from_str::<Value>(&text) {
            Ok(v) if v.is_object() => Ok(Some(v)),
            // Non-object JSON is invalid for an opencode config — treat as
            // corrupt so we don't clobber whatever the user had.
            Ok(v) => Err(format!("corrupt: expected object, got {}", v_type(&v))),
            Err(e) => Err(format!("corrupt, not overwriting: {e}")),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("read: {e}")),
    }
}

// ── live provider edit / delete (agent-tab live management) ────────

/// Field-level edit of one provider fragment in opencode.jsonc
/// (08-27-agent-tabs-live-config design §2). Patched keys map onto the
/// fragment's native fields: baseUrl → options.baseURL, apiKey →
/// options.apiKey ("" clears), api → the npm package (inverse of the
/// renderer's protocol→npm choice), name → fragment name. models and every
/// other key (other providers, $schema, theme, mcp, …) are preserved.
pub fn edit_opencode_provider(
    home: &Path,
    provider_id: &str,
    patch: &ProviderPatch,
) -> ApplyResult {
    let mut result = ApplyResult::new();
    let path = home.join(".config/opencode/opencode.jsonc");

    let Some(mut root) = read_live_root(&path, &mut result) else {
        return result;
    };

    let frag = {
        let obj = root
            .as_object_mut()
            .expect("read_opencode_root guarantees object");
        match obj
            .get_mut("provider")
            .and_then(|p| p.as_object_mut())
            .and_then(|p| p.get_mut(provider_id))
        {
            Some(f) if f.is_object() => f,
            _ => {
                result.push_err(path, format!("provider '{provider_id}' not found"));
                return result;
            }
        }
    };

    let frag_obj = frag.as_object_mut().expect("checked is_object");
    if let Some(v) = &patch.name {
        frag_obj.insert("name".into(), json!(v));
    }
    if let Some(v) = &patch.base_url {
        let options = frag_obj.entry("options").or_insert_with(|| json!({}));
        if !options.is_object() {
            *options = json!({});
        }
        options
            .as_object_mut()
            .unwrap()
            .insert("baseURL".into(), json!(v));
    }
    if let Some(v) = &patch.api_key {
        if v.is_empty() {
            // "" clears the key (same contract as the canonical masking rules).
            if let Some(o) = frag_obj.get_mut("options").and_then(|o| o.as_object_mut()) {
                o.remove("apiKey");
            }
        } else {
            let options = frag_obj.entry("options").or_insert_with(|| json!({}));
            if !options.is_object() {
                *options = json!({});
            }
            options
                .as_object_mut()
                .unwrap()
                .insert("apiKey".into(), json!(v));
        }
    }
    if let Some(v) = &patch.api {
        // Inverse of the renderer's npm choice — editing the protocol on the
        // opencode side means switching the SDK package.
        let npm = if v == "anthropic-messages" {
            "@ai-sdk/anthropic"
        } else {
            "@ai-sdk/openai-compatible"
        };
        frag_obj.insert("npm".into(), json!(npm));
    }

    let bytes = serde_json::to_vec_pretty(&root).expect("serialize opencode.jsonc");
    match backup_write_verify_json(&path, &bytes, 0o600) {
        Ok(backup) => result.push_ok(path, backup),
        Err(msg) => result.push_err(path, msg),
    }
    result
}

/// Remove one provider fragment from opencode.jsonc. When the top-level
/// `model` points at it ("<id>/…") the dangling `model` key is removed too.
/// Design §2.
pub fn delete_opencode_provider(home: &Path, provider_id: &str) -> ApplyResult {
    let mut result = ApplyResult::new();
    let path = home.join(".config/opencode/opencode.jsonc");

    let Some(mut root) = read_live_root(&path, &mut result) else {
        return result;
    };

    let obj = root
        .as_object_mut()
        .expect("read_opencode_root guarantees object");
    let removed = obj
        .get_mut("provider")
        .and_then(|p| p.as_object_mut())
        .map(|p| p.remove(provider_id).is_some())
        .unwrap_or(false);
    if !removed {
        result.push_err(path, format!("provider '{provider_id}' not found"));
        return result;
    }

    // Dangling-default cleanup: top-level model = "<id>/<model>".
    let prefix = format!("{provider_id}/");
    if let Some(m) = obj.get("model").and_then(|m| m.as_str()) {
        if m.starts_with(&prefix) {
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

/// Read the opencode config for a live edit/delete: a missing file and a
/// corrupt file are both errors (nothing to work on), reported via `result`.
/// Returns None when the caller should stop.
fn read_live_root(path: &Path, result: &mut ApplyResult) -> Option<Value> {
    match read_opencode_root(path) {
        Ok(Some(v)) => Some(v),
        Ok(None) => {
            result.push_err(path.to_path_buf(), "opencode.jsonc not found".into());
            None
        }
        Err(msg) => {
            result.push_err(path.to_path_buf(), msg);
            None
        }
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

    // --- live provider edit / delete (agent-tab live management) ---

    fn seed_live_jsonc(home: &std::path::Path) {
        // json5 fixture: comments + trailing commas, as a hand-maintained
        // opencode.jsonc would look.
        std::fs::write(
            home.join(".config/opencode/opencode.jsonc"),
            r#"{
  // user-maintained config
  "$schema": "https://opencode.ai/schema.json",
  "model": "prov-a/model-a",
  "provider": {
    "prov-a": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "Prov A",
      "options": {
        "baseURL": "https://a.example/v1",
        "apiKey": "sk-old-key-xxxx",
      },
      "models": { "model-a": { "name": "Model A" } },
    },
    "prov-b": { "options": { "baseURL": "https://b.example/v1" } },
  },
  "theme": "dark",
}"#,
        )
        .unwrap();
    }

    fn read_jsonc(home: &std::path::Path) -> Value {
        let text = std::fs::read_to_string(home.join(".config/opencode/opencode.jsonc")).unwrap();
        serde_json::from_str(&text).expect("written back as strict pretty JSON")
    }

    #[test]
    fn edit_opencode_provider_on_json5_comment_file() {
        let home = temp_home();
        seed_live_jsonc(&home);

        let patch = crate::routes::models::render::ProviderPatch {
            name: Some("Renamed A".into()),
            base_url: Some("https://new.example/v1".into()),
            api: Some("anthropic-messages".into()),
            api_key: Some("sk-new-key-xxxx".into()),
        };
        let r = edit_opencode_provider(&home, "prov-a", &patch);
        assert!(r.ok, "errors: {:?}", r.errors);

        let after = read_jsonc(&home);
        // Patched fields applied (api patch flips the npm package).
        assert_eq!(after["provider"]["prov-a"]["name"], "Renamed A");
        assert_eq!(after["provider"]["prov-a"]["npm"], "@ai-sdk/anthropic");
        assert_eq!(after["provider"]["prov-a"]["options"]["baseURL"], "https://new.example/v1");
        assert_eq!(after["provider"]["prov-a"]["options"]["apiKey"], "sk-new-key-xxxx");
        // Fragment's models preserved verbatim.
        assert_eq!(after["provider"]["prov-a"]["models"]["model-a"]["name"], "Model A");
        // Sibling provider + unrelated keys preserved; top-level model kept.
        assert_eq!(after["provider"]["prov-b"]["options"]["baseURL"], "https://b.example/v1");
        assert_eq!(after["$schema"], "https://opencode.ai/schema.json");
        assert_eq!(after["theme"], "dark");
        assert_eq!(after["model"], "prov-a/model-a");
    }

    #[test]
    fn edit_opencode_provider_empty_api_key_clears() {
        let home = temp_home();
        seed_live_jsonc(&home);

        let patch = crate::routes::models::render::ProviderPatch {
            api_key: Some(String::new()),
            ..Default::default()
        };
        let r = edit_opencode_provider(&home, "prov-a", &patch);
        assert!(r.ok);
        let after = read_jsonc(&home);
        assert!(after["provider"]["prov-a"]["options"].get("apiKey").is_none());
        // baseURL survives the clear.
        assert_eq!(after["provider"]["prov-a"]["options"]["baseURL"], "https://a.example/v1");
    }

    #[test]
    fn edit_opencode_provider_missing_node_errors_and_leaves_file() {
        let home = temp_home();
        seed_live_jsonc(&home);
        let before = std::fs::read_to_string(home.join(".config/opencode/opencode.jsonc")).unwrap();

        let patch = crate::routes::models::render::ProviderPatch {
            base_url: Some("https://x".into()),
            ..Default::default()
        };
        let r = edit_opencode_provider(&home, "nope", &patch);
        assert!(!r.ok);
        assert!(r.errors[0].message.contains("not found"));
        assert_eq!(
            std::fs::read_to_string(home.join(".config/opencode/opencode.jsonc")).unwrap(),
            before,
            "file untouched on error"
        );
    }

    #[test]
    fn edit_opencode_provider_missing_file_errors() {
        let home = temp_home();
        let patch = crate::routes::models::render::ProviderPatch::default();
        let r = edit_opencode_provider(&home, "prov-a", &patch);
        assert!(!r.ok);
        assert!(r.errors[0].message.contains("not found"));
    }

    #[test]
    fn delete_opencode_provider_clears_dangling_model() {
        let home = temp_home();
        seed_live_jsonc(&home);

        let r = delete_opencode_provider(&home, "prov-a");
        assert!(r.ok, "errors: {:?}", r.errors);

        let after = read_jsonc(&home);
        assert!(after["provider"].get("prov-a").is_none());
        assert!(after["provider"].get("prov-b").is_some(), "sibling kept");
        // Dangling top-level model removed; unrelated keys kept.
        assert!(after.get("model").is_none());
        assert_eq!(after["theme"], "dark");
        assert_eq!(after["$schema"], "https://opencode.ai/schema.json");
    }

    #[test]
    fn delete_opencode_provider_keeps_model_of_other_provider() {
        let home = temp_home();
        seed_live_jsonc(&home);

        // model points at prov-a, deleting prov-b must keep it.
        let r = delete_opencode_provider(&home, "prov-b");
        assert!(r.ok);
        let after = read_jsonc(&home);
        assert!(after["provider"].get("prov-b").is_none());
        assert_eq!(after["model"], "prov-a/model-a");
    }

    #[test]
    fn delete_opencode_provider_missing_node_errors() {
        let home = temp_home();
        seed_live_jsonc(&home);
        let r = delete_opencode_provider(&home, "nope");
        assert!(!r.ok);
        assert!(r.errors[0].message.contains("not found"));
    }
}
