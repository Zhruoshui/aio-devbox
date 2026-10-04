// Sandbox env selection + env_hash (sandbox-mgr Phase 1, design §3.3).
//
// A sandbox's env is {scenarios: [...], versions: {id: label}} - the same
// contract as .aio/enabled.toml, minus the always_on entries (those are
// implied by gen; listing them here is rejected so the stored env stays
// canonical: two envs that assemble identically must hash identically).
//
// env_hash: design §3.3 says sha256(canonical env + fragment contents +
// head/tail). mgr implements that by hashing the ASSEMBLED Dockerfile.base
// bytes (aio_config::gen::assemble_for) - the assembly is a pure function of
// exactly those inputs, so the hash pins the same surface with one less
// reimplementation of gen's resolution rules (and stays correct if gen's
// ordering ever changes, because the bytes change with it).

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A sandbox's scenario + version + custom-tool selection (API shape and
/// env_json format).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SandboxEnv {
    /// Non-always_on scenario ids to bake in. Sorted before hashing.
    #[serde(default)]
    pub scenarios: Vec<String>,
    /// id -> version label for versioned scenarios (always_on included).
    /// BTreeMap so serialization is key-ordered and the env_json is stable.
    #[serde(default)]
    pub versions: BTreeMap<String, String>,
    /// 修正 3 (R4): selected custom-tool ids (the L5 layer). Each id names a
    /// `usertools/<id>/fragment.Dockerfile`; they are NOT in `scenarios` and
    /// do NOT flow through gen's ordinary fragment resolution. Sorted before
    /// hashing so the env_hash pins the tool SET, not the selection order.
    #[serde(default)]
    pub user_tools: Vec<String>,
}

