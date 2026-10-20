//! User-tools fragment store (R2 + 修正 2/3, task 09-23-image-detail-tools-log).
//!
//! 修正 2 removed the structured form: the WebUI is a single direct-edit
//! text editor over ONE fragment (`usertools/fragment.Dockerfile`, repo root).
//! mgr owns that file's FORMAT only insofar as it must round-trip verbatim;
//! the per-tool logic that used to live here (parse / render / validate_entries)
//! is gone with the form.
//!
//! 修正 3 promotes each custom tool to an independent L5 scenario. The tool
//! metadata (which tools exist, their display names) lives in mgr-data
//! (`<data>/usertools.json`, runtime state, never in the repo / docker build
//! context); the per-tool install fragments live under `usertools/<tool>/`
//! (a dedicated repo subtree the mgr container mounts read-write - compose.yml).
//! A tool = an entry in the meta file + a fragment directory, surfaced to the
//! create wizard through the L5 picker.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub mod recipe;
pub mod render;

/// Path of the single editable fragment inside the repo (`usertools/fragment.Dockerfile`).
/// 修正 3 moved it out of `scenarios/` into a dedicated subtree so the mgr
/// container can mount JUST this tree read-write (compose.yml) without opening
/// the whole scenario catalog to writes.
pub fn fragment_path(repo: &Path) -> PathBuf {
    repo.join("usertools").join("fragment.Dockerfile")
}

/// Path of one tool's fragment directory inside the repo (`usertools/<tool>/`).
pub fn tool_dir(repo: &Path, id: &str) -> PathBuf {
    repo.join("usertools").join(id)
}

/// One custom tool. `id` is the directory/fragment-safe form of the name
/// (`^[a-z0-9][a-z0-9-]*$`); `name` is the display label from the fragment's
/// `# --- tool: <name> ---` marker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserTool {
    pub id: String,
    pub name: String,
}

