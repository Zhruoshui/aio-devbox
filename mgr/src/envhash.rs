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

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A sandbox's scenario + version selection (API shape and env_json format).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SandboxEnv {
    /// Non-always_on scenario ids to bake in. Sorted before hashing.
    #[serde(default)]
    pub scenarios: Vec<String>,
    /// id -> version label for versioned scenarios (always_on included).
    /// BTreeMap so serialization is key-ordered and the env_json is stable.
    #[serde(default)]
    pub versions: BTreeMap<String, String>,
}

impl SandboxEnv {
    /// Validate against the repo's scenario catalog: every id must exist,
    /// scenario ids must NOT be always_on (implied, design §3.3 canonical),
    /// version ids must be versioned and their labels must be real versions.
    /// Returns the equivalent manifest for gen::assemble_for.
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

    /// Canonical JSON for env_json storage: sorted scenarios, key-ordered
    /// versions (BTreeMap already guarantees the latter).
    pub fn canonical_json(&self) -> String {
        let mut sorted = self.clone();
        sorted.scenarios.sort();
        sorted.scenarios.dedup();
        serde_json::to_string(&sorted).expect("SandboxEnv serializes")
    }
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
        };
        assert_eq!(
            env.canonical_json(),
            r#"{"scenarios":["a","b"],"versions":{}}"#
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
    }
}
