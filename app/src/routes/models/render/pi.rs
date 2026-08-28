// pi renderer: writes ~/.pi/agent/models.json (provider-node merge — other
// providers preserved) and ~/.pi/agent/settings.json (defaultProvider /
// defaultModel keys only, rest preserved). Matches pi-web's native
// models.json shape (research/pi-web §1). A corrupt target file aborts
// that file's write but lets the other file proceed (design §4).

use std::path::Path;

use serde_json::{json, Value};

use crate::routes::models::render::common::{
    backup_write_verify_json, read_json_object, ApplyResult, ProviderPatch, ReadError,
};
use crate::routes::models::store::{
    AgentAssignment, CanonicalConfig, CostEntry, ModelEntry, ProviderEntry,
};

/// Apply the pi assignment to ~/.pi/agent/. `home` is injectable so tests
/// use temp dirs; routes pass `home_dir()`.
pub fn apply_pi(home: &Path, canonical: &CanonicalConfig) -> ApplyResult {
    let mut result = ApplyResult::new();

    let Some(assignment) = &canonical.agents.pi else {
        result.push_err(
            home.join(".pi/agent/models.json"),
            "no pi assignment".into(),
        );
        return result;
    };
    let Some(provider) = canonical.providers.get(&assignment.provider) else {
        result.push_err(
            home.join(".pi/agent/models.json"),
            format!("provider '{}' not found", assignment.provider),
        );
        return result;
    };

    write_pi_models(home, provider, &assignment.provider, &mut result);
    write_pi_settings(home, assignment, &mut result);

    result
}

/// ~/.pi/agent/models.json: read (missing => {}); replace `providers[<id>]`
/// with the pi ProviderEntry shape (baseUrl/api/apiKey/headers/compat/
/// models+cost); preserve every other provider node and any unknown keys
/// at every level. Backup first; atomic write 0600. Corrupt file => abort
/// this file (design §4).
fn write_pi_models(
    home: &Path,
    provider: &ProviderEntry,
    provider_id: &str,
    result: &mut ApplyResult,
) {
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

    // Merge: set providers[<id>] = rendered provider, leave the rest.
    let providers = root
        .as_object_mut()
        .expect("read_json_object guarantees object")
        .entry("providers")
        .or_insert_with(|| json!({}));
    if !providers.is_object() {
        *providers = json!({});
    }
    providers
        .as_object_mut()
        .unwrap()
        .insert(provider_id.to_string(), render_pi_provider(provider));

    let bytes = serde_json::to_vec_pretty(&root).expect("serialize pi models.json");
    match backup_write_verify_json(&path, &bytes, 0o600) {
        Ok(backup) => result.push_ok(path, backup),
        Err(msg) => result.push_err(path, msg),
    }
}

/// ~/.pi/agent/settings.json: set only defaultProvider + defaultModel,
/// preserve everything else. Backup first; atomic write 0600.
fn write_pi_settings(
    home: &Path,
    assignment: &AgentAssignment,
    result: &mut ApplyResult,
) {
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
    obj.insert("defaultProvider".into(), json!(assignment.provider));
    obj.insert("defaultModel".into(), json!(assignment.model));

    let bytes = serde_json::to_vec_pretty(&root).expect("serialize pi settings.json");
    match backup_write_verify_json(&path, &bytes, 0o600) {
        Ok(backup) => result.push_ok(path, backup),
        Err(msg) => result.push_err(path, msg),
    }
}

// ── live provider edit / delete (agent-tab live management) ────────

