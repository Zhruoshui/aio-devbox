// Scenario discovery + metadata. Both `tui` (listing) and `gen` (fragment
// resolution) scan `scenarios/<id>/scenario.toml` through this single owner
// (cross-layer-thinking-guide: one decoder for the scenario payload).

use std::collections::HashMap;
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
    /// L1 ("os") is the always-on foundation; its version-selectable parts
    /// (node, python) are `always_on` scenarios the TUI shows as locked
    /// version rows, while the non-versioned L1 infra (apt, ca-certs)
    /// stays hardcoded in Dockerfile.base.head and never reaches the TUI.
    /// Defaults to "lang" so scenario.toml files predating the layer field
    /// still parse.
    #[serde(default = "default_category")]
    pub category: String,

    /// Always-on scenarios are baked into sandbox-base by `gen` regardless of
    /// the selection manifest, and the TUI renders them as non-toggleable
    /// (locked) rows. Used by L1 node/python (they MUST be present:
    /// code-server and the app web-builder depend on node) and by the L4 pi/
    /// pi-web AI-workbench stack (the app depends on pi's data formats at
    /// runtime, issue #8). Defaults to false (normal selectable scenario).
    #[serde(default)]
    pub always_on: bool,

    /// Selectable versions for this scenario. Non-empty => the TUI shows a
    /// version dropdown (Left/Right cycles) and `gen` substitutes each
    /// version's `vars` into the fragment's `{{key}}` placeholders. Empty
    /// (default) => not versioned; fragment is assembled verbatim.
    #[serde(default)]
    pub versions: Vec<Version>,

    /// Label (display string) of the version selected by default. `gen` uses
    /// this when the manifest carries no version for this scenario; the TUI
    /// pre-selects it. None => `gen` falls back to `versions[0]`.
    #[serde(default)]
    pub default_version: Option<String>,
}

/// One selectable version of a versioned scenario, declared in
/// `scenario.toml` under `[[versions]]`. `label` is the dropdown display; the
/// remaining fields (`vars`, flattened) are substituted into the fragment's
/// `{{key}}` placeholders by `gen` (e.g. `{{version}}`, `{{tag}}`).
#[derive(Debug, Clone, Deserialize)]
pub struct Version {
    pub label: String,
    #[serde(flatten)]
    pub vars: HashMap<String, String>,
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

    #[test]
    fn always_on_and_versions_default_when_omitted() {
        let raw = r#"
            id = "rust"
            name = "Rust"
            description = "rust toolchain"
        "#;
        let meta: ScenarioMeta = toml::from_str(raw).unwrap();
        assert!(!meta.always_on);
        assert!(meta.versions.is_empty());
        assert_eq!(meta.default_version, None);
    }

    #[test]
    fn versioned_always_on_scenario_parses() {
        let raw = r#"
id = "node"
name = "Node"
description = "node runtime"
category = "os"
always_on = true
default_version = "20.18.0"
[[versions]]
label = "20.18.0"
version = "20.18.0"
[[versions]]
label = "22.11.0"
version = "22.11.0"
"#;
        let meta: ScenarioMeta = toml::from_str(raw).unwrap();
        assert!(meta.always_on);
        assert_eq!(meta.category, "os");
        assert_eq!(meta.default_version.as_deref(), Some("20.18.0"));
        assert_eq!(meta.versions.len(), 2);
        assert_eq!(meta.versions[0].label, "20.18.0");
        assert_eq!(meta.versions[0].vars.get("version").unwrap(), "20.18.0");
        assert_eq!(meta.versions[1].label, "22.11.0");
    }

    #[test]
    fn version_vars_flatten_extra_keys_as_template_vars() {
        // python-build-standalone needs version + tag; both become {{}} vars.
        let raw = r#"
id = "python"
name = "Python"
description = "cpython"
[[versions]]
label = "3.12.7"
version = "3.12.7"
tag = "20241002"
"#;
        let meta: ScenarioMeta = toml::from_str(raw).unwrap();
        let v = &meta.versions[0];
        assert_eq!(v.label, "3.12.7");
        assert_eq!(v.vars.get("version").unwrap(), "3.12.7");
        assert_eq!(v.vars.get("tag").unwrap(), "20241002");
    }
}