/// Slugify a free-form tool name into a directory-safe id: lowercase, spaces /
/// underscores / runs of non-alphanumerics collapse to `-`, leading/trailing
/// `-` stripped. `Rust Tool` -> `rust-tool`. Returns None when nothing usable
/// remains (caller surfaces a clear error rather than inventing an id).
pub fn slug(name: &str) -> Option<String> {
    let mut out = String::new();
    let mut last_dash = false;
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Parse the tool names out of the fragment's `# --- tool: <name> ---` marker
/// blocks (display-only; drives the editor's "contains N tools" label). A
/// marker block's content is taken verbatim from the open marker line to the
/// matching `# --- /tool ---` close; the name is the open marker's payload.
pub fn list_tools(content: &str) -> Vec<String> {
    const OPEN: &str = "# --- tool:";
    const CLOSE: &str = "# --- /tool ---";
    let mut out = Vec::new();
    let mut in_block = false;
    for line in content.lines() {
        let t = line.trim();
        if let Some(name) = open_name(t) {
            if !in_block {
                out.push(name);
                in_block = true;
            }
            // A nested open inside a block is malformed; the sync/write guard
            // rejects the file before this list is ever relied on, so a
            // best-effort name list is fine here.
        } else if t == CLOSE {
            in_block = false;
        }
    }
    out
}

/// The payload of a `# --- tool: <name> ---` line.
fn open_name(line: &str) -> Option<String> {
    let rest = line.strip_prefix("# --- tool:")?.trim();
    let name = rest.strip_suffix("---")?.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// True when every `# --- tool:` block in `content` is well-formed: an open
/// marker is always closed by `# --- /tool ---`, opens never nest, and every
/// block carries a slug-able tool name. The save path requires this so the
/// per-tool fragments (sync_scenario_dirs) can never be derived from a
/// malformed file - a tool is all-or-nothing.
pub fn blocks_well_formed(content: &str) -> std::result::Result<(), String> {
    const OPEN: &str = "# --- tool:";
    const CLOSE: &str = "# --- /tool ---";
    let mut stack = 0usize;
    for (i, raw) in content.lines().enumerate() {
        let t = raw.trim();
        if open_name(t).is_some() {
            if stack > 0 {
                return Err(format!("第 {} 行：工具块嵌套（一个 `# --- tool:` 出现在另一个块内部）。", i + 1));
            }
            stack += 1;
        } else if t == CLOSE {
            if stack == 0 {
                return Err(format!("第 {} 行：`# --- /tool ---` 没有对应的起始标记。", i + 1));
            }
            stack -= 1;
        }
    }
    if stack > 0 {
        return Err("工具块缺少 `# --- /tool ---` 结束标记。".to_string());
    }
    Ok(())
}

/// Split `content` into per-tool fragment bodies keyed by slug id. Each
/// tool's fragment is the bytes from its `# --- tool: <name> ---` open marker
/// through the matching `# --- /tool ---` close (markers included), so the
/// assembled whole is byte-identical to concatenating the per-tool fragments
/// in order. Returns an error when a tool name does not slugify.
pub fn split_tools(content: &str) -> std::result::Result<Vec<(UserTool, String)>, String> {
    const OPEN: &str = "# --- tool:";
    const CLOSE: &str = "# --- /tool ---";
    blocks_well_formed(content)?;
    let mut out: Vec<(UserTool, String)> = Vec::new();
    let mut cur: Option<(UserTool, String)> = None;
    for line in content.lines() {
        let t = line.trim();
        if let Some(name) = open_name(t) {
            let id = slug(&name)
                .ok_or_else(|| format!("工具名 {name:?} 无法转换为目录名（需要字母/数字/连字符）。"))?;
            cur = Some((UserTool { id, name }, String::new()));
        }
        if let Some((_, body)) = cur.as_mut() {
            body.push_str(line);
            body.push('\n');
        }
        if t == CLOSE {
            if let Some(entry) = cur.take() {
                out.push(entry);
            }
        }
    }
    Ok(out)
}

/// Rebuild the per-tool fragment dirs under `usertools/` to match `content`:
/// write each tool's fragment, and REMOVE any tool dir that no longer has a
/// matching marker block. Returns the surviving tool list. A failure part-way
/// leaves the tree best-effort consistent (the meta is rewritten only from a
/// full successful split).
pub fn sync_scenario_dirs(repo: &Path, content: &str) -> Result<Vec<UserTool>> {
    let tools = split_tools(content).map_err(anyhow::Error::msg)?;
    let root = repo.join("usertools");

    // Remove dirs for tools that are gone.
    let live: std::collections::HashSet<&str> = tools.iter().map(|(t, _)| t.id.as_str()).collect();
    if root.is_dir() {
        for entry in std::fs::read_dir(&root).with_context(|| format!("read {}", root.display()))? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            if id == "fragment.Dockerfile" {
                continue;
            }
            if !live.contains(id.as_str()) {
                std::fs::remove_dir_all(&path)
                    .with_context(|| format!("remove stale tool dir {}", path.display()))?;
            }
        }
    }

    // Write each tool's fragment.
    for (tool, body) in &tools {
        let dir = tool_dir(repo, &tool.id);
        std::fs::create_dir_all(&dir).with_context(|| format!("mkdir {}", dir.display()))?;
        let frag = dir.join("fragment.Dockerfile");
        write_atomic(&frag, body)?;
    }

    Ok(tools.into_iter().map(|(t, _)| t).collect())
}

/// The tool metadata persisted in mgr-data (`<data>/usertools.json`). Runtime
/// state only — never in the repo (the on-disk fragment dirs are the source
/// of truth for what is installed; the meta only records display names).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolMeta {
    pub tools: Vec<UserTool>,
}

/// Persist the meta file atomically (same rename discipline as the fragment).
pub fn save_meta(data: &Path, meta: &ToolMeta) -> Result<()> {
    let path = data.join("usertools.json");
    let body = serde_json::to_string_pretty(meta).context("serialize usertools meta")?;
    write_atomic(&path, &body)
}

/// Load the meta file; a missing or corrupt file yields an empty meta (the
/// picker falls back to scanning the on-disk fragment dirs).
pub fn load_meta(data: &Path) -> ToolMeta {
    let path = data.join("usertools.json");
    let Ok(body) = std::fs::read_to_string(&path) else {
        return ToolMeta { tools: Vec::new() };
    };
    serde_json::from_str(&body).unwrap_or(ToolMeta { tools: Vec::new() })
}