/// Field-level edit of one provider node in ~/.pi/agent/models.json
/// (08-27-agent-tabs-live-config design §2). Only the patched keys are set
/// on the node — its models/cost/headers and every other provider are
/// preserved. `patch.name` maps to pi's optional provider `name` (display
/// name; schema `minLength: 1`): non-empty writes it, empty string removes
/// the key (writing "" would fail pi's schema validation).
pub fn edit_pi_provider(
    home: &Path,
    provider_id: &str,
    patch: &ProviderPatch,
) -> ApplyResult {
    let mut result = ApplyResult::new();
    let path = home.join(".pi/agent/models.json");

    let mut root: Value = match read_json_object(&path) {
        Ok(Some(v)) => v,
        Ok(None) => {
            result.push_err(path, "models.json not found".into());
            return result;
        }
        Err(ReadError::Corrupt(e)) => {
            result.push_err(path, format!("corrupt, not overwriting: {e}"));
            return result;
        }
        Err(ReadError::Io(e)) => {
            result.push_err(path, format!("read: {e}"));
            return result;
        }
    };

    let node = {
        let obj = root
            .as_object_mut()
            .expect("read_json_object guarantees object");
        match obj
            .get_mut("providers")
            .and_then(|p| p.as_object_mut())
            .and_then(|p| p.get_mut(provider_id))
        {
            Some(n) if n.is_object() => n,
            _ => {
                result.push_err(path, format!("provider '{provider_id}' not found"));
                return result;
            }
        }
    };

    let node_obj = node.as_object_mut().expect("checked is_object");
    match &patch.name {
        // pi's provider `name` has minLength 1 — "" removes the key.
        Some(v) if v.is_empty() => {
            node_obj.remove("name");
        }
        Some(v) => {
            node_obj.insert("name".into(), json!(v));
        }
        None => {}
    }
    if let Some(v) = &patch.base_url {
        node_obj.insert("baseUrl".into(), json!(v));
    }
    if let Some(v) = &patch.api {
        node_obj.insert("api".into(), json!(v));
    }
    match &patch.api_key {
        // "" clears the key (same contract as the canonical masking rules).
        Some(v) if v.is_empty() => {
            node_obj.remove("apiKey");
        }
        Some(v) => {
            node_obj.insert("apiKey".into(), json!(v));
        }
        None => {}
    }

    let bytes = serde_json::to_vec_pretty(&root).expect("serialize pi models.json");
    match backup_write_verify_json(&path, &bytes, 0o600) {
        Ok(backup) => result.push_ok(path, backup),
        Err(msg) => result.push_err(path, msg),
    }
    result
}

/// Remove one provider node from ~/.pi/agent/models.json. When the node is
/// pi's current default (settings.json defaultProvider == id) the dangling
/// `defaultProvider`/`defaultModel` keys are removed from settings.json too
/// (key-level — everything else preserved). Design §2.
pub fn delete_pi_provider(home: &Path, provider_id: &str) -> ApplyResult {
    let mut result = ApplyResult::new();
    let path = home.join(".pi/agent/models.json");

    let mut root: Value = match read_json_object(&path) {
        Ok(Some(v)) => v,
        Ok(None) => {
            result.push_err(path, "models.json not found".into());
            return result;
        }
        Err(ReadError::Corrupt(e)) => {
            result.push_err(path, format!("corrupt, not overwriting: {e}"));
            return result;
        }
        Err(ReadError::Io(e)) => {
            result.push_err(path, format!("read: {e}"));
            return result;
        }
    };

    {
        let obj = root
            .as_object_mut()
            .expect("read_json_object guarantees object");
        let removed = obj
            .get_mut("providers")
            .and_then(|p| p.as_object_mut())
            .map(|p| p.remove(provider_id).is_some())
            .unwrap_or(false);
        if !removed {
            result.push_err(path, format!("provider '{provider_id}' not found"));
            return result;
        }
    }

    let bytes = serde_json::to_vec_pretty(&root).expect("serialize pi models.json");
    match backup_write_verify_json(&path, &bytes, 0o600) {
        Ok(backup) => result.push_ok(path.clone(), backup),
        Err(msg) => result.push_err(path.clone(), msg),
    }

    // Dangling-default cleanup in settings.json (only when it pointed at
    // the deleted provider).
    let settings_path = home.join(".pi/agent/settings.json");
    match read_json_object(&settings_path) {
        Ok(Some(mut s)) => {
            let is_default = s.get("defaultProvider").and_then(|x| x.as_str())
                == Some(provider_id);
            if is_default {
                let obj = s
                    .as_object_mut()
                    .expect("read_json_object guarantees object");
                obj.remove("defaultProvider");
                obj.remove("defaultModel");
                let bytes = serde_json::to_vec_pretty(&s).expect("serialize pi settings.json");
                match backup_write_verify_json(&settings_path, &bytes, 0o600) {
                    Ok(backup) => result.push_ok(settings_path, backup),
                    Err(msg) => result.push_err(settings_path, msg),
                }
            }
        }
        // No settings file — nothing can dangle.
        Ok(None) => {}
        Err(ReadError::Corrupt(e)) => {
            result.push_err(settings_path, format!("corrupt, not overwriting: {e}"));
        }
        Err(ReadError::Io(e)) => {
            result.push_err(settings_path, format!("read: {e}"));
        }
    }

    result
}

