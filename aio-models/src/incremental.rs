// Incremental agent config — the sandbox-level config for pi/opencode.
//
// For incremental agents (pi, opencode), ALL providers coexist in the agent's
// native config file. "Switching" means changing the default provider/model
// pointer (cc-switch's "增量式" mode). No config file replacement happens.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Incremental agent config: a map of provider nodes + the default pointer.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncrementalAgentConfig {
    /// Provider nodes keyed by id (e.g. "moonshot", "openrouter").
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderNode>,
    /// Currently default provider id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_provider: Option<String>,
    /// Currently default model id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
}

/// One provider node in an incremental agent's config.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderNode {
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// API base URL.
    pub base_url: String,
    /// API protocol: "openai-completions" | "openai-responses" | "anthropic-messages".
    pub api: String,
    /// Optional API key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Extra HTTP headers.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    /// pi-specific compat blob.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compat: Option<serde_json::Value>,
    /// Models available on this provider.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<IncrementalModelEntry>,
}

/// A model entry in an incremental provider node.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncrementalModelEntry {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<IncrementalCost>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncrementalCost {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<f64>,
}

impl IncrementalAgentConfig {
    /// Set the default provider/model pointer. Validates the provider exists.
    pub fn set_default(&mut self, provider: &str, model: &str) -> Result<(), String> {
        let node = self
            .providers
            .get(provider)
            .ok_or_else(|| format!("provider '{provider}' not found"))?;
        if !node.models.iter().any(|m| m.id == model) {
            return Err(format!("model '{model}' not found in provider '{provider}'"));
        }
        self.default_provider = Some(provider.to_string());
        self.default_model = Some(model.to_string());
        Ok(())
    }

    /// All (provider_id, model_id) pairs for picker UIs.
    pub fn all_model_pairs(&self) -> Vec<(&str, &str)> {
        self.providers
            .iter()
            .flat_map(|(pid, node)| {
                node.models
                    .iter()
                    .map(move |m| (pid.as_str(), m.id.as_str()))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                    reasoning: Some(true),
                    context_window: Some(128_000),
                    max_tokens: None,
                    cost: None,
                },
            ],
        }
    }

    #[test]
    fn incremental_config_round_trips() {
        let mut config = IncrementalAgentConfig::default();
        config.providers.insert("prov-a".into(), sample_node("Provider A"));
        config.providers.insert("prov-b".into(), sample_node("Provider B"));
        config.default_provider = Some("prov-a".into());
        config.default_model = Some("model-a".into());

        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["defaultProvider"], "prov-a");
        assert_eq!(json["defaultModel"], "model-a");

        let back: IncrementalAgentConfig = serde_json::from_value(json).unwrap();
        assert_eq!(back.providers.len(), 2);
        assert_eq!(back.default_provider.as_deref(), Some("prov-a"));
    }

    #[test]
    fn set_default_validates_provider_and_model() {
        let mut config = IncrementalAgentConfig::default();
        config.providers.insert("prov-a".into(), sample_node("A"));

        assert!(config.set_default("prov-a", "model-a").is_ok());
        assert_eq!(config.default_provider.as_deref(), Some("prov-a"));
        assert_eq!(config.default_model.as_deref(), Some("model-a"));

        assert!(config.set_default("zzz", "model-a").is_err());
        assert!(config.set_default("prov-a", "zzz").is_err());
    }

    #[test]
    fn all_model_pairs_lists_everything() {
        let mut config = IncrementalAgentConfig::default();
        config.providers.insert("a".into(), sample_node("A"));
        config.providers.insert("b".into(), sample_node("B"));

        let pairs = config.all_model_pairs();
        assert_eq!(pairs.len(), 4);
        assert!(pairs.contains(&("a", "model-a")));
        assert!(pairs.contains(&("b", "model-b")));
    }
}
