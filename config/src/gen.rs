// Build-time Dockerfile.base assembler (design §2.4). Reads the selection
// manifest, resolves each enabled id to its fragment, and concatenates:
//   Dockerfile.base.head + Σ(asc id) fragment.Dockerfile + Dockerfile.base.tail
// into Dockerfile.base. Pure file IO + string assembly - no Dockerfile parsing.
// Idempotent: same selection -> same output (so `make build-base` is safe to
// re-run). Fragments already carry their own `# >>> scenario: id >>>` banners,
// so gen only joins them with blank-line separators. A manifest `scenarios`
// of `["*"]` (the full preset) is expanded to all discovered non-always_on ids
// before assembly (design §2.2); explicit selections are used verbatim.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::manifest;
use crate::scenario;

/// Paths gen reads/writes, all under the repo root.
struct Paths {
    manifest: PathBuf,
    scenarios: PathBuf,
    head: PathBuf,
    tail: PathBuf,
    out: PathBuf,
}

impl Paths {
    fn new(repo: &Path) -> Paths {
        Paths {
            manifest: repo.join(".aio/enabled.toml"),
            scenarios: repo.join("scenarios"),
            head: repo.join("Dockerfile.base.head"),
            tail: repo.join("Dockerfile.base.tail"),
            out: repo.join("Dockerfile.base"),
        }
    }
}

pub fn run(repo: &Path) -> Result<()> {
    let p = Paths::new(repo);
    let enabled = manifest::load(&p.manifest)?;
    let known = scenario::scan(&p.scenarios)?;
    let by_id: HashMap<&str, &scenario::Scenario> = known
        .iter()
        .map(|s| (s.meta.id.as_str(), s))
        .collect();

    // Resolve a "["*"]" wildcard in the manifest to the concrete ids of every
    // discovered non-always_on scenario (design §2.2). A manifest with explicit
    // ids - or no selection at all - is returned verbatim, so pre-wildcard
    // selections assemble byte-identically (gen stays idempotent, AC3).
    let selected = enabled.expand(&known)?;

    // Deterministic order: by profile LAYER (category_rank) then id, then dedup.
    // Always-on scenarios (L1 node/python + the L4 pi/pi-web workbench stack)
    // are baked UNCONDITIONALLY - their version selection (if any) lives in
    // manifest.versions, not .scenarios - so
    // they are added here regardless of the selection. Layer order
    // (L1 os -> L2 shell -> L3 lang -> L4 app; L5 service future) makes the
    // assembled Dockerfile.base read head -> L1 -> L2 -> L3 -> L4 -> tail and
    // match the TUI grouping. Layer order is for readability only - fragments
    // are independent RUN layers with no cross-layer build-time dependency, so
    // no dependency graph is introduced.
    let mut keyed: Vec<(String, String)> = Vec::new();
    for s in &known {
        if s.meta.always_on {
            keyed.push((s.meta.id.clone(), s.meta.category.clone()));
        }
    }
    for id in &selected {
        let cat = by_id
            .get(id.as_str())
            .map(|s| s.meta.category.clone())
            .unwrap_or_default();
        keyed.push((id.clone(), cat));
    }
    sort_by_layer(&mut keyed);
    let mut ids: Vec<String> = keyed.into_iter().map(|(id, _)| id).collect();
    ids.dedup();

    let head = fs::read_to_string(&p.head)
        .with_context(|| format!("read {}", p.head.display()))?;
    let tail = fs::read_to_string(&p.tail)
        .with_context(|| format!("read {}", p.tail.display()))?;

    // Resolve + validate each id to its fragment. Always-on ids are always in
    // `known`; unknown selectable ids (stale manifest pointing at a removed
    // scenario) bail loudly. For versioned scenarios, substitute {{key}}
    // placeholders from the selected version's vars and bail on any unresolved
    // placeholder. `display` carries "id@version" for the summary print.
    let mut fragments: Vec<(&str, String)> = Vec::with_capacity(ids.len());
    let mut display: Vec<String> = Vec::with_capacity(ids.len());
    for id in &ids {
        let s = match by_id.get(id.as_str()) {
            Some(s) => *s,
            None => bail!(
                "enabled scenario {:?} not found in {} (run `make config` to reselect)",
                id,
                p.scenarios.display()
            ),
        };
        let raw = fs::read_to_string(&s.fragment)
            .with_context(|| format!("read {}", s.fragment.display()))?;
        let (frag, ver) = render_fragment(s, &raw, &enabled.versions)
            .with_context(|| format!("render fragment for scenario {:?}", id))?;
        display.push(match &ver {
            Some(label) => format!("{}@{}", id, label),
            None => id.clone(),
        });
        fragments.push((id.as_str(), frag));
    }

    let out = assemble(&head, &tail, &fragments);
    fs::write(&p.out, &out).with_context(|| format!("write {}", p.out.display()))?;
    println!(
        "wrote {} (scenarios: {})",
        p.out.display(),
        if display.is_empty() {
            "none".to_string()
        } else {
            display.join(", ")
        }
    );
    Ok(())
}

