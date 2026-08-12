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
    /// Profile layer this scenario belongs to: "os" (L1) / "shell" (L2) /
    /// "lang" (L3) / "app" (L4) / "service" (L5, future). Free-form string so
    /// the TUI/gen never break on a category they don't know yet; the TUI
    /// groups rows by `category_rank` and renders unknown categories last.
    /// L1 ("os") lives in Dockerfile.base.head and is NOT a selectable
    /// scenario, so it never reaches the TUI. Defaults to "lang" so
    /// scenario.toml files predating the layer field still parse.
    #[serde(default = "default_category")]
    pub category: String,
}

/// Default category when a scenario.toml omits `category` (back-compat with
/// pre-layer scenario files, which are all language toolchains).
fn default_category() -> String {
    "lang".to_string()
}

/// Canonical display order of profile layers. Categories not listed here sort
/// last (by `category` then id) so adding a new layer never reorders known ones.
const CATEGORY_ORDER: &[&str] = &["os", "shell", "lang", "app", "service"];

/// Sort key for a category. Lower = earlier in the TUI. Unknown -> usize::MAX.
pub fn category_rank(category: &str) -> usize {
    CATEGORY_ORDER.iter().position(|c| *c == category).unwrap_or(usize::MAX)
}

/// Human-readable header for a category group in the TUI.
pub fn category_title(category: &str) -> String {
    match category {
        "os" => "L1 · 操作系统 / 基础环境".to_string(),
        "shell" => "L2 · Shell 命令".to_string(),
        "lang" => "L3 · 语言开发链路".to_string(),
        "app" => "L4 · 应用 / AI agent".to_string(),
        "service" => "L5 · 外部服务".to_string(),
        other => format!("· {}", other),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_defaults_to_lang_when_omitted() {
        // A pre-layer scenario.toml (no `category` field) must still parse and
        // default to "lang" so existing files survive the layer rollout.
        let raw = r#"
            id = "rust"
            name = "Rust"
            description = "rust toolchain"
        "#;
        let meta: ScenarioMeta = toml::from_str(raw).unwrap();
        assert_eq!(meta.category, "lang");
    }

    #[test]
    fn category_field_is_honored() {
        let raw = r#"
            id = "shell-utils"
            name = "Shell"
            description = "shell layer"
            category = "shell"
        "#;
        let meta: ScenarioMeta = toml::from_str(raw).unwrap();
        assert_eq!(meta.category, "shell");
    }

    #[test]
    fn category_rank_orders_known_layers_and_pushes_unknown_last() {
        assert!(category_rank("shell") < category_rank("lang"));
        assert!(category_rank("lang") < category_rank("app"));
        assert_eq!(category_rank("os"), 0);
        // Unknown categories sort after every known layer.
        assert!(category_rank("lang") < category_rank("future-layer"));
    }
}
