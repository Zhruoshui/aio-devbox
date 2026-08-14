// Selection manifest (`.aio/enabled.toml`). The TUI writes it; `gen` reads it.
// One struct, one (de)serializer - the single owner of the manifest contract
// on both sides (cross-layer-thinking-guide).

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Enabled {
    /// Scenario ids to bake in. Applied in alphabetical order by `gen`
    /// (design §2.4) for reproducible output regardless of selection order.
    /// Always-on scenarios (L1 node/python) are NOT listed here - `gen`
    /// includes them unconditionally; only their version selection lives in
    /// `versions` below.
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
