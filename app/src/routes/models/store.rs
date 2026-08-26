// Canonical model-config store.
//
// The canonical config lives at `~/.aio/models.json` (0600, atomic write). It is
// the single source of truth for providers (endpoint/key/protocol/models) and
// per-agent assignments. All functions take an injectable `&Path` so tests use
// temp dirs and routes use `state.models_file` — no global mutable state.
//
// Schema follows design §2 (pi's models.json schema superset, extended with
// protocol declaration and per-agent assignments). See research/pi-web-*.md
// for the canonical shape and its pi-web/cc-switch lineage.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── types ──────────────────────────────────────────────────────────

/// Top-level canonical config. `version` is always 1 on write.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalConfig {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderEntry>,
    #[serde(default)]
    pub agents: AgentsConfig,
}

fn default_version() -> u32 {
    1
}

impl Default for CanonicalConfig {
    fn default() -> Self {
        Self {
            version: 1,
            providers: BTreeMap::new(),
            agents: AgentsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentsConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pi: Option<AgentAssignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opencode: Option<AgentAssignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude: Option<ClaudeAssignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex: Option<CodexAssignment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssignment {
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeAssignment {
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub haiku_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sonnet_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opus_model: Option<String>,
    #[serde(default = "default_auth_field")]
    pub auth_field: String,
}

fn default_auth_field() -> String {
    "AUTH_TOKEN".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexAssignment {
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default = "default_wire_api")]
    pub wire_api: String,
}

fn default_wire_api() -> String {
    "responses".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderEntry {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default = "default_api")]
    pub api: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub compat: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anthropic: Option<AnthropicBlock>,
    #[serde(default)]
    pub models: Vec<ModelEntry>,
}

fn default_api() -> String {
    "openai-completions".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnthropicBlock {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelEntry {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<CostEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub compat: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<f64>,
}

// ── API response wrappers ──────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct PutResponse {
    pub ok: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ImportResponse {
    pub ok: bool,
    pub imported: Vec<String>,
    pub skipped: Vec<String>,
}

// ── error type ─────────────────────────────────────────────────────

#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
    Corrupt(serde_json::Error),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "io: {e}"),
            StoreError::Corrupt(e) => write!(f, "corrupt: {e}"),
        }
    }
}

impl std::error::Error for StoreError {}

// ── read / write ───────────────────────────────────────────────────

/// Read and parse the canonical config. Missing file => default empty.
/// Corrupt JSON => error (caller must move the file aside, NOT overwrite).
pub fn read_config(path: &Path) -> Result<CanonicalConfig, StoreError> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            if text.trim().is_empty() {
                return Ok(CanonicalConfig::default());
            }
            serde_json::from_str::<CanonicalConfig>(&text).map_err(StoreError::Corrupt)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(CanonicalConfig::default()),
        Err(e) => Err(StoreError::Io(e)),
    }
}

/// Atomically write the canonical config (temp file + rename, mode 0600).
/// Parent dir is created with 0755 if missing.
pub fn write_config(path: &Path, config: &CanonicalConfig) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o755));
    }

    let json = serde_json::to_string_pretty(config).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

// ── key masking ────────────────────────────────────────────────────

/// Mask an API key for GET responses: first 3 + "****" + last 4 when
/// len >= 8, else "****". Character-based (not byte) slicing: API keys are
/// ASCII in practice, but a non-ASCII key must not panic on a UTF-8 boundary.
pub fn mask_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() >= 8 {
        let head: String = chars[..3].iter().collect();
        let tail: String = chars[chars.len() - 4..].iter().collect();
        format!("{head}****{tail}")
    } else {
        "****".to_string()
    }
}

/// Replace every provider's apiKey with its masked form (for GET responses).
pub fn mask_config(config: &mut CanonicalConfig) {
    for provider in config.providers.values_mut() {
        if let Some(key) = &provider.api_key {
            if !key.is_empty() {
                provider.api_key = Some(mask_key(key));
            }
        }
    }
}

// ── merge (masked-echo semantics) ──────────────────────────────────