/// Sort (id, category) pairs by profile layer (`category_rank`) then id, for
/// layer-ordered assembly. Pure (no IO) so the ordering contract is
/// unit-testable without touching the filesystem.
fn sort_by_layer(pairs: &mut [(String, String)]) {
    pairs.sort_by(|a, b| {
        scenario::category_rank(&a.1)
            .cmp(&scenario::category_rank(&b.1))
            .then_with(|| a.0.cmp(&b.0))
    });
}

/// Pure assembly: head + (blank-line + fragment) per enabled scenario + blank
/// line + tail, each segment guaranteed a trailing newline. Extracted from
/// `run` so it is unit-testable without file IO (quality-guidelines: test pure
/// functions).
fn assemble(head: &str, tail: &str, fragments: &[(&str, String)]) -> String {
    let mut out = String::new();
    out.push_str(head);
    ensure_trailing_newline(&mut out);
    for (_id, frag) in fragments {
        out.push('\n');
        out.push_str(frag);
        ensure_trailing_newline(&mut out);
    }
    out.push('\n');
    out.push_str(tail);
    ensure_trailing_newline(&mut out);
    out
}

fn ensure_trailing_newline(s: &mut String) {
    if !s.ends_with('\n') {
        s.push('\n');
    }
}

/// Render a scenario fragment. For a versioned scenario (non-empty `versions`),
/// resolve the selected version and substitute its `vars` into `{{key}}`
/// placeholders, returning the rendered text plus the selected version label.
/// For a non-versioned scenario, return the fragment verbatim with no label.
/// Bails if a fragment has placeholders but no versions, or if placeholders
/// remain unresolved after substitution (missing var / typo).
fn render_fragment(
    s: &scenario::Scenario,
    raw: &str,
    selections: &[manifest::VersionSelect],
) -> Result<(String, Option<String>)> {
    if s.meta.versions.is_empty() {
        if raw.contains("{{") {
            bail!(
                "scenario {:?}: fragment has placeholders but declares no versions",
                s.meta.id
            );
        }
        return Ok((raw.to_string(), None));
    }
    let selected = resolve_version(s, selections)?;
    let rendered = substitute(raw, &selected.vars);
    if rendered.contains("{{") {
        bail!(
            "scenario {:?}: unresolved placeholders after substituting version {:?} (missing var)",
            s.meta.id,
            selected.label
        );
    }
    Ok((rendered, Some(selected.label.clone())))
}