// ── pi ProviderEntry rendering ────────────────────────────────────

/// Build the pi-native ProviderEntry JSON for a canonical provider, with
/// empty/null fields omitted (pi-web's normalizeModelsConfigCosts does the
/// same; we don't want pi to receive empty `headers: {}`). The shape is
/// pi-web's `ProviderEntry` (research/pi-web §1) — NOT the canonical one
/// (no `anthropic` field: pi doesn't use it). `name` IS written: pi's
/// `ProviderConfigSchema` has an optional `name` and its display-name chain
/// prefers it over the provider key (pi provider-composer.ts:
/// `config?.name ?? providerId`) — without it pi shows the raw id
/// (e.g. "provider-1") instead of the user-named provider (08-28 task).
fn render_pi_provider(provider: &ProviderEntry) -> Value {
    let mut p = serde_json::Map::new();

    if !provider.name.is_empty() {
        p.insert("name".into(), json!(provider.name));
    }
    if !provider.base_url.is_empty() {
        p.insert("baseUrl".into(), json!(provider.base_url));
    }
    if !provider.api.is_empty() {
        p.insert("api".into(), json!(provider.api));
    }
    if let Some(key) = &provider.api_key {
        if !key.is_empty() {
            p.insert("apiKey".into(), json!(key));
        }
    }
    if !provider.headers.is_empty() {
        p.insert("headers".into(), json!(provider.headers));
    }
    if !provider.compat.is_null() {
        p.insert("compat".into(), provider.compat.clone());
    }

    let models: Vec<Value> = provider.models.iter().map(render_pi_model).collect();
    p.insert("models".into(), Value::Array(models));

    Value::Object(p)
}