/// Scan `usertools/` for the create wizard's L5 picker: every tool dir with a
/// fragment on disk, id-sorted, display name read back out of the fragment's
/// `# --- tool: <name> ---` marker (falls back to the id). The on-disk dirs are
/// the single source of truth, so this is correct immediately after a save
/// (AC7) and on a fresh checkout with committed tool dirs.
pub fn picker_tools(repo: &Path) -> Vec<UserTool> {
    let root = repo.join("usertools");
    let mut out: Vec<UserTool> = Vec::new();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        let frag = path.join("fragment.Dockerfile");
        let Ok(body) = std::fs::read_to_string(&frag) else {
            continue;
        };
        let name = list_tools(&body).into_iter().next().unwrap_or_else(|| id.clone());
        out.push(UserTool { id, name });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

// ── validation (raw editor) ───────────────────────────────────────

/// Save-time guard for the direct editor: the hard rules that can be checked
/// statically. Comments are exempt (the file's own docs quote `/root`), every
/// instruction is not: /root is the workspace volume's mount point, so an
/// image-layer write there is masked and the tool "disappears".
pub fn validate(content: &str) -> Result<(), String> {
    if content.trim().is_empty() {
        return Err("文件内容不能为空。".into());
    }
    for segs in logical_instructions(content) {
        let joined = segs
            .iter()
            .map(|(_, s)| s.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        // Instruction keywords are matched case-insensitively; the ARGUMENTS
        // stay verbatim (the `writes_root` comparison is case-sensitive on
        // purpose - /root is the only spelling that addresses the mount).
        let upper = joined.to_ascii_uppercase();
        // WORKDIR /root (or ~) — the classic masked-write.
        if upper.starts_with("WORKDIR") {
            let rest = joined[joined.find(char::is_whitespace).unwrap_or(joined.len())..].trim();
            if writes_root(rest) {
                return Err(root_error_for(&segs, &joined));
            }
            continue;
        }
        // COPY/ADD: the destination is the last whitespace-separated token
        // (a `/root` SOURCE is deliberately allowed - the layer exists at
        // build time; only the destination is masked at runtime).
        if upper.starts_with("COPY") || upper.starts_with("ADD") {
            if let Some(dest) = joined.split_whitespace().last() {
                if writes_root(dest.trim_matches('"')) {
                    return Err(root_error_for(&segs, &joined));
                }
            }
            continue;
        }
        // RUN ...: any token pointing into /root (or ~) is a write target we
        // cannot justify — the directory is a mount point, not image storage.
        // The check runs on the WHOLE logical instruction: the canonical
        // install shape is multi-line (`RUN set -eux; \` + the actual
        // commands), so a physical-line scan stops exactly one line short of
        // the write. The `\`-glued form catches a `/root` split by the
        // user's own continuation, the space-joined one catches a bare
        // `/root` that only becomes a token after joining (`-C \` + `/root`).
        if upper.starts_with("RUN") {
            let glued = segs.iter().map(|(_, s)| s.as_str()).collect::<String>();
            let hit = token_writes_root(&joined) || token_writes_root(&glued);
            if hit {
                return Err(root_error_for(&segs, &joined));
            }
        }
    }
    Ok(())
}

/// One Dockerfile logical instruction = its physical lines, each carrying its
/// 1-based line number: a line ending in `\` continues the instruction onto
/// the next physical line, and comment / blank lines inside a continuation
/// are skipped (Docker's parser ignores them too — a `#` line must not be able
/// to break, or hide, a continuation).
fn logical_instructions(content: &str) -> Vec<Vec<(usize, String)>> {
    let mut out: Vec<Vec<(usize, String)>> = Vec::new();
    let mut cur: Vec<(usize, String)> = Vec::new();
    for (i, raw) in content.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let continued = line.ends_with('\\');
        cur.push((i + 1, line.trim_end_matches('\\').trim_end().to_string()));
        if !continued {
            out.push(std::mem::take(&mut cur));
        }
    }
    // A file ending on a dangling `\` still gets its instruction checked
    // (it is a syntax error anyway, but a masked write is the worse one).
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Does any token of this text address the masked mount?
fn token_writes_root(text: &str) -> bool {
    text.split_whitespace().any(writes_root)
}

/// Point the error at the physical line carrying the offending token when one
/// can be named, else at the instruction's first line (the token was split
/// across the continuation).
fn root_error_for(segs: &[(usize, String)], joined: &str) -> String {
    match segs.iter().find(|(_, s)| token_writes_root(s)) {
        Some((no, s)) => root_error(*no, s),
        None => root_error(segs.first().map(|(no, _)| *no).unwrap_or(0), joined),
    }
}

/// Does this token address the masked workspace mount?
fn writes_root(tok: &str) -> bool {
    let t = tok.trim_matches(|c| c == '"' || c == '\'' || c == ';' || c == ',');
    t == "/root"
        || t.starts_with("/root/")
        || t.starts_with("/root\\")
        || t == "~"
        || t.starts_with("~/")
        || t.contains("=/root/")
        || t.contains(":/root/")
}

fn root_error(line_no: usize, line: &str) -> String {
    format!(
        "第 {line_no} 行写入了 /root：{line:?}\n共享工作区卷挂载在 /root，镜像层里写进去的内容会被遮盖（工具在容器里“消失”）。\n单二进制装到 /usr/local/bin，目录树装到 /opt/<tool> 再软链到 /usr/local/bin。"
    )
}

// ── atomic write ───────────────────────────────────────────────────

/// Write `content` to `path` atomically (temp file + rename in the same
/// directory). The rename keeps the inode swap atomic for readers (the mgr
/// Dockerfile-read path and a container code-server mount may both be reading
/// it); unlike caddy.rs' in-place `fs::write` this file is NOT bind-mounted,
/// so a rename is safe here and gives readers an all-or-nothing view.
pub fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let dir = path
        .parent()
        .context("path has no parent directory")?;
    std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    let file = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("fragment.Dockerfile");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = dir.join(format!(".{file}.tmp-{}-{nanos}", std::process::id()));
    std::fs::write(&tmp, content).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("rename into {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_fragment() -> String {
        "# header comment\n\
         # --- tool: RipGrep ---\n\
         # install: apt\n\
         RUN apt-get install -y ripgrep\n\
         # --- /tool ---\n\
         \n\
         # --- tool: fd-find ---\n\
         RUN curl -fsSL https://example.com/fd.tar.gz | tar -xz -C /opt\n\
         # --- /tool ---\n\
         # footer\n"
            .to_string()
    }

    // ── list_tools / split_tools ────────────────────────────────────

    #[test]
    fn list_tools_counts_marker_blocks() {
        assert_eq!(list_tools(&sample_fragment()), vec!["RipGrep", "fd-find"]);
        assert_eq!(list_tools("# just a comment\nRUN true\n"), Vec::<String>::new());
    }

    #[test]
    fn split_tools_extracts_each_block_verbatim() {
        let content = sample_fragment();
        let tools = split_tools(&content).unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].0.id, "ripgrep");
        assert_eq!(tools[1].0.id, "fd-find");
        // Each per-tool fragment is a verbatim slice of the whole file, in
        // order: concat(tools) + the prose between/around them == content.
        for (_, body) in &tools {
            assert!(content.contains(body.as_str()), "{body:?}");
        }
        // Order is preserved: block 1 appears before block 2 in the file.
        assert!(content.find(&tools[0].1).unwrap() < content.find(&tools[1].1).unwrap());
    }

    #[test]
    fn split_tools_rejects_malformed_blocks() {
        assert!(split_tools("# --- tool: x ---\nRUN true\n").is_err()); // unterminated
        assert!(split_tools("# --- /tool ---\n").is_err()); // close without open
        assert!(split_tools("# --- tool: x ---\n# --- tool: y ---\n# --- /tool ---\n# --- /tool ---\n").is_err()); // nested
    }

    #[test]
    fn slug_normalizes_names() {
        assert_eq!(slug("Rust Tool"), Some("rust-tool".into()));
        assert_eq!(slug("  my_tool  "), Some("my-tool".into()));
        assert_eq!(slug("fd"), Some("fd".into()));
        assert_eq!(slug("..."), None);
        assert_eq!(slug(""), None);
    }

    // ── sync_scenario_dirs ──────────────────────────────────────────

    #[test]
    fn sync_writes_and_prunes_tool_dirs() {
        let dir = std::env::temp_dir().join(format!("mgr-ut-sync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let content = sample_fragment();
        let tools = sync_scenario_dirs(&dir, &content).unwrap();
        assert_eq!(tools.len(), 2);
        assert!(tool_dir(&dir, "ripgrep").join("fragment.Dockerfile").is_file());
        assert!(tool_dir(&dir, "fd-find").join("fragment.Dockerfile").is_file());

        // Removing a tool from the fragment prunes its dir on re-sync.
        let trimmed = content.replace(
            "# --- tool: fd-find ---\nRUN curl -fsSL https://example.com/fd.tar.gz | tar -xz -C /opt\n# --- /tool ---\n",
            "",
        );
        let tools = sync_scenario_dirs(&dir, &trimmed).unwrap();
        assert_eq!(tools.len(), 1);
        assert!(tool_dir(&dir, "ripgrep").join("fragment.Dockerfile").is_file());
        assert!(!tool_dir(&dir, "fd-find").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── validate (raw editor) ───────────────────────────────────────

    #[test]
    fn validate_rejects_writes_into_root_but_allows_comments() {
        // The file's own docs quote /root; only instruction lines are judged.
        let ok = "# 绝不写 /root\nRUN echo ok\n";
        assert!(validate(ok).is_ok());

        for bad in [
            "WORKDIR /root\n",
            "WORKDIR ~\n",
            "COPY tool /root/tool\n",
            "ADD x.tar.gz /root/\n",
            "RUN mkdir -p /root/bin\n",
            "RUN install -m 0755 t /root/bin/t\n",
            "RUN tar -xf x.tar.gz -C /root\n",
        ] {
            let err = validate(bad).expect_err(bad);
            assert!(err.contains("/root"), "{bad} -> {err}");
        }
    }

    #[test]
    fn validate_follows_continuations_into_root_writes() {
        for bad in [
            "RUN set -eux; \\\n    install -d /root/bin\n",
            "RUN set -eux; \\\n    cp -a x /root/\n",
            "WORKDIR /root\nRUN echo x\nRUN y \\\n /root/z\n",
            "RUN echo a \\\n/root\n",
        ] {
            let err = validate(bad).expect_err(bad);
            assert!(err.contains("/root"), "{bad} -> {err}");
        }
        let commented = "RUN set -eux; \\\n# note\n    mkdir -p /root/bin\n";
        assert!(validate(commented).unwrap_err().contains("/root"));
        let err = validate("RUN true \\\n && true \\\n && mkdir -p /root/x\n").unwrap_err();
        assert!(err.contains("第 3 行"), "{err}");
    }

    #[test]
    fn validate_allows_root_as_a_copy_source() {
        // Only the DESTINATION is masked at runtime; a /root source is a
        // build-time path the layer itself owns.
        assert!(validate("COPY /root/x /usr/local/bin/x\n").is_ok());
    }

    #[test]
    fn validate_rejects_empty_and_accepts_system_paths() {
        assert!(validate("   \n").is_err());
        let ok = "RUN install -m 0755 /tmp/jq /usr/local/bin/jq\n\
                  RUN mkdir -p /opt/fd\n\
                  COPY --chmod=755 x /usr/local/bin/x\n";
        assert!(validate(ok).is_ok());
    }

    // ── meta round-trip ─────────────────────────────────────────────

    #[test]
    fn meta_round_trips_and_picker_drops_missing_fragments() {
        let dir = std::env::temp_dir().join(format!("mgr-ut-meta-{}", std::process::id()));
        let repo = dir.join("repo");
        let data = dir.join("data");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&data).unwrap();

        let meta = ToolMeta {
            tools: vec![
                UserTool { id: "ripgrep".into(), name: "RipGrep".into() },
                UserTool { id: "fd-find".into(), name: "fd-find".into() },
            ],
        };
        save_meta(&data, &meta).unwrap();
        assert_eq!(load_meta(&data), meta);

        // picker_tools returns exactly the tools whose fragment is on disk.
        sync_scenario_dirs(&repo, &sample_fragment()).unwrap();
        save_meta(&data, &meta).unwrap();
        let picker = picker_tools(&repo);
        assert_eq!(picker.len(), 2);

        // A stale meta entry (fragment dir deleted by hand) is dropped.
        let mut stale = load_meta(&data);
        stale.tools.push(UserTool { id: "ghost".into(), name: "Ghost".into() });
        save_meta(&data, &stale).unwrap();
        let picker = picker_tools(&repo);
        assert_eq!(picker.len(), 2);
        assert!(!picker.iter().any(|t| t.id == "ghost"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── atomic write ────────────────────────────────────────────────

    #[test]
    fn write_atomic_replaces_content_without_leaving_temp_files() {
        let dir = std::env::temp_dir().join(format!("mgr-usertools-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("fragment.Dockerfile");
        write_atomic(&path, "one\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\n");
        write_atomic(&path, "two\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two\n");
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n != "fragment.Dockerfile")
            .collect();
        assert!(leftovers.is_empty(), "temp files left: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fragment_path_is_the_usertools_fragment() {
        assert_eq!(
            fragment_path(Path::new("/repo")),
            PathBuf::from("/repo/usertools/fragment.Dockerfile")
        );
    }
}
