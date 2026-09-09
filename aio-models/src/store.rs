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
use std::collections::HashSet;
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
    pub claude: Option<ClaudePresets>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex: Option<CodexPresets>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssignment {
    pub provider: String,
    pub model: String,
}

// ── claude/codex presets (cc-switch style, design §1) ─────────────
//
// claude and codex are switch-style agents: exactly one preset takes effect
// at a time. A preset derives from the unified provider library (provider id
// + model + override fields) — it never copies the provider's key/headers
// blob; credentials stay in the provider library (SSOT).

/// N named claude presets + the currently-selected one. `current` is a preset
/// id; unset or dangling (apply refuses, validate rejects on PUT).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", from = "ClaudePresetsShadow")]
pub struct ClaudePresets {
    pub presets: Vec<ClaudePreset>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudePreset {
    /// Backend-generated short kebab id (`preset-<hex>`); backfilled on PUT
    /// when the frontend creates a preset without one (design §2).
    #[serde(default)]
    pub id: String,
    #[serde(default = "default_preset_name")]
    pub name: String,
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

fn default_preset_name() -> String {
    "默认配置".to_string()
}

impl ClaudePresets {
    /// The preset `current` points at; None when current is unset or dangling.
    pub fn current_preset(&self) -> Option<&ClaudePreset> {
        self.current
            .as_ref()
            .and_then(|id| self.presets.iter().find(|p| &p.id == id))
    }
}

/// Deserialization shadow: eats both the new shape (`presets`+`current`) and
/// the pre-preset single-assignment shape (`provider`/`model`/... at the top
/// level), converting the latter into a one-preset list (design §1). No
/// version field — the migration is shape-lossless (single assignment ⊂
/// presets) and old keys drop out on the next save.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudePresetsShadow {
    #[serde(default)]
    presets: Vec<ClaudePreset>,
    #[serde(default)]
    current: Option<String>,
    // Old single-assignment fields (migration input when `presets` is empty).
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    haiku_model: Option<String>,
    #[serde(default)]
    sonnet_model: Option<String>,
    #[serde(default)]
    opus_model: Option<String>,
    #[serde(default)]
    auth_field: Option<String>,
}

impl From<ClaudePresetsShadow> for ClaudePresets {
    fn from(s: ClaudePresetsShadow) -> Self {
        if !s.presets.is_empty() {
            return ClaudePresets {
                presets: s.presets,
                current: s.current,
            };
        }
        // Old shape: hoist the single assignment into one default preset and
        // make it current.
        if let Some(provider) = s.provider.filter(|p| !p.is_empty()) {
            let preset = ClaudePreset {
                id: "default".into(),
                name: default_preset_name(),
                provider,
                model: s.model.unwrap_or_default(),
                haiku_model: s.haiku_model,
                sonnet_model: s.sonnet_model,
                opus_model: s.opus_model,
                auth_field: s.auth_field.unwrap_or_else(default_auth_field),
            };
            return ClaudePresets {
                presets: vec![preset],
                current: Some("default".into()),
            };
        }
        ClaudePresets::default()
    }
}

/// N named codex presets + the currently-selected one (mirror of ClaudePresets).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", from = "CodexPresetsShadow")]
pub struct CodexPresets {
    pub presets: Vec<CodexPreset>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexPreset {
    #[serde(default)]
    pub id: String,
    #[serde(default = "default_preset_name")]
    pub name: String,
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

impl CodexPresets {
    /// The preset `current` points at; None when current is unset or dangling.
    pub fn current_preset(&self) -> Option<&CodexPreset> {
        self.current
            .as_ref()
            .and_then(|id| self.presets.iter().find(|p| &p.id == id))
    }
}

/// Deserialization shadow for codex (see ClaudePresetsShadow).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexPresetsShadow {
    #[serde(default)]
    presets: Vec<CodexPreset>,
    #[serde(default)]
    current: Option<String>,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default)]
    wire_api: Option<String>,
}

impl From<CodexPresetsShadow> for CodexPresets {
    fn from(s: CodexPresetsShadow) -> Self {
        if !s.presets.is_empty() {
            return CodexPresets {
                presets: s.presets,
                current: s.current,
            };
        }
        if let Some(provider) = s.provider.filter(|p| !p.is_empty()) {
            let preset = CodexPreset {
                id: "default".into(),
                name: default_preset_name(),
                provider,
                model: s.model.unwrap_or_default(),
                reasoning_effort: s.reasoning_effort,
                wire_api: s.wire_api.unwrap_or_else(default_wire_api),
            };
            return CodexPresets {
                presets: vec![preset],
                current: Some("default".into()),
            };
        }
        CodexPresets::default()
    }
}

// ── preset id generation (design §2) ──────────────────────────────