impl SandboxEnv {
    /// Validate against the repo's scenario catalog: every id must exist,
    /// scenario ids must NOT be always_on (implied, design §3.3 canonical),
    /// version ids must be versioned and their labels must be real versions,
    /// and every user_tool id must name an on-disk `usertools/<id>/fragment.Dockerfile`.
    /// Returns the equivalent manifest for gen::assemble_for (user_tools are
    /// validated here but NOT added to it - they ride assemble_with_tools).
    pub fn to_manifest_checked(
        &self,
        repo: &std::path::Path,
    ) -> Result<aio_config::manifest::Enabled> {
        let known = aio_config::scenario::scan(&repo.join("scenarios"))?;
        let find = |id: &str| known.iter().find(|s| s.meta.id == id);

        for id in &self.scenarios {
            let Some(s) = find(id) else {
                bail!("unknown scenario {id:?} (see GET /api/scenarios)");
            };
            if s.meta.always_on {
                bail!(
                    "scenario {id:?} is always_on and cannot be listed (it is baked unconditionally; set its version in .versions instead)"
                );
            }
        }

        for (id, label) in &self.versions {
            let Some(s) = find(id) else {
                bail!("unknown versioned scenario {id:?}");
            };
            if s.meta.versions.is_empty() {
                bail!("scenario {id:?} has no versions");
            }
            if !s.meta.versions.iter().any(|v| &v.label == label) {
                bail!(
                    "version {label:?} not offered by scenario {id:?} (available: {})",
                    s.meta
                        .versions
                        .iter()
                        .map(|v| v.label.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }

        for id in &self.user_tools {
            let frag = crate::usertools::tool_dir(repo, id).join("fragment.Dockerfile");
            if !frag.is_file() {
                bail!("unknown custom tool {id:?} (no {} - see the user-tools page)", frag.display());
            }
        }

        Ok(aio_config::manifest::Enabled {
            scenarios: self.scenarios.clone(),
            versions: self
                .versions
                .iter()
                .map(|(id, label)| aio_config::manifest::VersionSelect {
                    id: id.clone(),
                    label: label.clone(),
                })
                .collect(),
        })
    }

    /// Canonical JSON for env_json storage: sorted scenarios + user_tools,
    /// key-ordered versions (BTreeMap already guarantees the latter).
    pub fn canonical_json(&self) -> String {
        let mut sorted = self.clone();
        sorted.scenarios.sort();
        sorted.scenarios.dedup();
        sorted.user_tools.sort();
        sorted.user_tools.dedup();
        serde_json::to_string(&sorted).expect("SandboxEnv serializes")
    }
}

/// 修正 3 (R4): assemble the L1-L4 base (gen::assemble_for) PLUS the selected
/// custom-tool fragments (one `usertools/<id>/fragment.Dockerfile` per tool,
/// id-sorted) PLUS a whole-layer marker comment carrying the sorted tool ids.
/// The marker is what puts the tool SET into the env_hash (a tool rename or a
/// set change therefore invalidates the cached image); the per-tool fragments
/// are what actually install the tools. A tool whose fragment is missing fails
/// loudly (never a silent skip). No tools = byte-identical to assemble_for.
pub fn assemble_with_tools(repo: &Path, env: &SandboxEnv) -> Result<String> {
    let manifest = env.to_manifest_checked(repo)?;
    let (mut dockerfile, _display) = aio_config::gen::assemble_for(repo, &manifest)?;
    if env.user_tools.is_empty() {
        return Ok(dockerfile);
    }
    let mut ids = env.user_tools.clone();
    ids.sort();
    ids.dedup();
    for id in &ids {
        let frag = crate::usertools::tool_dir(repo, id).join("fragment.Dockerfile");
        let body = std::fs::read_to_string(&frag)
            .with_context(|| format!("read custom-tool fragment {}", frag.display()))?;
        dockerfile.push('\n');
        dockerfile.push_str(&body);
        if !dockerfile.ends_with('\n') {
            dockerfile.push('\n');
        }
    }
    dockerfile.push_str("\n# >>> aio user-tools >>>\n");
    dockerfile.push_str(&format!("# tools: {}\n", ids.join(",")));
    dockerfile.push_str("# <<< aio user-tools <<<\n");
    Ok(dockerfile)
}

/// sha256 hex of the assembled Dockerfile.base content.
pub fn env_hash(dockerfile_base: &str) -> String {
    let mut h = Sha256::new();
    h.update(dockerfile_base.as_bytes());
    format!("{:x}", h.finalize())
}

/// Image tag prefix convention (design §3.5): sandbox-base-<hash[:12]> for
/// the base image; app/code-server derive the same short hash. vnc is NOT
/// env-dependent (debian:bookworm-slim) and keeps one shared tag.
pub const TAG_PREFIX: &str = "sandbox-base-";
pub const VNC_TAG: &str = "sandbox-vnc";

/// All image tags a sandbox env needs (base, app, code-server) + shared vnc.
pub fn image_tags(env_hash: &str) -> (String, String, String) {
    let short = &env_hash[..12];
    (
        format!("{TAG_PREFIX}{short}"),
        format!("sandbox-app-{short}"),
        format!("sandbox-code-server-{short}"),
    )
}

/// Compose project name for a sandbox: sbx-<name> (design §3.5).
pub fn project_name(name: &str) -> String {
    format!("sbx-{name}")
}

/// Readable combo description for the images table (S3 R1, design §2).
/// Format: `<scenario+...> [<id>@<label>...] [svc: cs,vnc,pi,piweb]` — the
/// service list names the switches that are ON (all = `all`, none = `none`).
///
/// Service-switch source of truth (S1 normalization): only code_server and
/// vnc live in db::Services; pi and pi-web are SCENARIOS (their ids in
/// `env.scenarios`), folded there by routes.rs normalize_services — so the
/// pi/pi-web switches are derived from the scenario list, not from Services.
pub fn describe_combo(env: &SandboxEnv, services: &crate::db::Services) -> String {
    let mut parts: Vec<String> = Vec::new();

    // Scenarios + versions.
    if env.scenarios.is_empty() {
        parts.push("(base)".into());
    } else {
        let mut scens = env.scenarios.clone();
        scens.sort();
        parts.push(scens.join("+"));
    }
    if !env.versions.is_empty() {
        let versions: Vec<String> = env
            .versions
            .iter()
            .map(|(id, label)| format!("{id}@{label}"))
            .collect();
        parts.push(versions.join(","));
    }

    // 修正 3 (R4): the selected custom-tool set (sorted) as its own segment,
    // so the images page / reuse hint can tell tool sets apart.
    if !env.user_tools.is_empty() {
        let mut tools = env.user_tools.clone();
        tools.sort();
        parts.push(format!("[tools: {}]", tools.join(",")));
    }

    // Service switches: cs/vnc from Services; pi/piweb derived from the
    // scenario list (S1 normalization — see the doc comment above).
    let pi = env.scenarios.iter().any(|s| s == "pi");
    let pi_web = env.scenarios.iter().any(|s| s == "pi-web");
    let on: Vec<String> = [
        ("cs", services.code_server),
        ("vnc", services.vnc),
        ("pi", pi),
        ("piweb", pi_web),
    ]
    .into_iter()
    .filter(|(_, on)| *on)
    .map(|(name, _)| name.to_string())
    .collect();
    let svc = if on.len() == 4 {
        "all".to_string()
    } else if on.is_empty() {
        "none".to_string()
    } else {
        on.join(",")
    };
    parts.push(format!("[svc: {svc}]"));

    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_sha256_hex() {
        // sha256("x") known constant - pins the hash choice (a future change
        // to the digest invalidates every stored env_hash, so lock it).
        assert_eq!(
            env_hash("x"),
            "2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881"
        );
    }

    #[test]
    fn tags_use_short_hash() {
        let (base, app, cs) = image_tags(&"a".repeat(64));
        assert_eq!(base, "sandbox-base-aaaaaaaaaaaa");
        assert_eq!(app, "sandbox-app-aaaaaaaaaaaa");
        assert_eq!(cs, "sandbox-code-server-aaaaaaaaaaaa");
    }

    #[test]
    fn canonical_json_sorts_scenarios() {
        let env = SandboxEnv {
            scenarios: vec!["b".into(), "a".into()],
            versions: BTreeMap::new(),
            user_tools: vec![],
        };
        assert_eq!(
            env.canonical_json(),
            r#"{"scenarios":["a","b"],"versions":{},"user_tools":[]}"#
        );
    }

    #[test]
    fn canonical_json_sorts_and_dedups_user_tools() {
        let env = SandboxEnv {
            scenarios: vec![],
            versions: BTreeMap::new(),
            user_tools: vec!["fd".into(), "ripgrep".into(), "fd".into()],
        };
        assert_eq!(
            env.canonical_json(),
            r#"{"scenarios":[],"versions":{},"user_tools":["fd","ripgrep"]}"#
        );
    }

    #[test]
    fn describe_combo_shapes() {
        use crate::db::Services;

        // All-on: cs+vnc services with pi and pi-web as scenarios = `all`.
        let env = SandboxEnv {
            scenarios: vec!["node".into(), "python".into(), "pi".into(), "pi-web".into()],
            versions: BTreeMap::from([
                ("node".into(), "20".into()),
                ("python".into(), "3.12".into()),
            ]),
            user_tools: vec![],
        };
        assert_eq!(
            describe_combo(
                &env,
                &Services {
                    code_server: true,
                    vnc: true
                }
            ),
            "node+pi+pi-web+python node@20,python@3.12 [svc: all]"
        );

        // Scenarios sorted; versions appended sorted (BTreeMap order).
        let env2 = SandboxEnv {
            scenarios: vec!["b".into(), "a".into()],
            versions: BTreeMap::new(),
            user_tools: vec![],
        };
        assert_eq!(
            describe_combo(
                &env2,
                &Services {
                    code_server: true,
                    vnc: true
                }
            ),
            "a+b [svc: cs,vnc]"
        );

        // Partial services: only on ones listed. pi/pi-web derived from
        // scenarios, NOT from Services.
        assert_eq!(
            describe_combo(
                &env2,
                &Services {
                    code_server: true,
                    vnc: false
                }
            ),
            "a+b [svc: cs]"
        );
        let env3 = SandboxEnv {
            scenarios: vec!["pi".into(), "pi-web".into()],
            versions: BTreeMap::new(),
            user_tools: vec![],
        };
        // pi/pi-web are SCENARIOS here (S1 normalization) — they surface in
        // the service list from the scenarios, not from Services.
        assert_eq!(
            describe_combo(
                &env3,
                &Services {
                    code_server: false,
                    vnc: true
                }
            ),
            "pi+pi-web [svc: vnc,pi,piweb]"
        );

        // No scenarios = (base).
        let base = SandboxEnv {
            scenarios: vec![],
            versions: BTreeMap::new(),
            user_tools: vec![],
        };
        assert_eq!(
            describe_combo(
                &base,
                &Services {
                    code_server: false,
                    vnc: false
                }
            ),
            "(base) [svc: none]"
        );

        // 修正 3: the selected custom-tool set rides as its own [tools: ...]
        // segment, sorted, so two tool sets never share a combo string.
        let env4 = SandboxEnv {
            scenarios: vec!["node".into(), "python".into(), "pi".into(), "pi-web".into()],
            versions: BTreeMap::new(),
            user_tools: vec!["fd".into(), "ripgrep".into()],
        };
        assert_eq!(
            describe_combo(
                &env4,
                &Services {
                    code_server: true,
                    vnc: true
                }
            ),
            "node+pi+pi-web+python [tools: fd,ripgrep] [svc: all]"
        );
    }

    // 修正 3: assemble_with_tools pins the tool set into the bytes (and thus
    // the env_hash), appends each tool's fragment, and stays byte-identical to
    // assemble_for when no tool is selected.
    #[test]
    fn assemble_with_tools_marks_the_tool_set_and_appends_fragments() {
        let dir = std::env::temp_dir().join(format!("mgr-envhash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // assemble_for scans <repo>/scenarios and reads head/tail; an empty
        // scenarios dir + stub head/tail = the pure baseline (no L1-L4).
        std::fs::create_dir_all(dir.join("scenarios")).unwrap();
        std::fs::write(dir.join("Dockerfile.base.head"), "FROM debian:bookworm-slim\n").unwrap();
        std::fs::write(dir.join("Dockerfile.base.tail"), "# tail\n").unwrap();
        crate::usertools::write_atomic(
            &crate::usertools::fragment_path(&dir),
            "# --- tool: RipGrep ---\nRUN apt-get install -y ripgrep\n# --- /tool ---\n",
        )
        .unwrap();
        let tools = crate::usertools::sync_scenario_dirs(
            &dir,
            "# --- tool: RipGrep ---\nRUN apt-get install -y ripgrep\n# --- /tool ---\n\
             # --- tool: fd ---\nRUN curl -fsSL https://example.com/fd.tgz | tar -xz -C /opt\n# --- /tool ---\n",
        )
        .unwrap();
        assert_eq!(tools.len(), 2);

        // No tools -> byte-identical to assemble_for (the base has no marker).
        let base_env = SandboxEnv {
            scenarios: vec![],
            versions: BTreeMap::new(),
            user_tools: vec![],
        };
        let (plain, _) = aio_config::gen::assemble_for(&dir, &base_env.to_manifest_checked(&dir).unwrap()).unwrap();
        let with_none = assemble_with_tools(&dir, &base_env).unwrap();
        assert_eq!(plain, with_none, "no tools => identical to assemble_for");

        // A tool set appends the fragments + marker; a different set differs.
        let one = SandboxEnv {
            scenarios: vec![],
            versions: BTreeMap::new(),
            user_tools: vec!["ripgrep".into()],
        };
        let two = SandboxEnv {
            scenarios: vec![],
            versions: BTreeMap::new(),
            user_tools: vec!["ripgrep".into(), "fd".into()],
        };
        let asm_one = assemble_with_tools(&dir, &one).unwrap();
        let asm_two = assemble_with_tools(&dir, &two).unwrap();
        assert!(asm_one.contains("apt-get install -y ripgrep"));
        assert!(asm_one.contains("# tools: ripgrep"));
        assert!(!asm_one.contains("# tools: fd,ripgrep"));
        assert!(asm_two.contains("# tools: fd,ripgrep"));
        assert_ne!(env_hash(&asm_one), env_hash(&asm_two), "tool set changes the hash");
        assert_ne!(env_hash(&asm_one), env_hash(&plain), "any tool changes the hash");

        // Selection order must not matter (sorted before hashing).
        let two_rev = SandboxEnv {
            scenarios: vec![],
            versions: BTreeMap::new(),
            user_tools: vec!["fd".into(), "ripgrep".into()],
        };
        assert_eq!(assemble_with_tools(&dir, &two_rev).unwrap(), asm_two);

        // Unknown tool fails loudly.
        let bad = SandboxEnv {
            scenarios: vec![],
            versions: BTreeMap::new(),
            user_tools: vec!["ghost".into()],
        };
        assert!(assemble_with_tools(&dir, &bad).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
