// Provider template — the mgr-level provider library entry.
//
// A ProviderTemplate is a reusable provider definition stored in the mgr's
// provider library (`models.json`). It serves as a TEMPLATE source: when a
// user configures an agent in a sandbox, they can pick a template and copy it
// into the sandbox's own agent config (方案A: 模板复制, 拷贝后独立).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// One provider template in the mgr's provider library.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTemplate {
    /// Display name (e.g. "Moonshot AI", "OpenRouter").
    pub name: String,
    /// API base URL (e.g. "https://api.moonshot.cn/v1").
    pub base_url: String,
    /// API protocol: "openai-completions" | "openai-responses" | "anthropic-messages".
    pub api: String,
    /// Optional API key (masked when returned by the API).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Extra HTTP headers (rarely needed).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    /// Optional model list. Can be manually maintained or dynamically fetched.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<TemplateModelEntry>,
    /// Provider website URL (for reference).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website_url: Option<String>,
    /// Category: "official" | "cn_official" | "aggregator" | "third_party" | "custom".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Free-form notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// A model entry in a provider template.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateModelEntry {
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
    pub cost: Option<TemplateCost>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateCost {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<f64>,
}

/// The full provider library stored at the mgr level.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderLibrary {
    #[serde(default = "default_version")]
    pub version: u32,
    /// Provider templates keyed by id (e.g. "moonshot", "openrouter").
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderTemplate>,
}

fn default_version() -> u32 {
    1
}

/// Mask an apiKey for API responses (show first 4 + last 4 chars).
pub fn mask_api_key(key: &str) -> String {
    if key.len() <= 8 {
        return "****".to_string();
    }
    format!("{}****{}", &key[..4], &key[key.len() - 4..])
}

/// Mask all apiKeys in a library for API responses.
pub fn mask_library(library: &mut ProviderLibrary) {
    for template in library.providers.values_mut() {
        if let Some(key) = &template.api_key {
            template.api_key = Some(mask_api_key(key));
        }
    }
}

impl Default for ProviderTemplate {
    fn default() -> Self {
        Self {
            name: String::new(),
            base_url: String::new(),
            api: "openai-completions".into(),
            api_key: None,
            headers: BTreeMap::new(),
            models: Vec::new(),
            website_url: None,
            category: None,
            notes: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_template_serializes_camel_case() {
        let t = ProviderTemplate {
            name: "Moonshot".into(),
            base_url: "https://api.moonshot.cn/v1".into(),
            api: "openai-completions".into(),
            api_key: Some("sk-xxx".into()),
            headers: BTreeMap::new(),
            models: vec![TemplateModelEntry {
                id: "kimi-k2".into(),
                name: Some("Kimi K2".into()),
                reasoning: Some(true),
                context_window: Some(131_072),
                max_tokens: Some(8_192),
                cost: Some(TemplateCost {
                    input: Some(0.5),
                    output: Some(2.0),
                    cache_read: None,
                    cache_write: None,
                }),
            }],
            website_url: Some("https://moonshot.cn".into()),
            category: Some("cn_official".into()),
            notes: None,
        };
        let json = serde_json::to_value(&t).unwrap();
        assert_eq!(json["baseUrl"], "https://api.moonshot.cn/v1");
        assert_eq!(json["apiKey"], "sk-xxx");
        assert_eq!(json["websiteUrl"], "https://moonshot.cn");
    }

    #[test]
    fn mask_api_key_short_and_long() {
        assert_eq!(mask_api_key("sk-abc"), "****");
        assert_eq!(mask_api_key("sk-abcdef1234567890"), "sk-a****7890");
    }

    #[test]
    fn mask_library_masks_all_keys() {
        let mut lib = ProviderLibrary::default();
        lib.providers.insert(
            "test".into(),
            ProviderTemplate {
                name: "Test".into(),
                base_url: "https://test.com".into(),
                api: "openai-completions".into(),
                api_key: Some("sk-test-key-123456".into()),
                ..Default::default()
            },
        );
        mask_library(&mut lib);
        assert_eq!(
            lib.providers["test"].api_key.as_deref(),
            Some("sk-t****3456")
        );
    }
}
