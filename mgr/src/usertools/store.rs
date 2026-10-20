//! store.rs — 货架视图扫描、物化与跨沙箱引用统计(10-20 design)。
//!
//! 事实来源是 `usertools/<id>/` 目录本身:有 recipe.toml 的工具带完整元
//! 数据,没有的(存量/手写)回退到 fragment 标记名。无缓存层——目录扫描
//! 即事实,保存后立刻可见。

use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::db;
use crate::envhash::SandboxEnv;

use super::recipe::{self, Recipe};
use super::{list_tools, render, slug, tool_dir, write_atomic};

/// One shelf card (GET /api/usertools/store 的 mine 条目)。字段 snake_case
/// 上 wire,前端镜像在 mgr-web types.ts(decode-once-at-boundary)。
#[derive(Debug, Clone, Serialize)]
pub struct Card {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub version: String,
    pub homepage: String,
    /// recipe::Install::label();存量无 recipe 目录为 "custom"。
    pub install_type: String,
    pub has_recipe: bool,
    /// 引用该工具的沙箱名(按名排序,空 = 未被引用)。
    pub referenced_by: Vec<String>,
}

/// Scan the shelf: every `usertools/<id>/` dir, metadata merged from its
/// recipe when present (a corrupt recipe degrades to a legacy card — the tool
/// stays visible and installable), referenced_by filled per card.
pub fn shelf(repo: &Path, conn: &rusqlite::Connection) -> Result<Vec<Card>> {
    let rows = db::list_sandboxes(conn)?;
    let mut refs: Vec<(String, String)> = Vec::new(); // (tool id, sandbox name)
    for row in &rows {
        let Ok(env) = serde_json::from_str::<SandboxEnv>(&row.env_json) else {
            continue; // mgr-owned rows are always valid env_json; skip junk.
        };
        for id in &env.user_tools {
            refs.push((id.clone(), row.name.clone()));
        }
    }

    let root = repo.join("usertools");
    let mut out = Vec::new();
    let entries = std::fs::read_dir(&root).with_context(|| format!("read {}", root.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        let frag = path.join("fragment.Dockerfile");
        if !frag.is_file() {
            continue;
        }
        let mut card = Card {
            id: id.clone(),
            name: id.clone(),
            description: String::new(),
            category: "other".into(),
            version: String::new(),
            homepage: String::new(),
            install_type: "custom".into(),
            has_recipe: false,
            referenced_by: Vec::new(),
        };
        match recipe::load(repo, &id) {
            Some(Ok(r)) => {
                card.name = r.name.clone();
                card.description = r.description.clone();
                card.category = r.category.clone();
                card.version = r.version.clone();
                card.homepage = r.homepage.clone();
                card.install_type = r.install.label().into();
                card.has_recipe = true;
            }
            Some(Err(_)) => {
                // Corrupt recipe: degrade to a legacy card, keep it usable.
                tracing::warn!(tool = %id, "recipe.toml unreadable; degrading to custom card");
            }
            None => {}
        }
        if !card.has_recipe {
            if let Ok(body) = std::fs::read_to_string(&frag) {
                if let Some(name) = list_tools(&body).into_iter().next() {
                    card.name = name;
                }
            }
        }
        card.referenced_by = refs
            .iter()
            .filter(|(t, _)| t == &id)
            .map(|(_, s)| s.clone())
            .collect();
        card.referenced_by.sort();
        card.referenced_by.dedup();
        out.push(card);
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Sandboxes whose env references `id` (the DELETE guard's data, D5).
pub fn referenced_by(conn: &rusqlite::Connection, id: &str) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for row in db::list_sandboxes(conn)? {
        let Ok(env) = serde_json::from_str::<SandboxEnv>(&row.env_json) else {
            continue;
        };
        if env.user_tools.iter().any(|t| t == id) {
            names.push(row.name);
        }
    }
    names.sort();
    Ok(names)
}

/// Validate + render + write one tool directory (recipe.toml + fragment).
/// The single write path for form-published tools, used by create (POST),
/// update (PUT) and builtin materialization.
pub fn write_recipe(repo: &Path, id: &str, recipe: &Recipe) -> Result<()> {
    recipe::validate(recipe, id).map_err(anyhow::Error::msg)?;
    let frag = render::render(id, recipe).map_err(anyhow::Error::msg)?;
    let dir = tool_dir(repo, id);
    std::fs::create_dir_all(&dir).with_context(|| format!("mkdir {}", dir.display()))?;
    write_atomic(&dir.join("recipe.toml"), &recipe::to_toml(recipe).map_err(anyhow::Error::msg)?)?;
    write_atomic(&dir.join("fragment.Dockerfile"), &frag)?;
    Ok(())
}

/// Remove one tool directory. `id` must be a slug-form dir name (reuses the
/// slug round-trip as the path-traversal guard); returns false when absent.
pub fn delete_tool(repo: &Path, id: &str) -> Result<bool> {
    if slug(id).as_deref() != Some(id) {
        anyhow::bail!("非法的工具 id {id:?}");
    }
    let dir = tool_dir(repo, id);
    if !dir.exists() {
        return Ok(false);
    }
    std::fs::remove_dir_all(&dir).with_context(|| format!("remove {}", dir.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mgr-ut-store-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("usertools")).unwrap();
        dir
    }

    fn conn_with_refs(pairs: &[(&str, &str)]) -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::init_schema(&conn).unwrap();
        for (tool, sandbox) in pairs {
            let env = SandboxEnv { scenarios: vec![], versions: Default::default(), user_tools: vec![tool.to_string()] };
            let row = db::SandboxRow {
                name: sandbox.to_string(),
                created_at: 0,
                env_json: serde_json::to_string(&env).unwrap(),
                env_hash: String::new(),
                cpus: None,
                mem_mb: None,
                status: "running".into(),
                adopted: false,
                external_compose: None,
                services_json: None,
            };
            db::insert_sandbox(&conn, &row).unwrap();
        }
        conn
    }

    #[test]
    fn shelf_merges_recipe_and_legacy_cards_with_refs() {
        let repo = setup("shelf");

        // A form tool: htop via write_recipe.
        let r = recipe::Recipe {
            schema: recipe::SCHEMA_VERSION,
            name: "htop".into(),
            description: "进程监视器".into(),
            category: "shell".into(),
            version: "bookworm".into(),
            homepage: "https://htop.dev".into(),
            install: recipe::Install::Apt { packages: vec!["htop".into()] },
        };
        write_recipe(&repo, "htop", &r).unwrap();

        // A legacy tool: fragment only, marker name wins over the id.
        let legacy = tool_dir(&repo, "qpdf");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("fragment.Dockerfile"), "# --- tool: qpdf ---\nRUN true\n# --- /tool ---\n").unwrap();

        let conn = conn_with_refs(&[("htop", "sbx-b"), ("htop", "sbx-a"), ("ghost", "sbx-c")]);
        let cards = shelf(&repo, &conn).unwrap();
        assert_eq!(cards.len(), 2);

        let htop = cards.iter().find(|c| c.id == "htop").unwrap();
        assert!(htop.has_recipe);
        assert_eq!(htop.install_type, "apt");
        assert_eq!(htop.description, "进程监视器");
        assert_eq!(htop.referenced_by, vec!["sbx-a", "sbx-b"]);

        let q = cards.iter().find(|c| c.id == "qpdf").unwrap();
        assert!(!q.has_recipe);
        assert_eq!(q.install_type, "custom");
        assert_eq!(q.name, "qpdf");
        assert!(q.referenced_by.is_empty());

        // The DELETE guard reads the same data.
        assert_eq!(referenced_by(&conn, "htop").unwrap(), vec!["sbx-a", "sbx-b"]);
        assert!(referenced_by(&conn, "qpdf").unwrap().is_empty());

        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn corrupt_recipe_degrades_to_custom_card() {
        let repo = setup("corrupt");
        let dir = tool_dir(&repo, "broken");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("recipe.toml"), "not [valid toml").unwrap();
        std::fs::write(dir.join("fragment.Dockerfile"), "# --- tool: Broken ---\nRUN true\n# --- /tool ---\n").unwrap();

        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::init_schema(&conn).unwrap();
        let cards = shelf(&repo, &conn).unwrap();
        assert_eq!(cards.len(), 1);
        assert!(!cards[0].has_recipe);
        assert_eq!(cards[0].name, "Broken");
        assert_eq!(cards[0].install_type, "custom");

        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn write_recipe_renders_and_overwrites() {
        let repo = setup("write");
        let r = recipe::Recipe {
            schema: recipe::SCHEMA_VERSION,
            name: "fd".into(),
            description: String::new(),
            category: "dev".into(),
            version: String::new(),
            homepage: String::new(),
            install: recipe::Install::Apt { packages: vec!["fd-find".into()] },
        };
        write_recipe(&repo, "fd", &r).unwrap();
        let frag1 = std::fs::read_to_string(tool_dir(&repo, "fd").join("fragment.Dockerfile")).unwrap();
        assert!(frag1.contains("fd-find"));

        // Update path: same call overwrites both files.
        let mut r2 = r.clone();
        r2.description = "find 的现代替代".into();
        write_recipe(&repo, "fd", &r2).unwrap();
        let toml = std::fs::read_to_string(tool_dir(&repo, "fd").join("recipe.toml")).unwrap();
        assert!(toml.contains("find 的现代替代"));

        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn delete_tool_guards_ids_and_reports_absence() {
        let repo = setup("delete");
        assert!(delete_tool(&repo, "..").is_err());
        assert!(delete_tool(&repo, "Ghost").is_err()); // not slug-form
        assert!(!delete_tool(&repo, "ghost").unwrap()); // absent, not an error
        let _ = std::fs::remove_dir_all(&repo);
    }
}