/// Merge incoming PUT body with stored keys:
/// - None (field absent) => keep stored key
/// - Some("") => clear (set to None)
/// - Some(mask) => keep stored key (masked echo)
/// - Some(other) => store new value
/// After merge, empty-string keys are normalized to None for new providers.
pub fn merge_api_keys(stored: &CanonicalConfig, incoming: &mut CanonicalConfig) {
    for (id, incoming_provider) in &mut incoming.providers {
        if let Some(stored_provider) = stored.providers.get(id) {
            match &incoming_provider.api_key {
                None => {
                    incoming_provider.api_key = stored_provider.api_key.clone();
                }
                Some(k) if k.is_empty() => {
                    incoming_provider.api_key = None;
                }
                Some(k) if k == &mask_key(stored_provider.api_key.as_deref().unwrap_or("")) => {
                    incoming_provider.api_key = stored_provider.api_key.clone();
                }
                Some(_) => { /* new value: keep as-is */ }
            }
        }
        // New provider (not in stored): normalize empty string to None.
        if matches!(&incoming_provider.api_key, Some(k) if k.is_empty()) {
            incoming_provider.api_key = None;
        }
    }
}

// ── validation ─────────────────────────────────────────────────────

/// Validate the canonical config. Returns Ok(()) or a list of error messages.
pub fn validate(config: &CanonicalConfig) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();

    for (id, provider) in &config.providers {
        if !is_valid_provider_id(id) {
            errors.push(format!(
                "invalid provider id '{}': must be [a-z0-9-]+",
                id
            ));
        }
        for model in &provider.models {
            if model.id.is_empty() {
                errors.push(format!("provider '{}' has a model with empty id", id));
            }
        }
    }

    if let Some(a) = &config.agents.pi {
        validate_assignment("pi", &a.provider, &a.model, &config.providers, &mut errors);
    }
    if let Some(a) = &config.agents.opencode {
        validate_assignment("opencode", &a.provider, &a.model, &config.providers, &mut errors);
    }
    if let Some(a) = &config.agents.claude {
        validate_assignment("claude", &a.provider, &a.model, &config.providers, &mut errors);
    }
    if let Some(a) = &config.agents.codex {
        validate_assignment("codex", &a.provider, &a.model, &config.providers, &mut errors);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn is_valid_provider_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn validate_assignment(
    agent: &str,
    provider: &str,
    model: &str,
    providers: &BTreeMap<String, ProviderEntry>,
    errors: &mut Vec<String>,
) {
    if !providers.contains_key(provider) {
        errors.push(format!(
            "agent '{}' references unknown provider '{}'",
            agent, provider
        ));
    } else if !providers[provider]
        .models
        .iter()
        .any(|m| m.id == model)
    {
        errors.push(format!(
            "agent '{}' model '{}' not found in provider '{}'",
            agent, model, provider
        ));
    }
}

// ── pi import ──────────────────────────────────────────────────────

/// Result of importing providers from pi's models.json.
pub struct ImportResult {
    pub imported: Vec<String>,
    pub skipped: Vec<String>,
    pub providers: BTreeMap<String, ProviderEntry>,
}

/// Read pi's models.json and map each provider 1:1 into canonical format.
/// - id = pi provider key if already kebab-case, else sanitized
/// - name = original key (when provider.name is empty)
/// - api/baseUrl/apiKey/headers/compat/models carried over
/// - anthropic block set when api == "anthropic-messages"
/// Providers whose id already exists in `current` are skipped.
pub fn import_from_pi(
    pi_path: &Path,
    current: &CanonicalConfig,
) -> Result<ImportResult, StoreError> {
    let text = std::fs::read_to_string(pi_path).map_err(StoreError::Io)?;
    let pi_config: CanonicalConfig = serde_json::from_str(&text).map_err(StoreError::Corrupt)?;

    let mut imported = Vec::new();
    let mut skipped = Vec::new();
    let mut new_providers: BTreeMap<String, ProviderEntry> = BTreeMap::new();

    for (key, mut provider) in pi_config.providers {
        let id = if is_valid_provider_id(&key) {
            key.clone()
        } else {
            sanitize_id(&key)
        };

        if current.providers.contains_key(&id) || new_providers.contains_key(&id) {
            skipped.push(id);
            continue;
        }

        if provider.name.is_empty() {
            provider.name = key.clone();
        }

        if provider.api == "anthropic-messages" && provider.anthropic.is_none() {
            provider.anthropic = Some(AnthropicBlock {
                base_url: Some(provider.base_url.clone()),
            });
        }

        new_providers.insert(id.clone(), provider);
        imported.push(id);
    }

    Ok(ImportResult {
        imported,
        skipped,
        providers: new_providers,
    })
}

/// Sanitize a key into a valid provider id: lowercase, non-[a-z0-9] -> '-'.
fn sanitize_id(key: &str) -> String {
    let s: String = key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let s = s.trim_matches('-');
    if s.is_empty() {
        "provider".to_string()
    } else {
        s.to_string()
    }
}

// ── tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Unique temp dir per test call (avoids cross-test collisions).
    fn temp_dir() -> std::path::PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut p = std::env::temp_dir();
        p.push(format!("aio-models-test-{}-{}", std::process::id(), n));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn config_path(dir: &std::path::Path) -> std::path::PathBuf {
        dir.join("models.json")
    }

    // --- mask_key ---

    #[test]
    fn mask_key_long() {
        assert_eq!(mask_key("sk-abcdef1234"), "sk-****1234");
    }

    #[test]
    fn mask_key_short() {
        assert_eq!(mask_key("short"), "****");
        assert_eq!(mask_key(""), "****");
    }

    // --- read/write round-trip ---

    #[test]
    fn read_missing_returns_default() {
        let dir = temp_dir();
        let path = config_path(&dir);
        let cfg = read_config(&path).unwrap();
        assert_eq!(cfg.version, 1);
        assert!(cfg.providers.is_empty());
    }

    #[test]
    fn write_then_read_round_trips() {
        let dir = temp_dir();
        let path = config_path(&dir);
        let mut cfg = CanonicalConfig::default();
        cfg.providers.insert(
            "my-prov".into(),
            ProviderEntry {
                name: "Test".into(),
                base_url: "https://api.test.com/v1".into(),
                api: "openai-completions".into(),
                api_key: Some("sk-secret12345".into()),
                ..Default::default()
            },
        );
        write_config(&path, &cfg).unwrap();

        // File must be 0600.
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        let read = read_config(&path).unwrap();
        assert_eq!(read.providers.len(), 1);
        assert_eq!(
            read.providers["my-prov"].api_key.as_deref(),
            Some("sk-secret12345")
        );
    }

    // --- mask round-trip (the core PUT semantics) ---

    #[test]
    fn mask_round_trip_preserves_key() {
        let dir = temp_dir();
        let path = config_path(&dir);

        // Store a config with a real key.
        let mut cfg = CanonicalConfig::default();
        cfg.providers.insert(
            "prov".into(),
            ProviderEntry {
                name: "P".into(),
                base_url: "https://x".into(),
                api: "openai-completions".into(),
                api_key: Some("sk-realsecret999".into()),
                ..Default::default()
            },
        );
        write_config(&path, &cfg).unwrap();

        // GET masks the key.
        let mut masked = read_config(&path).unwrap();
        mask_config(&mut masked);
        assert_eq!(
            masked.providers["prov"].api_key.as_deref(),
            Some("sk-****t999")
        );

        // PUT sends the mask back unchanged.
        let stored = read_config(&path).unwrap();
        let mut incoming = masked.clone();
        merge_api_keys(&stored, &mut incoming);
        assert_eq!(
            incoming.providers["prov"].api_key.as_deref(),
            Some("sk-realsecret999")
        );
    }

    #[test]
    fn put_empty_string_clears_key() {
        let dir = temp_dir();
        let path = config_path(&dir);

        let mut cfg = CanonicalConfig::default();
        cfg.providers.insert(
            "prov".into(),
            ProviderEntry {
                name: "P".into(),
                base_url: "https://x".into(),
                api: "openai-completions".into(),
                api_key: Some("sk-realsecret999".into()),
                ..Default::default()
            },
        );
        write_config(&path, &cfg).unwrap();

        let stored = read_config(&path).unwrap();
        let mut incoming = stored.clone();
        // Simulate the user clearing the field.
        incoming.providers.get_mut("prov").unwrap().api_key = Some(String::new());
        merge_api_keys(&stored, &mut incoming);
        assert!(incoming.providers["prov"].api_key.is_none());
    }

    #[test]
    fn put_new_value_replaces_key() {
        let dir = temp_dir();
        let path = config_path(&dir);

        let mut cfg = CanonicalConfig::default();
        cfg.providers.insert(
            "prov".into(),
            ProviderEntry {
                name: "P".into(),
                base_url: "https://x".into(),
                api: "openai-completions".into(),
                api_key: Some("sk-oldsecret99".into()),
                ..Default::default()
            },
        );
        write_config(&path, &cfg).unwrap();

        let stored = read_config(&path).unwrap();
        let mut incoming = stored.clone();
        incoming.providers.get_mut("prov").unwrap().api_key =
            Some("sk-brand-new-key".into());
        merge_api_keys(&stored, &mut incoming);
        assert_eq!(
            incoming.providers["prov"].api_key.as_deref(),
            Some("sk-brand-new-key")
        );
    }

    #[test]
    fn new_provider_empty_key_normalized_to_none() {
        let stored = CanonicalConfig::default();
        let mut incoming = CanonicalConfig::default();
        incoming.providers.insert(
            "new".into(),
            ProviderEntry {
                name: "New".into(),
                base_url: "https://x".into(),
                api: "openai-completions".into(),
                api_key: Some(String::new()),
                ..Default::default()
            },
        );
        merge_api_keys(&stored, &mut incoming);
        assert!(incoming.providers["new"].api_key.is_none());
    }

    // --- validation ---

    #[test]
    fn validate_rejects_bad_provider_id() {
        let mut cfg = CanonicalConfig::default();
        cfg.providers.insert(
            "Bad_ID".into(),
            ProviderEntry {
                name: "X".into(),
                base_url: "https://x".into(),
                api: "openai-completions".into(),
                ..Default::default()
            },
        );
        let errs = validate(&cfg).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("Bad_ID")));
    }

    #[test]
    fn validate_rejects_dangling_agent_ref() {
        let mut cfg = CanonicalConfig::default();
        cfg.providers.insert(
            "prov".into(),
            ProviderEntry {
                name: "P".into(),
                base_url: "https://x".into(),
                api: "openai-completions".into(),
                models: vec![ModelEntry {
                    id: "model-a".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        cfg.agents.pi = Some(AgentAssignment {
            provider: "nonexistent".into(),
            model: "model-a".into(),
        });
        let errs = validate(&cfg).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("unknown provider")));

        // Fix provider, but model doesn't exist.
        cfg.agents.pi = Some(AgentAssignment {
            provider: "prov".into(),
            model: "no-such-model".into(),
        });
        let errs = validate(&cfg).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("not found")));
    }

    #[test]
    fn validate_accepts_valid_config() {
        let mut cfg = CanonicalConfig::default();
        cfg.providers.insert(
            "good-prov".into(),
            ProviderEntry {
                name: "Good".into(),
                base_url: "https://x".into(),
                api: "openai-completions".into(),
                models: vec![ModelEntry {
                    id: "m1".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        cfg.agents.pi = Some(AgentAssignment {
            provider: "good-prov".into(),
            model: "m1".into(),
        });
        assert!(validate(&cfg).is_ok());
    }

    // --- corrupt file protection ---

    #[test]
    fn corrupt_file_returns_error_not_default() {
        let dir = temp_dir();
        let path = config_path(&dir);
        std::fs::write(&path, "{{{{not json").unwrap();
        let result = read_config(&path);
        assert!(result.is_err());
        match result {
            Err(StoreError::Corrupt(_)) => {}
            other => panic!("expected Corrupt, got {:?}", other),
        }
    }

    // --- pi import ---

    #[test]
    fn pi_import_maps_providers() {
        let dir = temp_dir();
        let pi_path = dir.join("pi_models.json");
        std::fs::write(
            &pi_path,
            r#"{
  "providers": {
    "My-Provider": {
      "baseUrl": "https://api.test.com/v1",
      "api": "openai-completions",
      "apiKey": "sk-testkey1234",
      "models": [
        {"id": "model-a", "name": "Model A", "reasoning": true}
      ]
    },
    "anthropic-prov": {
      "baseUrl": "https://api.anthropic.com",
      "api": "anthropic-messages",
      "apiKey": "sk-ant-key-xxxx",
      "models": [
        {"id": "claude-test"}
      ]
    }
  }
}"#,
        )
        .unwrap();

        let current = CanonicalConfig::default();
        let result = import_from_pi(&pi_path, &current).unwrap();

        assert_eq!(result.imported.len(), 2);
        assert!(result.imported.contains(&"my-provider".into()));
        assert!(result.imported.contains(&"anthropic-prov".into()));

        let p = &result.providers["my-provider"];
        assert_eq!(p.name, "My-Provider");
        assert_eq!(p.base_url, "https://api.test.com/v1");
        assert_eq!(p.api, "openai-completions");
        assert!(p.anthropic.is_none());

        let a = &result.providers["anthropic-prov"];
        assert!(a.anthropic.is_some());
        assert_eq!(
            a.anthropic.as_ref().unwrap().base_url.as_deref(),
            Some("https://api.anthropic.com")
        );
    }

    #[test]
    fn pi_import_skips_existing() {
        let dir = temp_dir();
        let pi_path = dir.join("pi_models.json");
        std::fs::write(
            &pi_path,
            r#"{"providers": {"existing": {"baseUrl": "https://x", "api": "openai-completions"}}}"#,
        )
        .unwrap();

        let mut current = CanonicalConfig::default();
        current.providers.insert(
            "existing".into(),
            ProviderEntry {
                name: "Existing".into(),
                ..Default::default()
            },
        );

        let result = import_from_pi(&pi_path, &current).unwrap();
        assert!(result.imported.is_empty());
        assert_eq!(result.skipped, vec!["existing"]);
    }
}