/// Build a pi-native ModelEntry, omitting empty/null fields. The shape is
/// pi-web's `ModelEntry` (research/pi-web §1): id, name?, api?, reasoning?,
/// input?, contextWindow?, maxTokens?, cost?{input,output,cacheRead,
/// cacheWrite}, headers?, compat?.
fn render_pi_model(m: &ModelEntry) -> Value {
    let mut o = serde_json::Map::new();

    o.insert("id".into(), json!(m.id));

    if let Some(name) = &m.name {
        if !name.is_empty() {
            o.insert("name".into(), json!(name));
        }
    }
    if let Some(api) = &m.api {
        if !api.is_empty() {
            o.insert("api".into(), json!(api));
        }
    }
    if m.reasoning {
        o.insert("reasoning".into(), json!(true));
    }
    if let Some(input) = &m.input {
        if !input.is_empty() {
            o.insert("input".into(), json!(input));
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
    if let Some(headers) = &m.headers {
        if !headers.is_empty() {
            o.insert("headers".into(), json!(headers));
        }
    }
    if !m.compat.is_null() {
        o.insert("compat".into(), m.compat.clone());
    }

    Value::Object(o)
}

/// canonical `CostEntry` is USD-per-1M-tokens; pi's native `models.json` cost
/// fields are the SAME unit — pi's usage math multiplies rates by token counts
/// then divides by 1e6 (pi packages/ai/src/models.ts `usage.cost.*`), and its
/// bundled generated data matches models.dev verbatim. Pass through as-is.
///
/// pi's `ModelCostSchema` requires all four fields whenever `cost` is present
/// (only `tiers` is optional), and models.dev often omits `cache_write`
/// (e.g. deepseek) — fill missing fields with 0, matching pi's own
/// generate-models semantics (`cacheWrite: cost?.cache_write || 0`). An entry
/// with no known field still renders empty => caller omits `cost` entirely
/// (cost itself is optional in the schema).
fn render_pi_cost(cost: &CostEntry) -> serde_json::Map<String, Value> {
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
    // Known-field fill: once any rate is present the other three default to 0
    // (pi's generate-models uses `cache_read || 0` for the same reason). An
    // entry with no known field stays empty => caller omits `cost` entirely.
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
    use crate::routes::models::store::{
        AgentAssignment, CanonicalConfig, CostEntry, ModelEntry, ProviderEntry,
    };
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp_home() -> std::path::PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut p = std::env::temp_dir();
        p.push(format!("aio-pi-{}-{}", std::process::id(), n));
        std::fs::create_dir_all(p.join(".pi/agent")).unwrap();
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
                    name: Some("DeepSeek V4 Pro".into()),
                    reasoning: true,
                    context_window: Some(100_000),
                    max_tokens: Some(4_000),
                    cost: Some(CostEntry {
                        input: Some(0.435),
                        output: Some(0.87),
                        cache_read: Some(0.003_625),
                        cache_write: None,
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        c.agents.pi = Some(AgentAssignment {
            provider: "aruoshui".into(),
            model: "deepseek-v4-pro".into(),
        });
        c
    }

    // --- render_pi_provider ---

    #[test]
    fn provider_renders_omit_empty_fields() {
        let p = ProviderEntry {
            name: "P".into(),
            base_url: "https://x/v1".into(),
            api: "openai-completions".into(),
            api_key: Some("sk-abc12345".into()),
            ..Default::default()
        };
        let v = render_pi_provider(&p);
        // `name` IS written (pi's display-name chain prefers it over the key).
        assert_eq!(v["name"], "P");
        // No `anthropic` in pi's shape.
        assert!(v.get("anthropic").is_none());
        assert_eq!(v["baseUrl"], "https://x/v1");
        assert_eq!(v["api"], "openai-completions");
        assert_eq!(v["apiKey"], "sk-abc12345");
        // No `headers` when empty.
        assert!(v.get("headers").is_none());
        assert!(v.get("models").is_some());
    }

    #[test]
    fn provider_empty_name_omitted() {
        let p = ProviderEntry {
            name: String::new(),
            base_url: "https://x/v1".into(),
            api: "openai-completions".into(),
            ..Default::default()
        };
        let v = render_pi_provider(&p);
        assert!(v.get("name").is_none(), "empty name must be omitted");
    }

    #[test]
    fn provider_omits_empty_api_key() {
        let p = ProviderEntry {
            name: "P".into(),
            base_url: "https://x/v1".into(),
            api: "openai-completions".into(),
            api_key: Some(String::new()),
            ..Default::default()
        };
        let v = render_pi_provider(&p);
        assert!(v.get("apiKey").is_none());
    }

    #[test]
    fn model_cost_renders_camel_case() {
        let m = ModelEntry {
            id: "m".into(),
            cost: Some(CostEntry {
                input: Some(0.1),
                output: Some(0.2),
                cache_read: Some(0.3),
                cache_write: None,
            }),
            ..Default::default()
        };
        let v = render_pi_model(&m);
        // canonical is $/M; pi's native schema is the same unit (pass-through).
        assert_eq!(v["cost"]["input"], 0.1);
        assert_eq!(v["cost"]["output"], 0.2);
        assert_eq!(v["cost"]["cacheRead"], 0.3);
    }

    #[test]
    fn model_cost_fills_required_fields_with_zero() {
        // pi's ModelCostSchema requires all four fields whenever cost is
        // present; models.dev often omits cache_write (e.g. deepseek). Missing
        // fields must render as 0 (pi's own generate-models uses `|| 0`), or
        // pi rejects the file with "must have required properties cacheWrite".
        let m = ModelEntry {
            id: "m".into(),
            cost: Some(CostEntry {
                input: Some(0.14),
                output: Some(0.28),
                cache_read: Some(0.0028),
                cache_write: None,
            }),
            ..Default::default()
        };
        let v = render_pi_model(&m);
        assert_eq!(v["cost"]["input"], 0.14);
        assert_eq!(v["cost"]["cacheWrite"], 0);
        let mut keys: Vec<&str> =
            v["cost"].as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["cacheRead", "cacheWrite", "input", "output"],
            "all four required fields present"
        );
    }

    #[test]
    fn model_cost_all_none_omits_cost_object() {
        // cost itself is optional in pi's schema — nothing known, nothing written.
        let m = ModelEntry {
            id: "m".into(),
            cost: Some(CostEntry::default()),
            ..Default::default()
        };
        let v = render_pi_model(&m);
        assert!(v.get("cost").is_none());
    }

    // --- apply_pi golden path ---

    #[test]
    fn apply_pi_preserves_other_providers_and_unknown_keys() {
        let home = temp_home();
        // Seed pi models.json with another provider + an unknown top-level key.
        std::fs::write(
            home.join(".pi/agent/models.json"),
            r#"{
  "version": 1,
  "providers": {
    "existing-prov": {
      "baseUrl": "https://other.example/v1",
      "api": "openai-completions",
      "apiKey": "sk-other-key-xxxx",
      "models": [{"id":"old-model","custom":"keep-me"}]
    }
  },
  "unknownKey": 42
}"#,
        )
        .unwrap();
        std::fs::write(
            home.join(".pi/agent/settings.json"),
            r#"{"defaultProvider":"existing-prov","defaultModel":"old-model","extra":"keep"}"#,
        )
        .unwrap();

        let cfg = sample_config();
        let r = apply_pi(&home, &cfg);
        assert!(r.ok, "errors: {:?}", r.errors);
        assert_eq!(r.written.len(), 2);
        assert!(r.errors.is_empty());

        // models.json: existing provider preserved verbatim; new provider added.
        let after: Value =
            serde_json::from_str(&std::fs::read_to_string(home.join(".pi/agent/models.json")).unwrap())
                .unwrap();
        assert_eq!(after["unknownKey"], 42, "unknown top-level key preserved");
        assert_eq!(
            after["providers"]["existing-prov"]["apiKey"],
            "sk-other-key-xxxx",
            "existing provider untouched"
        );
        assert_eq!(
            after["providers"]["existing-prov"]["models"][0]["custom"],
            "keep-me"
        );
        assert_eq!(after["providers"]["aruoshui"]["baseUrl"], "https://ai.aruoshui.com/v1");
        assert_eq!(
            after["providers"]["aruoshui"]["apiKey"],
            "sk-real-key-xxxx"
        );

        // settings.json: only defaultProvider/defaultModel set; extra preserved.
        let s: Value =
            serde_json::from_str(&std::fs::read_to_string(home.join(".pi/agent/settings.json")).unwrap())
                .unwrap();
        assert_eq!(s["defaultProvider"], "aruoshui");
        assert_eq!(s["defaultModel"], "deepseek-v4-pro");
        assert_eq!(s["extra"], "keep");
    }

    #[test]
    fn apply_pi_creates_files_when_missing() {
        let home = temp_home();
        // No prior files.
        let cfg = sample_config();
        let r = apply_pi(&home, &cfg);
        assert!(r.ok);
        assert_eq!(r.written.len(), 2);
        // written entries with no backup (files didn't exist before).
        assert!(r.written[0].backup.is_none());

        // Permission 0600 on models.json, 0644 on settings.json.
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

    #[test]
    fn apply_pi_corrupt_models_aborts_models_but_writes_settings() {
        let home = temp_home();
        std::fs::write(home.join(".pi/agent/models.json"), "{{not json").unwrap();
        std::fs::write(
            home.join(".pi/agent/settings.json"),
            r#"{"defaultProvider":"old"}"#,
        )
        .unwrap();

        let cfg = sample_config();
        let r = apply_pi(&home, &cfg);

        // models.json: error, NOT overwritten (still corrupt).
        assert!(!r.ok, "ok should be false: {r:?}");
        assert!(r.errors.iter().any(|e| e.path.contains("models.json")));
        assert_eq!(
            std::fs::read_to_string(home.join(".pi/agent/models.json")).unwrap(),
            "{{not json",
            "corrupt file must be left untouched"
        );

        // settings.json: still written.
        assert!(r.written.iter().any(|w| w.path.contains("settings.json")));
        let s: Value =
            serde_json::from_str(&std::fs::read_to_string(home.join(".pi/agent/settings.json")).unwrap())
                .unwrap();
        assert_eq!(s["defaultProvider"], "aruoshui");
    }

    #[test]
    fn apply_pi_creates_backup_on_existing_file() {
        let home = temp_home();
        std::fs::write(
            home.join(".pi/agent/models.json"),
            r#"{"providers":{"old":{"baseUrl":"https://x"}}}"#,
        )
        .unwrap();
        let cfg = sample_config();
        let r = apply_pi(&home, &cfg);
        assert!(r.ok);
        let models_written = r
            .written
            .iter()
            .find(|w| w.path.contains("models.json"))
            .unwrap();
        assert!(models_written.backup.is_some(), "backup should exist");
        assert!(models_written
            .backup
            .as_ref()
            .unwrap()
            .contains("aio-bak-"));
    }

    #[test]
    fn apply_pi_prunes_old_backups() {
        let home = temp_home();
        let path = home.join(".pi/agent/models.json");
        std::fs::write(&path, r#"{"providers":{}}"#).unwrap();
        let cfg = sample_config();
        // Apply 4 times — pruning should keep newest 3 backups.
        for _ in 0..4 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            let _ = apply_pi(&home, &cfg);
        }
        let n = std::fs::read_dir(home.join(".pi/agent"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("models.json.aio-bak-")
            })
            .count();
        assert!(n <= 3, "expected <=3 backups, got {n}");
    }

    #[test]
    fn apply_pi_missing_assignment_errors() {
        let home = temp_home();
        let mut cfg = sample_config();
        cfg.agents.pi = None;
        let r = apply_pi(&home, &cfg);
        assert!(!r.ok);
        assert_eq!(r.errors.len(), 1);
        assert!(r.errors[0].message.contains("no pi assignment"));
    }

    #[test]
    fn apply_pi_unknown_provider_errors() {
        let home = temp_home();
        let mut cfg = sample_config();
        cfg.agents.pi = Some(AgentAssignment {
            provider: "nope".into(),
            model: "m".into(),
        });
        let r = apply_pi(&home, &cfg);
        assert!(!r.ok);
        assert!(r.errors[0].message.contains("not found"));
    }

    // --- live provider edit / delete (agent-tab live management) ---

    fn seed_live_pi(home: &std::path::Path) {
        std::fs::write(
            home.join(".pi/agent/models.json"),
            r#"{
  "version": 1,
  "providers": {
    "prov-a": {
      "baseUrl": "https://a.example/v1",
      "api": "openai-completions",
      "apiKey": "sk-old-key-xxxx",
      "models": [{"id": "model-a", "cost": {"input": 1.4e-07}}]
    },
    "prov-b": {"baseUrl": "https://b.example/v1"}
  },
  "unknownKey": 42
}"#,
        )
        .unwrap();
        std::fs::write(
            home.join(".pi/agent/settings.json"),
            r#"{"defaultProvider":"prov-a","defaultModel":"model-a","extra":"keep"}"#,
        )
        .unwrap();
    }

    fn read_models(home: &std::path::Path) -> Value {
        serde_json::from_str(
            &std::fs::read_to_string(home.join(".pi/agent/models.json")).unwrap(),
        )
        .unwrap()
    }

    fn read_settings(home: &std::path::Path) -> Value {
        serde_json::from_str(
            &std::fs::read_to_string(home.join(".pi/agent/settings.json")).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn edit_pi_provider_merges_fields_only() {
        let home = temp_home();
        seed_live_pi(&home);

        let patch = crate::routes::models::render::ProviderPatch {
            name: Some("Ignored For Pi".into()), // pi nodes have no name field
            base_url: Some("https://new.example/v1".into()),
            api: Some("openai-responses".into()),
            api_key: Some("sk-new-key-xxxx".into()),
        };
        let r = edit_pi_provider(&home, "prov-a", &patch);
        assert!(r.ok, "errors: {:?}", r.errors);

        let after = read_models(&home);
        // Patched keys changed.
        assert_eq!(after["providers"]["prov-a"]["baseUrl"], "https://new.example/v1");
        assert_eq!(after["providers"]["prov-a"]["api"], "openai-responses");
        assert_eq!(after["providers"]["prov-a"]["apiKey"], "sk-new-key-xxxx");
        assert_eq!(after["providers"]["prov-a"]["name"], "Ignored For Pi");
        // Node's models (incl. cost) preserved verbatim.
        assert_eq!(after["providers"]["prov-a"]["models"][0]["id"], "model-a");
        assert_eq!(after["providers"]["prov-a"]["models"][0]["cost"]["input"], 1.4e-7);
        // Sibling provider and unknown top-level key untouched.
        assert_eq!(after["providers"]["prov-b"]["baseUrl"], "https://b.example/v1");
        assert_eq!(after["unknownKey"], 42);
    }

    #[test]
    fn edit_pi_provider_empty_name_removes_key() {
        let home = temp_home();
        seed_live_pi(&home);

        let patch = crate::routes::models::render::ProviderPatch {
            name: Some(String::new()), // minLength 1 — "" removes the key
            ..Default::default()
        };
        let r = edit_pi_provider(&home, "prov-a", &patch);
        assert!(r.ok, "errors: {:?}", r.errors);
        let after = read_models(&home);
        assert!(after["providers"]["prov-a"].get("name").is_none());
        // Other fields survive.
        assert_eq!(after["providers"]["prov-a"]["baseUrl"], "https://a.example/v1");
    }

    #[test]
    fn edit_pi_provider_empty_api_key_clears() {
        let home = temp_home();
        seed_live_pi(&home);

        let patch = crate::routes::models::render::ProviderPatch {
            api_key: Some(String::new()),
            ..Default::default()
        };
        let r = edit_pi_provider(&home, "prov-a", &patch);
        assert!(r.ok);
        let after = read_models(&home);
        assert!(after["providers"]["prov-a"].get("apiKey").is_none());
        // Other fields survive.
        assert_eq!(after["providers"]["prov-a"]["baseUrl"], "https://a.example/v1");
    }

    #[test]
    fn edit_pi_provider_missing_node_errors_and_leaves_file() {
        let home = temp_home();
        seed_live_pi(&home);
        let before = std::fs::read_to_string(home.join(".pi/agent/models.json")).unwrap();

        let patch = crate::routes::models::render::ProviderPatch {
            base_url: Some("https://x".into()),
            ..Default::default()
        };
        let r = edit_pi_provider(&home, "nope", &patch);
        assert!(!r.ok);
        assert!(r.errors[0].message.contains("not found"));
        assert_eq!(
            std::fs::read_to_string(home.join(".pi/agent/models.json")).unwrap(),
            before,
            "file untouched on error"
        );
    }

    #[test]
    fn edit_pi_provider_missing_file_errors() {
        let home = temp_home();
        let patch = crate::routes::models::render::ProviderPatch::default();
        let r = edit_pi_provider(&home, "prov-a", &patch);
        assert!(!r.ok);
        assert!(r.errors[0].message.contains("not found"));
    }

    #[test]
    fn delete_pi_provider_clears_dangling_default() {
        let home = temp_home();
        seed_live_pi(&home);

        let r = delete_pi_provider(&home, "prov-a");
        assert!(r.ok, "errors: {:?}", r.errors);
        // models.json + settings.json both written.
        assert_eq!(r.written.len(), 2);

        let models = read_models(&home);
        assert!(models["providers"].get("prov-a").is_none());
        assert!(models["providers"].get("prov-b").is_some(), "sibling kept");
        assert_eq!(models["unknownKey"], 42);

        // settings.json: dangling defaults removed, other keys kept.
        let settings = read_settings(&home);
        assert!(settings.get("defaultProvider").is_none());
        assert!(settings.get("defaultModel").is_none());
        assert_eq!(settings["extra"], "keep");
    }

    #[test]
    fn delete_pi_provider_keeps_settings_when_not_default() {
        let home = temp_home();
        seed_live_pi(&home);

        let r = delete_pi_provider(&home, "prov-b");
        assert!(r.ok);
        assert_eq!(r.written.len(), 1, "only models.json written");

        let models = read_models(&home);
        assert!(models["providers"].get("prov-b").is_none());
        assert!(models["providers"].get("prov-a").is_some());

        // settings.json untouched (default still valid).
        let settings = read_settings(&home);
        assert_eq!(settings["defaultProvider"], "prov-a");
        assert_eq!(settings["defaultModel"], "model-a");
    }

    #[test]
    fn delete_pi_provider_missing_node_errors() {
        let home = temp_home();
        seed_live_pi(&home);
        let r = delete_pi_provider(&home, "nope");
        assert!(!r.ok);
        assert!(r.errors[0].message.contains("not found"));
    }

    // silence unused import in test build
    #[test]
    fn home_dir_resolves() {
        let _ = crate::routes::models::render::home_dir();
    }
}
