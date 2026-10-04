// Provider snapshot — the sandbox-level switch-style agent config entry.
//
// For switch-style agents (claude, codex), each provider is a COMPLETE config
// snapshot. Switching providers means atomically replacing the agent's native
// config file with the snapshot's contents (cc-switch's "切换式" mode).
//
// `settings_config` is an opaque JSON blob whose shape depends on the agent:
//   - claude: full ~/.claude/settings.json content (env, permissions, hooks, ...)
//   - codex:  { "config": "<config.toml string>", "auth": { ... auth.json ... } }

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One provider snapshot for a switch-style agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSnapshot {
    /// Unique id (backend-generated, e.g. "snap-a1b2c3d4").
    pub id: String,
    /// Display name (e.g. "Moonshot Kimi", "OpenRouter GPT-4o").
    pub name: String,
    /// The complete agent-specific config payload (opaque JSON).
    pub settings_config: Value,
    /// Creation timestamp (Unix seconds).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    /// Free-form notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// Switch-style agent config: a list of snapshots + the currently active one.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchAgentConfig {
    #[serde(default)]
    pub providers: Vec<ProviderSnapshot>,
    /// Id of the currently active snapshot. None = no provider selected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
}

impl SwitchAgentConfig {
    /// Find a snapshot by id.
    pub fn find(&self, id: &str) -> Option<&ProviderSnapshot> {
        self.providers.iter().find(|p| p.id == id)
    }

    /// Find a mutable snapshot by id.
    pub fn find_mut(&mut self, id: &str) -> Option<&mut ProviderSnapshot> {
        self.providers.iter_mut().find(|p| p.id == id)
    }

    /// Remove a snapshot by id. Returns true if found and removed.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.providers.len();
        self.providers.retain(|p| p.id != id);
        self.providers.len() < before
    }

    /// Set the current snapshot. Validates the id exists.
    pub fn set_current(&mut self, id: &str) -> Result<(), String> {
        if self.find(id).is_none() {
            return Err(format!("snapshot '{id}' not found"));
        }
        self.current = Some(id.to_string());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_snapshot(id: &str, name: &str) -> ProviderSnapshot {
        ProviderSnapshot {
            id: id.into(),
            name: name.into(),
            settings_config: json!({"env": {"ANTHROPIC_MODEL": "kimi-k2"}}),
            created_at: Some(1728000000),
            notes: None,
        }
    }

    #[test]
    fn switch_agent_config_round_trips() {
        let mut config = SwitchAgentConfig::default();
        config.providers.push(sample_snapshot("a", "Provider A"));
        config.providers.push(sample_snapshot("b", "Provider B"));
        config.current = Some("a".into());

        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["providers"].as_array().unwrap().len(), 2);
        assert_eq!(json["current"], "a");

        let back: SwitchAgentConfig = serde_json::from_value(json).unwrap();
        assert_eq!(back.providers.len(), 2);
        assert_eq!(back.current.as_deref(), Some("a"));
    }

    #[test]
    fn find_and_remove_snapshot() {
        let mut config = SwitchAgentConfig::default();
        config.providers.push(sample_snapshot("a", "A"));
        config.providers.push(sample_snapshot("b", "B"));

        assert!(config.find("a").is_some());
        assert!(config.find("zzz").is_none());

        assert!(config.remove("a"));
        assert_eq!(config.providers.len(), 1);
        assert!(!config.remove("a"));
    }

    #[test]
    fn set_current_validates_id() {
        let mut config = SwitchAgentConfig::default();
        config.providers.push(sample_snapshot("a", "A"));

        assert!(config.set_current("a").is_ok());
        assert_eq!(config.current.as_deref(), Some("a"));

        assert!(config.set_current("zzz").is_err());
        assert_eq!(config.current.as_deref(), Some("a"));
    }
}