/// Resolve the selected version for a versioned scenario: the manifest entry
/// matching this id, else `default_version`, else `versions[0]`. Bails if the
/// chosen label is not in the scenario's versions list (stale manifest / typo).
fn resolve_version<'a>(
    s: &'a scenario::Scenario,
    selections: &[manifest::VersionSelect],
) -> Result<&'a scenario::Version> {
    let label = selections
        .iter()
        .find(|vs| vs.id == s.meta.id)
        .map(|vs| vs.label.as_str())
        .or(s.meta.default_version.as_deref())
        .or_else(|| s.meta.versions.first().map(|v| v.label.as_str()));
    let Some(label) = label else {
        bail!(
            "scenario {:?} declares versions but none selectable (no manifest entry, no default_version)",
            s.meta.id
        );
    };
    s.meta
        .versions
        .iter()
        .find(|v| v.label == label)
        .with_context(|| {
            format!(
                "scenario {:?}: version label {:?} not in its versions list",
                s.meta.id, label
            )
        })
}

/// Substitute `{{key}}` placeholders in `frag` using `vars`. Each var's value
/// replaces every occurrence of `{{key}}`. Caller checks for leftover `{{` to
/// detect unresolved placeholders.
fn substitute(frag: &str, vars: &HashMap<String, String>) -> String {
    let mut out = frag.to_string();
    for (k, v) in vars {
        let placeholder = ["{{", k.as_str(), "}}"].concat();
        out = out.replace(&placeholder, v);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assemble_empty_is_head_plus_tail() {
        // No scenarios: output is head + blank + tail, each newline-terminated.
        let out = assemble("FROM x\nRUN a", "USER root\n", &[]);
        assert_eq!(out, "FROM x\nRUN a\n\nUSER root\n");
    }

    #[test]
    fn assemble_inserts_fragments_between_head_and_tail() {
        let frag = "# >>> scenario: rust >>>\nRUN rustup\n# <<< scenario: rust <<<";
        let out = assemble("FROM x\n", "USER root\n", &[("rust", frag.to_string())]);
        // head \n, blank, fragment \n, blank, tail \n
        assert_eq!(out, "FROM x\n\n# >>> scenario: rust >>>\nRUN rustup\n# <<< scenario: rust <<<\n\nUSER root\n");
    }

    #[test]
    fn assemble_preserves_fragment_order() {
        let a = "# a\nRUN a";
        let b = "# b\nRUN b";
        let out = assemble("H\n", "T\n", &[("a", a.to_string()), ("b", b.to_string())]);
        let ai = out.find("# a").unwrap();
        let bi = out.find("# b").unwrap();
        assert!(ai < bi, "fragments must keep their given order");
    }

    #[test]
    fn sort_by_layer_orders_by_layer_then_id() {
        // Enabled out of layer order: a lang (L3) toolchain, a shell (L2)
        // bundle, an app (L4) CLI, and a second lang. After sorting: shell
        // (L2) before lang (L3) before app (L4); within a layer, by id.
        let mut pairs = vec![
            ("rust".to_string(), "lang".to_string()),
            ("shell-utils".to_string(), "shell".to_string()),
            ("aichat".to_string(), "app".to_string()),
            ("python-dev".to_string(), "lang".to_string()),
        ];
        sort_by_layer(&mut pairs);
        let ids: Vec<&str> = pairs.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["shell-utils", "python-dev", "rust", "aichat"]
        );
    }

    #[test]
    fn sort_by_layer_unknown_category_sorts_last() {
        // A category not in CATEGORY_ORDER ranks after every known layer.
        let mut pairs = vec![
            ("known".to_string(), "lang".to_string()),
            ("mystery".to_string(), "future-layer".to_string()),
        ];
        sort_by_layer(&mut pairs);
        let ids: Vec<&str> = pairs.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, vec!["known", "mystery"]);
    }

    /// Build a versioned ScenarioMeta for fragment-rendering tests (the
    /// fragment path is unused by render_fragment/resolve_version).
    fn versioned_meta(id: &str, labels: &[&str], default: Option<&str>) -> scenario::ScenarioMeta {
        scenario::ScenarioMeta {
            id: id.to_string(),
            name: id.to_string(),
            description: String::new(),
            category: "os".to_string(),
            always_on: true,
            default_version: default.map(String::from),
            versions: labels
                .iter()
                .map(|l| scenario::Version {
                    label: l.to_string(),
                    vars: HashMap::from([("version".to_string(), l.to_string())]),
                })
                .collect(),
        }
    }

    #[test]
    fn substitute_replaces_known_placeholders() {
        let vars = HashMap::from([
            ("version".to_string(), "20.18.0".to_string()),
            ("tag".to_string(), "20241002".to_string()),
        ]);
        let out = substitute("ARG V={{version}} T={{tag}} X={{version}}", &vars);
        assert_eq!(out, "ARG V=20.18.0 T=20241002 X=20.18.0");
    }

    #[test]
    fn substitute_leaves_unknown_placeholders_intact() {
        // {{missing}} has no var -> left as-is; caller detects leftover {{.
        let vars = HashMap::from([("version".to_string(), "20.18.0".to_string())]);
        let out = substitute("{{version}} {{missing}}", &vars);
        assert_eq!(out, "20.18.0 {{missing}}");
    }

    #[test]
    fn render_fragment_versioned_uses_default_when_no_selection() {
        let s = scenario::Scenario {
            meta: versioned_meta("node", &["20.18.0", "22.11.0"], Some("20.18.0")),
            fragment: PathBuf::new(),
        };
        let (rendered, label) = render_fragment(&s, "ARG NODE_VERSION={{version}}", &[]).unwrap();
        assert_eq!(rendered, "ARG NODE_VERSION=20.18.0");
        assert_eq!(label.as_deref(), Some("20.18.0"));
    }

    #[test]
    fn render_fragment_uses_manifest_selection_over_default() {
        let s = scenario::Scenario {
            meta: versioned_meta("node", &["20.18.0", "22.11.0"], Some("20.18.0")),
            fragment: PathBuf::new(),
        };
        let sel = vec![manifest::VersionSelect {
            id: "node".to_string(),
            label: "22.11.0".to_string(),
        }];
        let (rendered, label) = render_fragment(&s, "ARG V={{version}}", &sel).unwrap();
        assert_eq!(rendered, "ARG V=22.11.0");
        assert_eq!(label.as_deref(), Some("22.11.0"));
    }

    #[test]
    fn render_fragment_bails_on_unresolved_placeholder() {
        let s = scenario::Scenario {
            meta: versioned_meta("node", &["20.18.0"], Some("20.18.0")),
            fragment: PathBuf::new(),
        };
        // {{tag}} has no source var -> leftover {{ -> error.
        assert!(render_fragment(&s, "{{version}} {{tag}}", &[]).is_err());
    }

    #[test]
    fn render_fragment_non_versioned_passthrough() {
        let mut meta = versioned_meta("rust", &["1.0"], None);
        meta.versions.clear();
        let s = scenario::Scenario {
            meta,
            fragment: PathBuf::new(),
        };
        let (rendered, label) = render_fragment(&s, "RUN rustup", &[]).unwrap();
        assert_eq!(rendered, "RUN rustup");
        assert_eq!(label, None);
    }

    #[test]
    fn resolve_version_falls_back_to_first_without_default_or_selection() {
        let s = scenario::Scenario {
            meta: versioned_meta("node", &["20.18.0", "22.11.0"], None),
            fragment: PathBuf::new(),
        };
        assert_eq!(resolve_version(&s, &[]).unwrap().label, "20.18.0");
    }

    #[test]
    fn resolve_version_bails_on_unknown_label() {
        let s = scenario::Scenario {
            meta: versioned_meta("node", &["20.18.0"], None),
            fragment: PathBuf::new(),
        };
        let sel = vec![manifest::VersionSelect {
            id: "node".to_string(),
            label: "99.0.0".to_string(),
        }];
        assert!(resolve_version(&s, &sel).is_err());
    }
}