/// Generate a short preset id: `preset-<5 hex>` from a nanos+counter-seeded
/// splitmix64 (std-only — no rand dep). Uniqueness within an agent domain is
/// guaranteed by `assign_missing_ids` regenerating on collision.
pub fn gen_preset_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut z = nanos ^ n.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    // splitmix64 finalizer.
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    format!("preset-{:05x}", z & 0xF_FFFF)
}

/// Assign ids to presets that lack one (frontend creates presets without
/// ids; PUT backfills here), keeping ids unique within the slice. Returns
/// the id assigned to the FIRST id-less preset (used to resolve a
/// `current: ""` placeholder).
fn assign_missing_ids<T>(
    presets: &mut [T],
    id_of: impl Fn(&T) -> &str,
    mut set_id: impl FnMut(&mut T, String),
) -> Option<String> {
    let existing: HashSet<String> = presets
        .iter()
        .map(|p| id_of(p))
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    let mut taken = existing;
    let mut first_assigned: Option<String> = None;
    for preset in presets.iter_mut() {
        if id_of(preset).is_empty() {
            let mut id = gen_preset_id();
            while taken.contains(&id) {
                id = gen_preset_id();
            }
            taken.insert(id.clone());
            if first_assigned.is_none() {
                first_assigned = Some(id.clone());
            }
            set_id(preset, id);
        }
    }
    first_assigned
}

/// Backfill preset ids for both preset-style agents (call on every PUT,
/// after merge, before validate — design §2). Also resolves the frontend's
/// `current: ""` placeholder ("the preset I just created") to the new id —
/// the frontend cannot know the backend-generated id when it marks a
/// freshly-added preset current.
pub fn ensure_preset_ids(config: &mut CanonicalConfig) {
    if let Some(presets) = config.agents.claude.as_mut() {
        let first = assign_missing_ids(&mut presets.presets, |p| p.id.as_str(), |p, id| p.id = id);
        if presets.current.as_deref() == Some("") {
            presets.current = first.or_else(|| presets.presets.first().map(|p| p.id.clone()));
        }
    }
    if let Some(presets) = config.agents.codex.as_mut() {
        let first = assign_missing_ids(&mut presets.presets, |p| p.id.as_str(), |p, id| p.id = id);
        if presets.current.as_deref() == Some("") {
            presets.current = first.or_else(|| presets.presets.first().map(|p| p.id.clone()));
        }
    }
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
    #[serde(default)]
    pub models: Vec<ModelEntry>,
}

