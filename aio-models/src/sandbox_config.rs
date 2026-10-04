// Sandbox agent config — the top-level config stored in each sandbox.
//
// This replaces the old CanonicalConfig's `agents` block. Each sandbox owns
// its own independent agent configuration (纯 sandbox 内部配置). The mgr's
// provider library serves only as a template source (方案A: 模板复制).

use serde::{Deserialize, Serialize};

use crate::incremental::IncrementalAgentConfig;
use crate::snapshot::SwitchAgentConfig;

/// Top-level agent config for one sandbox. Stored in the sandbox's
//  `~/.aio/models/sandbox_config.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxAgentConfig {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude: Option<SwitchAgentConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex: Option<SwitchAgentConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pi: Option<IncrementalAgentConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opencode: Option<IncrementalAgentConfig>,
}

fn default_version() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::incremental::IncrementalAgentConfig;
    use crate::snapshot::SwitchAgentConfig;
    use serde_json::json;

    #[test]
    fn sandbox_agent_config_round_trips() {
        let mut config = SandboxAgentConfig::default();

        let mut claude = SwitchAgentConfig::default();
        claude.providers.push(crate::snapshot::ProviderSnapshot {
            id: "snap-1".into(),
            name: "Moonshot".into(),
            settings_config: json!({"env": {}}),
            created_at: None,
            notes: None,
        });
        claude.current = Some("snap-1".into());
        config.claude = Some(claude);

        let mut pi = IncrementalAgentConfig::default();
        pi.providers.insert(
            "moonshot".into(),
            crate::incremental::ProviderNode {
                name: "Moonshot".into(),
                base_url: "https://api.moonshot.cn/v1".into(),
                api: "openai-completions".into(),
                api_key: None,
                headers: Default::default(),
                compat: None,
                models: vec![],
            },
        );
        config.pi = Some(pi);

        let json_str = serde_json::to_string_pretty(&config).unwrap();
        let back: SandboxAgentConfig = serde_json::from_str(&json_str).unwrap();

        assert!(back.claude.is_some());
        assert!(back.pi.is_some());
        assert_eq!(back.claude.unwrap().current.as_deref(), Some("snap-1"));
        assert_eq!(back.pi.unwrap().providers.len(), 1);
    }

    #[test]
    fn empty_config_is_valid() {
        let config = SandboxAgentConfig::default();
        let json = serde_json::to_value(&config).unwrap();
        assert!(json.get("claude").is_none());
        assert!(json.get("pi").is_none());
    }
}
