//! 内置种子货架(10-20 D3):mgr 随版本分发的静态推荐清单。
//!
//! 只收 apt 类型(零字段门槛、无 URL 失效问题);tarball/Release 类不进
//! 种子。清单是纯数据,点「添加」时才由 store 物化进 `usertools/<id>/`
//! (目录保持纯用户领地,git 只出现真上架过的工具)。

use std::path::Path;

use anyhow::{Context, Result};

use super::recipe::{Install, Recipe, SCHEMA_VERSION};

fn apt(
    name: &str,
    category: &str,
    description: &str,
    version: &str,
    homepage: &str,
    packages: &[&str],
) -> Recipe {
    Recipe {
        schema: SCHEMA_VERSION,
        name: name.into(),
        description: description.into(),
        category: category.into(),
        version: version.into(),
        homepage: homepage.into(),
        install: Install::Apt {
            packages: packages.iter().map(|p| p.to_string()).collect(),
        },
    }
}

/// The builtin shelf, display order. ids = slug(name); PRD leaves the exact
/// list to implementation time — keep every entry apt-only.
pub fn all() -> Vec<Recipe> {
    vec![
        apt("htop", "shell", "交互式进程监视器（top 的现代替代）", "bookworm 源", "https://htop.dev", &["htop"]),
        apt("tree", "shell", "以树状图列出目录内容", "bookworm 源", "http://mama.indstate.edu/users/ice/tree/", &["tree"]),
        apt("tmux", "shell", "终端复用器：会话保持 + 分屏", "bookworm 源", "https://tmux.github.io", &["tmux"]),
        apt("rsync", "shell", "增量文件同步 / 备份", "bookworm 源", "https://rsync.samba.org", &["rsync"]),
        apt("zip", "shell", "zip / unzip 压缩解压", "bookworm 源", "https://infozip.sourceforge.net", &["zip", "unzip"]),
        apt("qpdf", "document", "PDF 转换 / 加解密 / 拆合页", "bookworm 源", "https://qpdf.sourceforge.io", &["qpdf"]),
        apt("poppler-utils", "document", "pdftotext / pdftoppm 等 PDF 工具集", "bookworm 源", "https://poppler.freedesktop.org", &["poppler-utils"]),
        apt("imagemagick", "media", "图像格式转换与批处理（magick / convert）", "bookworm 源", "https://imagemagick.org", &["imagemagick"]),
        apt("ffmpeg", "media", "音视频转换 / 剪辑 / 抽帧", "bookworm 源", "https://ffmpeg.org", &["ffmpeg"]),
        apt("shellcheck", "dev", "shell 脚本静态检查", "bookworm 源", "https://www.shellcheck.net", &["shellcheck"]),
        apt("sqlite3", "dev", "SQLite 命令行客户端", "bookworm 源", "https://sqlite.org", &["sqlite3"]),
        apt("netcat", "network", "nc：TCP/UDP 调试瑞士军刀", "bookworm 源", "https://nmap.org/ncat/", &["netcat-openbsd"]),
    ]
}

/// One builtin by (slug) id.
pub fn get(id: &str) -> Option<Recipe> {
    all().into_iter().find(|r| super::slug(&r.name).as_deref() == Some(id))
}

/// Materialize builtin `id` under `usertools/<id>/` (recipe.toml + generated
/// fragment). Errors when the builtin is unknown or the dir already exists —
/// the caller (routes) turns that into a 404 / 409.
pub fn materialize(repo: &Path, id: &str) -> Result<Recipe> {
    let recipe = get(id).context(format!("未知内置工具 {id:?}"))?;
    let dir = repo.join("usertools").join(id);
    anyhow::ensure!(!dir.exists(), "工具 {id:?} 已存在");
    super::store::write_recipe(repo, id, &recipe)?;
    Ok(recipe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_ids_are_valid_and_unique() {
        let mut ids = Vec::new();
        for r in all() {
            let id = super::super::slug(&r.name).expect("slug-able name");
            assert!(matches!(r.install, Install::Apt { .. }), "{} must stay apt-only", r.name);
            super::super::recipe::validate(&r, &id).unwrap();
            ids.push(id);
        }
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "duplicate builtin ids");
        assert!(all().len() >= 8 && all().len() <= 12, "PRD says 8~12 entries");
    }

    #[test]
    fn materialize_writes_recipe_and_fragment_then_conflicts() {
        let dir = std::env::temp_dir().join(format!("mgr-ut-builtin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let r = materialize(&dir, "htop").unwrap();
        assert_eq!(r.name, "htop");
        let tool = dir.join("usertools").join("htop");
        assert!(tool.join("recipe.toml").is_file());
        let frag = std::fs::read_to_string(tool.join("fragment.Dockerfile")).unwrap();
        assert!(frag.contains("--no-install-recommends htop"));
        assert!(frag.contains("# --- tool: htop ---"));

        // Second materialize is a conflict; unknown id is an error.
        assert!(materialize(&dir, "htop").is_err());
        assert!(materialize(&dir, "nope").is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