fn default_api() -> String {
    "openai-completions".to_string()
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
    /// The stored file failed to parse. Message string (not serde_json::Error)
    /// so json5 sources (opencode.jsonc) report corruption the same way.
    Corrupt(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "io: {e}"),
            StoreError::Corrupt(msg) => write!(f, "corrupt: {msg}"),
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
            serde_json::from_str::<CanonicalConfig>(&text)
                .map_err(|e| StoreError::Corrupt(e.to_string()))
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
    if let Some(presets) = &config.agents.claude {
        validate_presets("claude", &presets.presets, &presets.current, &config.providers, &mut errors);
    }
    if let Some(presets) = &config.agents.codex {
        validate_presets("codex", &presets.presets, &presets.current, &config.providers, &mut errors);
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

/// Validate an agent's preset list: every preset's provider/model references
/// must resolve (even non-current presets must not be broken — design §3),
/// ids must be unique, and `current` must not dangle (design §6). Errors
/// carry the preset name so the user can find the offending card.
fn validate_presets<T>(
    agent: &str,
    presets: &[T],
    current: &Option<String>,
    providers: &BTreeMap<String, ProviderEntry>,
    errors: &mut Vec<String>,
) where
    T: PresetRef,
{
    let mut seen: HashSet<&str> = HashSet::new();
    for preset in presets {
        if !seen.insert(preset.id()) {
            errors.push(format!(
                "agent '{}' has duplicate preset id '{}'",
                agent,
                preset.id()
            ));
        }
        if !providers.contains_key(preset.provider()) {
            errors.push(format!(
                "agent '{}' preset '{}' references unknown provider '{}'",
                agent,
                preset.name(),
                preset.provider()
            ));
        } else if !providers[preset.provider()]
            .models
            .iter()
            .any(|m| m.id == preset.model())
        {
            errors.push(format!(
                "agent '{}' preset '{}' model '{}' not found in provider '{}'",
                agent,
                preset.name(),
                preset.model(),
                preset.provider()
            ));
        }
    }
    if let Some(id) = current {
        if !presets.iter().any(|p| p.id() == id.as_str()) {
            errors.push(format!(
                "agent '{}' current preset '{}' not found (dangling)",
                agent, id
            ));
        }
    }
}

/// Read-only view of a preset for the generic validator (ClaudePreset and
/// CodexPreset share the validated fields: id/name/provider/model).
trait PresetRef {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn provider(&self) -> &str;
    fn model(&self) -> &str;
}

impl PresetRef for ClaudePreset {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn provider(&self) -> &str {
        &self.provider
    }
    fn model(&self) -> &str {
        &self.model
    }
}

impl PresetRef for CodexPreset {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn provider(&self) -> &str {
        &self.provider
    }
    fn model(&self) -> &str {
        &self.model
    }
}

// ── pi / opencode import (agent-tab live sync) ─────────────────────

/// Result of importing providers from an agent's native config.
#[derive(Debug)]
pub struct ImportResult {
    pub imported: Vec<String>,
    pub skipped: Vec<String>,
    pub providers: BTreeMap<String, ProviderEntry>,
}

/// Canonical id for an imported provider key: keep it when already
/// kebab-case, else sanitize.
fn provider_id_from_key(key: &str) -> String {
    if is_valid_provider_id(key) {
        key.to_string()
    } else {
        sanitize_id(key)
    }
}

/// Finalize an imported (key, provider) pair: canonical id + name backfill
/// from the original key when the entry carries none. pi and opencode share
/// this (opencode fragments map into ProviderEntry first).
fn map_imported_provider(key: &str, mut provider: ProviderEntry) -> (String, ProviderEntry) {
    let id = provider_id_from_key(key);
    if provider.name.is_empty() {
        provider.name = key.to_string();
    }
    (id, provider)
}

/// Read pi's models.json and map each provider 1:1 into canonical format.
/// - id = pi provider key if already kebab-case, else sanitized
/// - name = original key (when provider.name is empty)
/// - api/baseUrl/apiKey/headers/compat/models carried over
/// - R1: no separate anthropic block — protocol selection alone decides
///   the endpoint, so an anthropic-messages provider carries no override.
/// Providers whose id already exists in `current` are skipped.
pub fn import_from_pi(
    pi_path: &Path,
    current: &CanonicalConfig,
) -> Result<ImportResult, StoreError> {
    import_pi_providers(pi_path, current, None)
}

/// Import only the pi provider that maps to `id` (agent-tab single-row
/// sync). `id` may be the raw pi key (what the live summary lists and what
/// edit/delete target) or the sanitized canonical id. Empty imported+skipped
/// when no provider maps to it — the route turns that into a 404.
pub fn import_pi_provider(
    pi_path: &Path,
    current: &CanonicalConfig,
    id: &str,
) -> Result<ImportResult, StoreError> {
    import_pi_providers(pi_path, current, Some(id))
}

fn import_pi_providers(
    pi_path: &Path,
    current: &CanonicalConfig,
    only: Option<&str>,
) -> Result<ImportResult, StoreError> {
    let text = std::fs::read_to_string(pi_path).map_err(StoreError::Io)?;
    let pi_config: CanonicalConfig = serde_json::from_str(&text)
        .map_err(|e| StoreError::Corrupt(e.to_string()))?;

    let mut imported = Vec::new();
    let mut skipped = Vec::new();
    let mut new_providers: BTreeMap<String, ProviderEntry> = BTreeMap::new();

    for (key, provider) in pi_config.providers {
        let (id, provider) = map_imported_provider(&key, provider);
        if let Some(want) = only {
            // `want` arrives from the frontend as the raw native key (the
            // live summary lists raw keys and edit/delete target them by
            // raw key). Accept either the raw key or the sanitized id so a
            // hand-edited non-kebab key (e.g. "My_Provider") still single-syncs
            // instead of 404-ing while visible in the list.
            if id != want && key != want {
                continue;
            }
        }
        if current.providers.contains_key(&id) || new_providers.contains_key(&id) {
            skipped.push(id);
            continue;
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

/// Read opencode's opencode.jsonc and map each provider fragment into the
/// canonical store — the reverse of render/opencode.rs's fragment builder:
/// - `options.baseURL` → `base_url` (a fragment without one is unmappable,
///   surfaced in `skipped`)
/// - npm containing "anthropic" → api `anthropic-messages`, else
///   `openai-completions` (inverse of the renderer's npm choice)
/// - `options.apiKey`/`options.headers` carried over; fragment.name → name
///   (backfilled from the key when absent)
/// - `models{<id>:{name?}}` → models[] (id + optional display name)
/// Same idempotency contract as import_from_pi: ids already in `current`
/// land in `skipped`.
pub fn import_from_opencode(
    oc_path: &Path,
    current: &CanonicalConfig,
) -> Result<ImportResult, StoreError> {
    import_opencode_providers(oc_path, current, None)
}

/// Import only the opencode provider fragment whose key maps to `id`
/// (agent-tab single-row sync; `id` may be the raw fragment key or the
/// sanitized canonical id — same contract as `import_pi_provider`). Empty
/// imported+skipped when absent.
pub fn import_opencode_provider(
    oc_path: &Path,
    current: &CanonicalConfig,
    id: &str,
) -> Result<ImportResult, StoreError> {
    import_opencode_providers(oc_path, current, Some(id))
}

fn import_opencode_providers(
    oc_path: &Path,
    current: &CanonicalConfig,
    only: Option<&str>,
) -> Result<ImportResult, StoreError> {
    let text = std::fs::read_to_string(oc_path).map_err(StoreError::Io)?;
    // json5 read: opencode.jsonc may carry comments / trailing commas.
    let root: Value = json5::from_str(&text).map_err(|e| StoreError::Corrupt(e.to_string()))?;

    let mut imported = Vec::new();
    let mut skipped = Vec::new();
    let mut new_providers: BTreeMap<String, ProviderEntry> = BTreeMap::new();

    // A missing / non-object `provider` key is not corrupt — nothing to import.
    let Some(fragments) = root.get("provider").and_then(|p| p.as_object()) else {
        return Ok(ImportResult {
            imported,
            skipped,
            providers: new_providers,
        });
    };

    for (key, fragment) in fragments {
        let id = provider_id_from_key(key);
        if let Some(want) = only {
            // Same id-domain contract as import_pi_providers: the frontend
            // sends the raw fragment key from the live summary.
            if id != want && key != want {
                continue;
            }
        }
        let Some(entry) = map_opencode_fragment(fragment) else {
            // Unmappable (no options.baseURL): surfaced as skipped so the UI
            // can say "n skipped" instead of silently dropping it.
            skipped.push(id);
            continue;
        };
        let (id, entry) = map_imported_provider(key, entry);
        if current.providers.contains_key(&id) || new_providers.contains_key(&id) {
            skipped.push(id);
            continue;
        }
        new_providers.insert(id.clone(), entry);
        imported.push(id);
    }

    Ok(ImportResult {
        imported,
        skipped,
        providers: new_providers,
    })
}

/// Convert one opencode provider fragment into a canonical ProviderEntry.
/// None when the fragment has no options.baseURL — without an endpoint
/// there is no meaningful canonical provider.
fn map_opencode_fragment(fragment: &Value) -> Option<ProviderEntry> {
    let base_url = fragment
        .get("options")?
        .get("baseURL")?
        .as_str()?
        .to_string();

    let npm = fragment.get("npm").and_then(|n| n.as_str()).unwrap_or("");
    let api = if npm.contains("anthropic") {
        "anthropic-messages"
    } else {
        "openai-completions"
    }
    .to_string();

    let name = fragment
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();

    let api_key = fragment
        .pointer("/options/apiKey")
        .and_then(|k| k.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);

    let mut headers = BTreeMap::new();
    if let Some(h) = fragment.pointer("/options/headers").and_then(|h| h.as_object()) {
        for (k, v) in h {
            if let Some(s) = v.as_str() {
                headers.insert(k.clone(), s.to_string());
            }
        }
    }

    let mut models = Vec::new();
    if let Some(m) = fragment.get("models").and_then(|m| m.as_object()) {
        for (id, node) in m {
            models.push(ModelEntry {
                id: id.clone(),
                name: node.get("name").and_then(|n| n.as_str()).map(String::from),
                ..Default::default()
            });
        }
    }

    Some(ProviderEntry {
        name,
        base_url,
        api,
        api_key,
        headers,
        compat: Value::Null,
        models,
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

        let a = &result.providers["anthropic-prov"];
        assert_eq!(a.base_url, "https://api.anthropic.com");
        assert_eq!(a.api, "anthropic-messages");
        // R1: no separate anthropic block — protocol selection alone decides
        // the endpoint, so a pi import carries no override.
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

    #[test]
    fn pi_import_single_provider_filters() {
        let dir = temp_dir();
        let pi_path = dir.join("pi_models.json");
        std::fs::write(
            &pi_path,
            r#"{"providers": {
              "prov-a": {"baseUrl": "https://a", "api": "openai-completions"},
              "prov-b": {"baseUrl": "https://b", "api": "openai-completions"}
            }}"#,
        )
        .unwrap();

        // Single-id import: only prov-b comes through.
        let r = import_pi_provider(&pi_path, &CanonicalConfig::default(), "prov-b").unwrap();
        assert_eq!(r.imported, vec!["prov-b"]);
        assert!(r.providers.contains_key("prov-b"));

        // Already in canonical -> skipped (idempotent single sync).
        let mut current = CanonicalConfig::default();
        current.providers.insert(
            "prov-a".into(),
            ProviderEntry {
                name: "A".into(),
                ..Default::default()
            },
        );
        let r = import_pi_provider(&pi_path, &current, "prov-a").unwrap();
        assert!(r.imported.is_empty());
        assert_eq!(r.skipped, vec!["prov-a"]);

        // Unknown id -> empty result (route turns this into 404).
        let r = import_pi_provider(&pi_path, &current, "nope").unwrap();
        assert!(r.imported.is_empty());
        assert!(r.skipped.is_empty());
    }

    #[test]
    fn pi_import_single_accepts_non_kebab_raw_key() {
        // The live summary lists RAW native keys and edit/delete target them
        // by raw key; a hand-edited non-kebab key must single-sync too (not
        // 404) — canonical id is the sanitized form.
        let dir = temp_dir();
        let pi_path = dir.join("pi_models.json");
        std::fs::write(
            &pi_path,
            r#"{"providers": {
              "My_Provider": {"baseUrl": "https://a", "api": "openai-completions"}
            }}"#,
        )
        .unwrap();

        // Sync by the raw key (what the frontend sends from the live list).
        let r = import_pi_provider(&pi_path, &CanonicalConfig::default(), "My_Provider").unwrap();
        assert_eq!(r.imported, vec!["my-provider"]);
        assert!(r.providers.contains_key("my-provider"));

        // Sync by the sanitized canonical id also works.
        std::fs::write(
            &pi_path,
            r#"{"providers": {
              "My_Provider": {"baseUrl": "https://a", "api": "openai-completions"}
            }}"#,
        )
        .unwrap();
        let r = import_pi_provider(&pi_path, &CanonicalConfig::default(), "my-provider").unwrap();
        assert_eq!(r.imported, vec!["my-provider"]);
    }

    // --- opencode import (agent-tab live sync, design §3) ---

    #[test]
    fn opencode_import_maps_fragments() {
        let dir = temp_dir();
        let oc_path = dir.join("opencode.jsonc");
        std::fs::write(
            &oc_path,
            r#"{
  "model": "aruoshui/deepseek-v4-pro",
  "provider": {
    "aruoshui": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "Aruoshui",
      "options": {
        "baseURL": "https://ai.aruoshui.com/v1",
        "apiKey": "sk-real-key-xxxx",
        "headers": { "X-Custom": "yes" }
      },
      "models": {
        "deepseek-v4-pro": { "name": "DeepSeek V4 Pro" },
        "qwen-max": {}
      }
    }
  }
}"#,
        )
        .unwrap();

        let result = import_from_opencode(&oc_path, &CanonicalConfig::default()).unwrap();
        assert_eq!(result.imported, vec!["aruoshui"]);
        assert!(result.skipped.is_empty());

        let p = &result.providers["aruoshui"];
        assert_eq!(p.name, "Aruoshui");
        assert_eq!(p.base_url, "https://ai.aruoshui.com/v1");
        assert_eq!(p.api, "openai-completions");
        assert_eq!(p.api_key.as_deref(), Some("sk-real-key-xxxx"));
        assert_eq!(p.headers.get("X-Custom").map(String::as_str), Some("yes"));
        assert_eq!(p.models.len(), 2);
        assert_eq!(p.models[0].id, "deepseek-v4-pro");
        assert_eq!(p.models[0].name.as_deref(), Some("DeepSeek V4 Pro"));
        assert_eq!(p.models[1].id, "qwen-max");
        assert_eq!(p.models[1].name, None);
    }

    #[test]
    fn opencode_import_anthropic_npm_and_name_backfill() {
        let dir = temp_dir();
        let oc_path = dir.join("opencode.jsonc");
        std::fs::write(
            &oc_path,
            r#"{"provider": {
              "antp": {
                "npm": "@ai-sdk/anthropic",
                "options": { "baseURL": "https://api.anthropic.com" },
                "models": { "claude-3": { "name": "Claude 3" } }
              }
            }}"#,
        )
        .unwrap();

        let result = import_from_opencode(&oc_path, &CanonicalConfig::default()).unwrap();
        let p = &result.providers["antp"];
        // Inverse of the renderer's npm choice.
        assert_eq!(p.api, "anthropic-messages");
        // fragment.name absent -> backfilled from the key.
        assert_eq!(p.name, "antp");
        assert!(p.api_key.is_none());
        assert_eq!(p.models[0].name.as_deref(), Some("Claude 3"));
    }

    #[test]
    fn opencode_import_missing_baseurl_is_skipped() {
        let dir = temp_dir();
        let oc_path = dir.join("opencode.jsonc");
        std::fs::write(
            &oc_path,
            r#"{"provider": {
              "no-endpoint": { "npm": "@ai-sdk/openai-compatible", "name": "No Endpoint" },
              "ok": { "npm": "@ai-sdk/openai-compatible",
                      "options": { "baseURL": "https://ok/v1" } }
            }}"#,
        )
        .unwrap();

        let result = import_from_opencode(&oc_path, &CanonicalConfig::default()).unwrap();
        assert_eq!(result.imported, vec!["ok"]);
        assert_eq!(result.skipped, vec!["no-endpoint"]);
    }

    #[test]
    fn opencode_import_skips_existing_ids() {
        let dir = temp_dir();
        let oc_path = dir.join("opencode.jsonc");
        std::fs::write(
            &oc_path,
            r#"{"provider": {
              "existing": { "options": { "baseURL": "https://x/v1" } }
            }}"#,
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
        let result = import_from_opencode(&oc_path, &current).unwrap();
        assert!(result.imported.is_empty());
        assert_eq!(result.skipped, vec!["existing"]);
    }

    #[test]
    fn opencode_import_single_provider_filters() {
        let dir = temp_dir();
        let oc_path = dir.join("opencode.jsonc");
        std::fs::write(
            &oc_path,
            r#"{"provider": {
              "prov-a": { "options": { "baseURL": "https://a/v1" } },
              "prov-b": { "options": { "baseURL": "https://b/v1" } }
            }}"#,
        )
        .unwrap();

        let r = import_opencode_provider(&oc_path, &CanonicalConfig::default(), "prov-a").unwrap();
        assert_eq!(r.imported, vec!["prov-a"]);

        let r = import_opencode_provider(&oc_path, &CanonicalConfig::default(), "nope").unwrap();
        assert!(r.imported.is_empty());
        assert!(r.skipped.is_empty());
    }

    #[test]
    fn opencode_import_single_accepts_non_kebab_raw_key() {
        let dir = temp_dir();
        let oc_path = dir.join("opencode.jsonc");
        std::fs::write(
            &oc_path,
            r#"{"provider": {
              "Weird Key": { "options": { "baseURL": "https://a/v1" } }
            }}"#,
        )
        .unwrap();

        // Frontend sends the raw fragment key from the live list.
        let r = import_opencode_provider(&oc_path, &CanonicalConfig::default(), "Weird Key").unwrap();
        assert_eq!(r.imported, vec!["weird-key"]);

        // And the sanitized id form.
        std::fs::write(
            &oc_path,
            r#"{"provider": {
              "Weird Key": { "options": { "baseURL": "https://a/v1" } }
            }}"#,
        )
        .unwrap();
        let r = import_opencode_provider(&oc_path, &CanonicalConfig::default(), "weird-key").unwrap();
        assert_eq!(r.imported, vec!["weird-key"]);
    }

    #[test]
    fn opencode_import_missing_file_is_io_not_found() {
        let dir = temp_dir();
        let r = import_from_opencode(&dir.join("nope.jsonc"), &CanonicalConfig::default());
        match r {
            Err(StoreError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {}
            other => panic!("expected Io(NotFound), got {other:?}"),
        }
    }

    #[test]
    fn opencode_import_corrupt_is_corrupt() {
        let dir = temp_dir();
        let oc_path = dir.join("opencode.jsonc");
        std::fs::write(&oc_path, "{{not even json5").unwrap();
        let r = import_from_opencode(&oc_path, &CanonicalConfig::default());
        assert!(matches!(r, Err(StoreError::Corrupt(_))));
    }

    #[test]
    fn opencode_import_json5_comments_and_trailing_commas() {
        let dir = temp_dir();
        let oc_path = dir.join("opencode.jsonc");
        std::fs::write(
            &oc_path,
            r#"{
  // user-maintained config with comments
  "theme": "dark",
  "provider": {
    "commented": { // inner comment
      "npm": "@ai-sdk/openai-compatible",
      "options": { "baseURL": "https://c/v1", },
    },
  },
}"#,
        )
        .unwrap();

        let result = import_from_opencode(&oc_path, &CanonicalConfig::default()).unwrap();
        assert_eq!(result.imported, vec!["commented"]);
        assert_eq!(result.providers["commented"].base_url, "https://c/v1");
    }

    // --- preset shape migration (design §1 shadow) ---

    #[test]
    fn old_single_claude_assignment_migrates_to_default_preset() {
        let json = r#"{
  "agents": {
    "claude": {
      "provider": "aruoshui",
      "model": "claude-sonnet-4",
      "haikuModel": "claude-haiku-4",
      "authField": "API_KEY"
    }
  }
}"#;
        let cfg: CanonicalConfig = serde_json::from_str(json).unwrap();
        let claude = cfg.agents.claude.as_ref().unwrap();
        assert_eq!(claude.presets.len(), 1);
        assert_eq!(claude.current.as_deref(), Some("default"));
        let p = &claude.presets[0];
        assert_eq!(p.id, "default");
        assert_eq!(p.provider, "aruoshui");
        assert_eq!(p.model, "claude-sonnet-4");
        assert_eq!(p.haiku_model.as_deref(), Some("claude-haiku-4"));
        assert_eq!(p.auth_field, "API_KEY");
    }

    #[test]
    fn old_single_codex_assignment_migrates_with_defaults() {
        let json = r#"{
  "agents": {
    "codex": {"provider": "prov", "model": "m1"}
  }
}"#;
        let cfg: CanonicalConfig = serde_json::from_str(json).unwrap();
        let codex = cfg.agents.codex.as_ref().unwrap();
        assert_eq!(codex.presets.len(), 1);
        assert_eq!(codex.current.as_deref(), Some("default"));
        let p = &codex.presets[0];
        // Defaults applied for fields the old shape omitted.
        assert_eq!(p.name, "默认配置");
        assert_eq!(p.wire_api, "responses");
        assert!(p.reasoning_effort.is_none());
    }

    #[test]
    fn old_shape_missing_model_still_migrates() {
        let json = r#"{"agents": {"claude": {"provider": "prov"}}}"#;
        let cfg: CanonicalConfig = serde_json::from_str(json).unwrap();
        let claude = cfg.agents.claude.as_ref().unwrap();
        assert_eq!(claude.presets.len(), 1);
        assert_eq!(claude.presets[0].model, "");
    }

    #[test]
    fn empty_presets_with_old_provider_migrates() {
        // New-shape empty array + old provider field: presets wins only when
        // non-empty, so the old provider still migrates (design §1).
        let json = r#"{
  "agents": {
    "claude": {"presets": [], "provider": "prov", "model": "m1"}
  }
}"#;
        let cfg: CanonicalConfig = serde_json::from_str(json).unwrap();
        let claude = cfg.agents.claude.as_ref().unwrap();
        assert_eq!(claude.presets.len(), 1);
        assert_eq!(claude.presets[0].provider, "prov");
    }

    #[test]
    fn empty_agent_block_becomes_empty_presets() {
        let json = r#"{"agents": {"claude": {}, "codex": {}}}"#;
        let cfg: CanonicalConfig = serde_json::from_str(json).unwrap();
        let claude = cfg.agents.claude.as_ref().unwrap();
        assert!(claude.presets.is_empty());
        assert!(claude.current.is_none());
    }

    #[test]
    fn new_shape_round_trips() {
        let json = r#"{
  "agents": {
    "claude": {
      "presets": [
        {"id": "preset-aaa", "name": "工作", "provider": "p1", "model": "m1"},
        {"id": "preset-bbb", "name": "备用", "provider": "p2", "model": "m2",
         "authField": "API_KEY"}
      ],
      "current": "preset-bbb"
    }
  }
}"#;
        let cfg: CanonicalConfig = serde_json::from_str(json).unwrap();
        let claude = cfg.agents.claude.as_ref().unwrap();
        assert_eq!(claude.presets.len(), 2);
        assert_eq!(claude.current_preset().unwrap().name, "备用");

        // Round-trip: serialize keeps only the new shape (old fields gone).
        let text = serde_json::to_string(&cfg).unwrap();
        let back: CanonicalConfig = serde_json::from_str(&text).unwrap();
        assert_eq!(
            back.agents.claude.as_ref().unwrap().presets.len(),
            2
        );
        assert!(!text.contains("\"provider\":\"p1\",\"model\":\"m1\"") || text.contains("presets"));
    }

    #[test]
    fn old_residual_keys_are_dropped() {
        // Pre-R1 residual keys (e.g. an `anthropic` block) must not break
        // deserialization and vanish on the next save.
        let json = r#"{
  "agents": {
    "claude": {"provider": "prov", "model": "m1", "anthropic": {"baseUrl": "https://x"}}
  }
}"#;
        let cfg: CanonicalConfig = serde_json::from_str(json).unwrap();
        let text = serde_json::to_string(&cfg).unwrap();
        assert!(!text.contains("anthropic"));
    }

    // --- preset id generation (design §2) ---

    #[test]
    fn gen_preset_id_format() {
        let id = gen_preset_id();
        assert!(id.starts_with("preset-"), "got {id}");
        assert_eq!(id.len(), "preset-".len() + 5);
        assert!(id["preset-".len()..]
            .chars()
            .all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn ensure_preset_ids_resolves_empty_current_placeholder() {
        // The frontend marks a freshly-added (id-less) preset current by
        // sending current:"" — resolve it to the backfilled id (design §2).
        let mut cfg = CanonicalConfig::default();
        cfg.agents.claude = Some(ClaudePresets {
            presets: vec![ClaudePreset {
                id: String::new(),
                name: "新".into(),
                provider: "p".into(),
                model: "m".into(),
                ..Default::default()
            }],
            current: Some(String::new()),
        });
        ensure_preset_ids(&mut cfg);
        let block = cfg.agents.claude.as_ref().unwrap();
        let new_id = &block.presets[0].id;
        assert!(!new_id.is_empty());
        assert_eq!(block.current.as_deref(), Some(new_id.as_str()));
    }

    #[test]
    fn ensure_preset_ids_backfills_and_avoids_collisions() {
        let mut cfg = CanonicalConfig::default();
        cfg.agents.claude = Some(ClaudePresets {
            presets: vec![
                ClaudePreset {
                    id: String::new(), // no id -> backfilled
                    name: "A".into(),
                    provider: "p".into(),
                    model: "m".into(),
                    ..Default::default()
                },
                ClaudePreset {
                    id: "preset-fixed".into(),
                    name: "B".into(),
                    provider: "p".into(),
                    model: "m".into(),
                    ..Default::default()
                },
                ClaudePreset {
                    id: String::new(), // second backfill must differ from the first
                    name: "C".into(),
                    provider: "p".into(),
                    model: "m".into(),
                    ..Default::default()
                },
            ],
            current: None,
        });
        ensure_preset_ids(&mut cfg);
        let presets = &cfg.agents.claude.as_ref().unwrap().presets;
        assert!(!presets[0].id.is_empty());
        assert_eq!(presets[1].id, "preset-fixed");
        assert!(!presets[2].id.is_empty());
        assert_ne!(presets[0].id, presets[2].id);
        // All ids pairwise unique.
        let ids: HashSet<&str> = presets.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids.len(), 3);
    }

    // --- validate over presets ---

    /// Provider with model m1, for validate tests.
    fn one_provider_config() -> CanonicalConfig {
        let mut cfg = CanonicalConfig::default();
        cfg.providers.insert(
            "prov".into(),
            ProviderEntry {
                name: "P".into(),
                base_url: "https://x".into(),
                api: "openai-completions".into(),
                models: vec![ModelEntry {
                    id: "m1".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        cfg
    }

    #[test]
    fn validate_non_current_preset_unknown_provider_reports_name() {
        let mut cfg = one_provider_config();
        cfg.agents.codex = Some(CodexPresets {
            presets: vec![
                CodexPreset {
                    id: "a".into(),
                    name: "好的".into(),
                    provider: "prov".into(),
                    model: "m1".into(),
                    ..Default::default()
                },
                CodexPreset {
                    id: "b".into(),
                    name: "坏的".into(),
                    provider: "ghost".into(),
                    model: "m1".into(),
                    ..Default::default()
                },
            ],
            current: Some("a".into()),
        });
        let errs = validate(&cfg).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| e.contains("preset '坏的'") && e.contains("ghost")),
            "errors: {errs:?}"
        );
    }

    #[test]
    fn validate_dangling_current_rejected() {
        let mut cfg = one_provider_config();
        cfg.agents.claude = Some(ClaudePresets {
            presets: vec![ClaudePreset {
                id: "a".into(),
                name: "A".into(),
                provider: "prov".into(),
                model: "m1".into(),
                ..Default::default()
            }],
            current: Some("deleted-id".into()),
        });
        let errs = validate(&cfg).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("dangling")));

        // Also: presets empty + current set.
        cfg.agents.claude.as_mut().unwrap().presets.clear();
        let errs = validate(&cfg).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("dangling")));
    }

    #[test]
    fn validate_duplicate_preset_id_rejected() {
        let mut cfg = one_provider_config();
        let preset = ClaudePreset {
            id: "dup".into(),
            name: "A".into(),
            provider: "prov".into(),
            model: "m1".into(),
            ..Default::default()
        };
        cfg.agents.claude = Some(ClaudePresets {
            presets: vec![preset.clone(), preset],
            current: Some("dup".into()),
        });
        let errs = validate(&cfg).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("duplicate preset id 'dup'")));
    }

    #[test]
    fn validate_multi_preset_current_none_ok() {
        // Presets present but no current yet: valid config (user is
        // mid-setup); apply is what refuses (design §6).
        let mut cfg = one_provider_config();
        cfg.agents.claude = Some(ClaudePresets {
            presets: vec![ClaudePreset {
                id: "a".into(),
                name: "A".into(),
                provider: "prov".into(),
                model: "m1".into(),
                ..Default::default()
            }],
            current: None,
        });
        assert!(validate(&cfg).is_ok());
    }
}
