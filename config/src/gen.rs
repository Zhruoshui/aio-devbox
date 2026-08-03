// Build-time Dockerfile.base assembler (design §2.4). Reads the selection
// manifest, resolves each enabled id to its fragment, and concatenates:
//   Dockerfile.base.head + Σ(asc id) fragment.Dockerfile + Dockerfile.base.tail
// into Dockerfile.base. Pure file IO + string assembly - no Dockerfile parsing.
// Idempotent: same selection -> same output (so `make build-base` is safe to
// re-run). Fragments already carry their own `# >>> scenario: id >>>` banners,
// so gen only joins them with blank-line separators.

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

    // Deterministic order: sort enabled ids alphabetically + dedup (design §2.4).
    let mut ids = enabled.scenarios.clone();
    ids.sort();
    ids.dedup();

    let head = fs::read_to_string(&p.head)
        .with_context(|| format!("read {}", p.head.display()))?;
    let tail = fs::read_to_string(&p.tail)
        .with_context(|| format!("read {}", p.tail.display()))?;

    // Resolve + validate each enabled id to its fragment (bail on unknown id so
    // a stale manifest pointing at a removed scenario fails loudly at build).
    let mut fragments: Vec<(&str, String)> = Vec::with_capacity(ids.len());
    for id in &ids {
        let s = match by_id.get(id.as_str()) {
            Some(s) => *s,
            None => bail!(
                "enabled scenario {:?} not found in {} (run `make config` to reselect)",
                id,
                p.scenarios.display()
            ),
        };
        let frag = fs::read_to_string(&s.fragment)
            .with_context(|| format!("read {}", s.fragment.display()))?;
        fragments.push((id.as_str(), frag));
    }

    let out = assemble(&head, &tail, &fragments);
    fs::write(&p.out, &out).with_context(|| format!("write {}", p.out.display()))?;
    println!(
        "wrote {} (scenarios: {})",
        p.out.display(),
        if ids.is_empty() {
            "none".to_string()
        } else {
            ids.join(", ")
        }
    );
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assemble_empty_is_head_plus_tail() {
        // No scenarios: output is head + blank + tail, each newline-terminated.
        let out = assemble("FROM x\nRUN a", "USER gem\n", &[]);
        assert_eq!(out, "FROM x\nRUN a\n\nUSER gem\n");
    }

    #[test]
    fn assemble_inserts_fragments_between_head_and_tail() {
        let frag = "# >>> scenario: rust >>>\nRUN rustup\n# <<< scenario: rust <<<";
        let out = assemble("FROM x\n", "USER gem\n", &[("rust", frag.to_string())]);
        // head \n, blank, fragment \n, blank, tail \n
        assert_eq!(out, "FROM x\n\n# >>> scenario: rust >>>\nRUN rustup\n# <<< scenario: rust <<<\n\nUSER gem\n");
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
}
