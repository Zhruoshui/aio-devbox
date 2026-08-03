// Scenario discovery + metadata. Both `tui` (listing) and `gen` (fragment
// resolution) scan `scenarios/<id>/scenario.toml` through this single owner
// (cross-layer-thinking-guide: one decoder for the scenario payload).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

/// One selectable dev scenario, declared in `scenarios/<id>/scenario.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct ScenarioMeta {
    pub id: String,
    pub name: String,
    pub description: String,
}

/// A discovered scenario: its metadata plus the path to its fragment.
#[derive(Debug, Clone)]
pub struct Scenario {
    pub meta: ScenarioMeta,
    /// Absolute path to `scenarios/<id>/fragment.Dockerfile`.
    pub fragment: PathBuf,
}

/// Scan `<dir>/*/scenario.toml` and return all scenarios, sorted by id for
/// deterministic TUI listing and gen assembly (design §2.4).
///
/// Errors if a scenario.toml is unparseable, if `id` != directory name (the id
/// must match its directory so gen resolves fragments by id), or if the
/// fragment.Dockerfile is missing.
pub fn scan(dir: &Path) -> Result<Vec<Scenario>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("read scenarios dir {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let toml_path = path.join("scenario.toml");
        if !toml_path.is_file() {
            continue;
        }
        let raw = fs::read_to_string(&toml_path)
            .with_context(|| format!("read {}", toml_path.display()))?;
        let meta: ScenarioMeta = toml::from_str(&raw)
            .with_context(|| format!("parse {}", toml_path.display()))?;
        let dir_name = entry.file_name().to_string_lossy().into_owned();
        if meta.id != dir_name {
            bail!(
                "{}: scenario id {:?} must match its directory name {:?}",
                toml_path.display(),
                meta.id,
                dir_name
            );
        }
        let fragment = path.join("fragment.Dockerfile");
        if !fragment.is_file() {
            bail!(
                "{}: missing fragment.Dockerfile next to scenario.toml",
                path.display()
            );
        }
        out.push(Scenario { meta, fragment });
    }
    out.sort_by(|a, b| a.meta.id.cmp(&b.meta.id));
    Ok(out)
}
