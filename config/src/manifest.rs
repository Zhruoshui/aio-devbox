// Selection manifest (`.aio/enabled.toml`). The TUI writes it; `gen` reads it.
// One struct, one (de)serializer - the single owner of the manifest contract
// on both sides (cross-layer-thinking-guide).

use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::scenario;

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Enabled {
    /// Scenario ids to bake in. Applied in alphabetical order by `gen`
    /// (design §2.4) for reproducible output regardless of selection order.
    /// Always-on scenarios (node/python + pi/pi-web) are NOT listed here -
    /// `gen` includes them unconditionally; only their version selection
    /// lives in `versions` below.
    pub scenarios: Vec<String>,

    /// Version selections for versioned scenarios (always_on or not). Each
    /// entry picks a version `label` for a scenario id; `gen` resolves the
    /// label back to the full version entry (with template vars) in
    /// scenario.toml. Default empty: a manifest predating versioning, or a
    /// fresh checkout, makes `gen` fall back to each scenario's
    /// `default_version` (then `versions[0]`).
    #[serde(default)]
    pub versions: Vec<VersionSelect>,
}

/// A version selection in the manifest: which `label` is chosen for a given
/// scenario id. The TUI writes one per versioned scenario; `gen` reads them to
/// substitute `{{key}}` placeholders in the scenario's fragment.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VersionSelect {
    pub id: String,
    pub label: String,
}

impl Enabled {
    /// Expand a `"*"` wildcard in `scenarios` to the concrete ids of every
    /// discovered, non-always_on scenario. Always-on scenarios are baked
    /// unconditionally by `gen`, so the wildcard must NOT list them - keeping
    /// that exclusion here, in the manifest owner, means CI only needs to write
    /// `scenarios = ["*"]` and never duplicates the always_on rule.
    ///
    /// Rules (design §2.2, PRD R2):
    ///   - `"*"` as the ONLY element expands to all discovered non-always_on ids
    ///     (in discovery order; `gen` re-sorts by layer afterwards).
    ///   - `"*"` mixed with explicit ids (e.g. `["*", "rust"]`) is an error - a
    ///     clear contract beats lenient dedup.
    ///   - No `"*"`: returns `self.scenarios` verbatim (a clone), so the
    ///     pre-wildcard flow is byte-identical and `gen` stays idempotent (AC3).
    pub fn expand(&self, discovered: &[scenario::Scenario]) -> Result<Vec<String>> {
        if self.scenarios.iter().any(|id| id == "*") {
            if self.scenarios.len() != 1 {
                bail!(
                    "manifest scenarios: wildcard \"*\" must be the only element (got {})",
                    self.scenarios.join(", ")
                );
            }
            return Ok(discovered
                .iter()
                .filter(|s| !s.meta.always_on)
                .map(|s| s.meta.id.clone())
                .collect());
        }
        Ok(self.scenarios.clone())
    }
}

/// Load the manifest. A missing file = no scenarios enabled (default), NOT an
/// error: a fresh checkout has no `.aio/enabled.toml` until `make config` runs,
/// and `gen` must still produce a valid (scenario-less) Dockerfile.base.
pub fn load(path: &Path) -> Result<Enabled> {
    match fs::read_to_string(path) {
        Ok(raw) => {
            let e: Enabled = toml::from_str(&raw)
                .with_context(|| format!("parse manifest {}", path.display()))?;
            Ok(e)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Enabled::default()),
        Err(e) => Err(e).with_context(|| format!("read manifest {}", path.display())),
    }
}

/// Save the manifest as pretty TOML, creating the parent dir if needed (so
/// `aio-config tui` works even if `.aio/` does not yet exist).
pub fn save(path: &Path, enabled: &Enabled) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    let raw = toml::to_string_pretty(enabled).context("serialize manifest")?;
    fs::write(path, raw).with_context(|| format!("write manifest {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Build a discovered scenario stub. The fragment path is unused by
    /// `expand`; only `meta.id` and `meta.always_on` matter.
    fn scenario(id: &str, always_on: bool) -> scenario::Scenario {
        scenario::Scenario {
            meta: scenario::ScenarioMeta {
                id: id.to_string(),
                name: id.to_string(),
                description: String::new(),
                category: "lang".to_string(),
                always_on,
                default_version: None,
                versions: Vec::new(),
            },
            fragment: PathBuf::new(),
        }
    }

    fn enabled(scenarios: &[&str]) -> Enabled {
        Enabled {
            scenarios: scenarios.iter().map(|s| s.to_string()).collect(),
            versions: Vec::new(),
        }
    }

    #[test]
    fn wildcard_expands_to_all_non_always_on() {
        // node/python are always_on (baked unconditionally); the wildcard must
        // expand to exactly the selectable scenarios, never the always_on ones.
        let discovered = vec![
            scenario("node", true),
            scenario("rust", false),
            scenario("go", false),
            scenario("python", true),
            scenario("shell-utils", false),
        ];
        let ids = enabled(&["*"]).expand(&discovered).unwrap();
        assert_eq!(ids, vec!["rust", "go", "shell-utils"]);
    }

    #[test]
    fn wildcard_mixed_with_explicit_ids_bails() {
        // "*" + explicit ids is a contract violation: error, not lenient dedup.
        assert!(enabled(&["*", "rust"]).expand(&[]).is_err());
        assert!(enabled(&["rust", "*"]).expand(&[]).is_err());
    }

    #[test]
    fn no_wildcard_returns_manifest_verbatim() {
        // Explicit selections must be returned unchanged (clone, same order) so
        // the pre-wildcard flow is byte-identical and gen stays idempotent.
        let e = enabled(&["rust", "go"]);
        let ids = e.expand(&[]).unwrap();
        assert_eq!(ids, vec!["rust", "go"]);
        // The source list is untouched (borrowed, cloned out).
        assert_eq!(e.scenarios, vec!["rust".to_string(), "go".to_string()]);
    }

    #[test]
    fn wildcard_with_empty_discovery_expands_to_empty() {
        // A scenarios/ dir with no discovered scenarios => "*" yields the empty
        // selection (pure head+tail baseline), a legal manifest.
        assert!(enabled(&["*"]).expand(&[]).unwrap().is_empty());
    }
}
