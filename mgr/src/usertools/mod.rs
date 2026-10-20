//! usertools 工具商店(10-20-usertools-app-store)。
//!
//! 方向翻转(D2)后的形态:`usertools/<id>/` 目录是工具的**唯一事实来源**
//! (构建侧 assemble_with_tools 一直读的就是它)。表单上架的工具存
//! `recipe.toml`(recipe.rs)+ **生成的** `fragment.Dockerfile`(render.rs
//! 是唯一写入方);无 recipe 的目录是存量/手写工具,仅 raw 编辑可改。
//!
//! 模块地图:recipe.rs 配方 schema / render.rs 确定性生成器 /
//! builtins.rs 内置种子货架 / store.rs 货架扫描+引用统计。
//! 本文件保留共享骨干:slug、标记块解析(list_tools/blocks_well_formed)、
//! /root 遮盖守卫(validate)、原子写(write_atomic)。
//!
//! 历史:09-23 修正 2 曾移除结构化表单(单文件直编);修正 3 把工具提升
//! 为 L5 场景。10-20 退役了单文件编辑面与 mgr-data 元数据(usertools.json),
//! 以「表单只管理自己生成的工具」的方式迎回表单。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub mod builtins;
pub mod recipe;
pub mod render;
pub mod store;

/// Path of one tool's fragment directory inside the repo (`usertools/<tool>/`).
pub fn tool_dir(repo: &Path, id: &str) -> PathBuf {
    repo.join("usertools").join(id)
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
/// blocks. The shelf uses this as the display-name fallback for recipe-less
/// (legacy) tools; render.rs markers round-trip through the same payload.
pub fn list_tools(content: &str) -> Vec<String> {
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
/// marker is always closed by `# --- /tool ---` and opens never nest. The raw
/// fragment save path requires this so a tool dir can never hold a malformed
/// block - a tool is all-or-nothing.
pub fn blocks_well_formed(content: &str) -> std::result::Result<(), String> {
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

    // ── list_tools ─────────────────────────────────────────────────

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

}
