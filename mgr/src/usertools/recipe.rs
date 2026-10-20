//! recipe.toml — the store's structured tool definition (task 10-20 D2).
//!
//! A form-published tool owns BOTH files under `usertools/<id>/`: this recipe
//! (metadata + install spec) and the GENERATED `fragment.Dockerfile` (render.rs
//! is the only writer). Directories without a recipe are legacy/hand-written
//! tools — fragment-only, raw-editable, still installable (R6).
//!
//! One struct serves two wire formats: TOML on disk (`recipe.toml`) and JSON
//! over the API (POST/PUT bodies, preview). Install is an internally-tagged
//! enum so both formats spell the discriminator as `type = "github-release"`.

use serde::{Deserialize, Serialize};

/// recipe.toml schema version. Bump on a breaking field change; `validate`
/// rejects anything else so an old mgr never silently misreads a newer file.
pub const SCHEMA_VERSION: i64 = 1;

/// Light shelf categories (D3: color chips / grouping, no icons). The store
/// UI maps these to labels; an unknown value degrades to the raw string —
/// same "never break on the unknown" rule as scenario categories.
pub const CATEGORIES: [&str; 6] = ["shell", "document", "media", "network", "dev", "other"];

fn default_category() -> String {
    "other".to_string()
}

fn default_kind() -> UrlKind {
    UrlKind::Binary
}

/// One tool recipe: display metadata + install spec. Field order is the TOML
/// file's stable layout (serde writes struct fields in declaration order).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recipe {
    pub schema: i64,
    /// Display name. Also the `# --- tool: <name> ---` marker payload, so a
    /// rename DOES change the generated fragment (and thus env_hash) — the
    /// name is identity-ish, not pure display. The other metadata fields are
    /// render-invisible (AC6).
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_category")]
    pub category: String,
    /// Display-only free text ("bookworm 源", "v10.2.0", …); NOT a pin. Real
    /// pinning is the install spec itself (apt rides the base image's suite,
    /// url/release fields are exact).
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub homepage: String,
    pub install: Install,
}

/// The install spec. Tagged by `type` (kebab-case); adding a sixth kind (the
/// out-of-scope npm/pip channel, say) is one new variant + one render template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Install {
    /// apt packages from the base image's suite. Zero-field-friction (the
    /// builtin shelf is made of these).
    Apt { packages: Vec<String> },
    /// Direct URL to a static binary or tarball.
    Url {
        url: String,
        #[serde(default = "default_kind")]
        kind: UrlKind,
        /// binary: the ONE executable's name (defaults to the tool id);
        /// tarball: paths inside the extracted tree to symlink into
        /// /usr/local/bin (e.g. "fd-v10.2.0/fd"), required non-empty.
        #[serde(default)]
        bins: Vec<String>,
    },
    /// GitHub release asset; URL is derived (repo/tag/asset), `tag = "latest"`
    /// uses the redirecting latest/download path.
    GithubRelease {
        repo: String,
        tag: String,
        asset: String,
        #[serde(default = "default_kind")]
        kind: UrlKind,
        #[serde(default)]
        bins: Vec<String>,
    },
    /// Inline shell script run as one RUN continuation.
    Script {
        script: String,
        /// Command name for the dual-channel self-check (defaults to the id).
        #[serde(default)]
        check: Option<String>,
    },
    /// Verbatim Dockerfile instructions — the form-published escape hatch.
    /// Marker lines inside are rejected (they would break block parsing).
    Raw { dockerfile: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UrlKind {
    /// Single static executable → /usr/local/bin/<bin>.
    Binary,
    /// Archive → /opt/<id>/ + symlinks for each bin.
    Tarball,
}

impl Install {
    /// The download URL this install resolves to (None for apt/script/raw).
    pub fn resolved_url(&self) -> Option<String> {
        match self {
            Install::Url { url, .. } => Some(url.clone()),
            Install::GithubRelease { repo, tag, asset, .. } => {
                if tag == "latest" {
                    Some(format!(
                        "https://github.com/{repo}/releases/latest/download/{asset}"
                    ))
                } else {
                    Some(format!(
                        "https://github.com/{repo}/releases/download/{tag}/{asset}"
                    ))
                }
            }
            _ => None,
        }
    }

    /// The command the dual-channel self-check probes (None for raw — raw
    /// carries its own checks or none, per the author's judgment).
    pub fn check_cmd(&self, id: &str) -> Option<String> {
        match self {
            Install::Apt { packages } => packages.first().cloned(),
            Install::Url { kind, bins, .. } => Some(match kind {
                UrlKind::Binary => bins
                    .first()
                    .cloned()
                    .unwrap_or_else(|| id.to_string()),
                UrlKind::Tarball => bins
                    .first()
                    .map(|p| basename(p).to_string())
                    .unwrap_or_else(|| id.to_string()),
            }),
            Install::GithubRelease { kind, bins, .. } => Some(match kind {
                UrlKind::Binary => bins
                    .first()
                    .cloned()
                    .unwrap_or_else(|| id.to_string()),
                UrlKind::Tarball => bins
                    .first()
                    .map(|p| basename(p).to_string())
                    .unwrap_or_else(|| id.to_string()),
            }),
            Install::Script { check, .. } => Some(
                check
                    .clone()
                    .unwrap_or_else(|| id.to_string()),
            ),
            Install::Raw { .. } => None,
        }
    }
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

impl Install {
    /// The one vocabulary for install kinds: fragment comment label AND shelf
    /// card `install_type` (store.rs). "custom" is reserved for recipe-less
    /// legacy dirs and never produced here.
    pub fn label(&self) -> &'static str {
        match self {
            Install::Apt { .. } => "apt",
            Install::Url { kind, .. } | Install::GithubRelease { kind, .. } => match kind {
                UrlKind::Binary => "url-binary",
                UrlKind::Tarball => "url-tarball",
            },
            Install::Script { .. } => "script",
            Install::Raw { .. } => "raw",
        }
    }
}

/// Field-level validation, shared by the API layer before any render. Errors
/// are user-facing (zh) like the rest of usertools. `id` is the directory name
/// the recipe will live under (kept for future per-id rules; unused for now).
pub fn validate(recipe: &Recipe, _id: &str) -> Result<(), String> {
    if recipe.schema != SCHEMA_VERSION {
        return Err(format!(
            "不支持的 recipe schema 版本 {}（本 mgr 支持 {}）。",
            recipe.schema, SCHEMA_VERSION
        ));
    }
    if recipe.name.trim().is_empty() {
        return Err("工具名称不能为空。".into());
    }
    if !CATEGORIES.contains(&recipe.category.as_str()) {
        return Err(format!(
            "未知分类 {:?}（可选：{}）。",
            recipe.category,
            CATEGORIES.join("、")
        ));
    }
    match &recipe.install {
        Install::Apt { packages } => {
            if packages.is_empty() {
                return Err("apt 安装至少需要一个包名。".into());
            }
            for p in packages {
                let ok = !p.is_empty()
                    && p.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                    && p.chars().all(|c| {
                        c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | '~')
                    });
                if !ok {
                    return Err(format!("非法的 apt 包名 {p:?}。"));
                }
            }
        }
        Install::Url { url, kind, bins } => {
            check_url(url)?;
            check_bins(*kind, bins)?;
        }
        Install::GithubRelease { repo, tag, asset, kind, bins } => {
            let repo_ok = repo.split('/').collect::<Vec<_>>().len() == 2
                && repo.split('/').all(|seg| {
                    !seg.is_empty()
                        && seg.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
                });
            if !repo_ok {
                return Err(format!("GitHub repo 需为 owner/name 形式，收到 {repo:?}。"));
            }
            check_word("tag", tag)?;
            check_word("asset", asset)?;
            check_bins(*kind, bins)?;
        }
        Install::Script { script, check } => {
            if script.trim().is_empty() {
                return Err("安装脚本不能为空。".into());
            }
            for line in script.lines() {
                let t = line.trim();
                if t.starts_with('#') {
                    return Err(
                        "脚本行不能以 # 开头（会被 Dockerfile 当注释吞掉）；如需复杂逻辑请用「自定义片段」类型。"
                            .into(),
                    );
                }
                if t.ends_with('\\') {
                    return Err("脚本行不能以反斜杠结尾（会破坏 RUN 续行拼接）。".into());
                }
            }
            if let Some(c) = check {
                check_word("check", c)?;
            }
        }
        Install::Raw { dockerfile } => {
            if dockerfile.trim().is_empty() {
                return Err("自定义片段不能为空。".into());
            }
            for line in dockerfile.lines() {
                let t = line.trim();
                if t.starts_with("# --- tool:") || t == "# --- /tool ---" {
                    return Err("自定义片段不能包含 tool 标记行（由系统包裹）。".into());
                }
            }
        }
    }
    Ok(())
}

/// URLs get embedded in quoted RUN lines — keep them free of the characters
/// that would break out of the quotes.
fn check_url(url: &str) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(format!("URL 需以 https:// 开头（收到 {url:?}）。"));
    }
    if url.chars().any(|c| c.is_whitespace() || c == '"' || c == '\\' || c == '\'') {
        return Err("URL 不能包含空白、引号或反斜杠。".into());
    }
    Ok(())
}

fn check_word(field: &str, v: &str) -> Result<(), String> {
    if v.is_empty() || v.chars().any(|c| c.is_whitespace() || c == '"' || c == '\\' || c == '\'') {
        return Err(format!("{field} 不能为空，且不能包含空白、引号或反斜杠。"));
    }
    Ok(())
}

fn check_bins(kind: UrlKind, bins: &[String]) -> Result<(), String> {
    match kind {
        UrlKind::Binary => {
            if bins.len() > 1 {
                return Err(format!("binary 类型最多一个可执行名（收到 {} 个）。", bins.len()));
            }
        }
        UrlKind::Tarball => {
            if bins.is_empty() {
                return Err("tarball 类型必须列出至少一个 tar 内可执行文件路径（bins）。".into());
            }
        }
    }
    for b in bins {
        if b.is_empty()
            || b.starts_with('/')
            || b.split('/').any(|seg| seg.is_empty() || seg == "..")
            || b.chars().any(|c| c.is_whitespace() || c == '"' || c == '\\' || c == '\'')
        {
            return Err(format!(
                "非法的 bin 路径 {b:?}：需为 tar 内相对路径，不含 ..、绝对路径或空白。"
            ));
        }
    }
    Ok(())
}

/// Parse a recipe.toml body (disk path owner: store.rs).
pub fn parse_toml(body: &str) -> Result<Recipe, String> {
    toml::from_str(body).map_err(|e| format!("recipe.toml 解析失败：{e}"))
}

/// Load `usertools/<id>/recipe.toml`. None = no recipe (legacy/hand-written
/// tool); Some(Err) = present but unreadable (the shelf degrades it to a
/// custom card rather than hiding the tool).
pub fn load(repo: &std::path::Path, id: &str) -> Option<Result<Recipe, String>> {
    let path = super::tool_dir(repo, id).join("recipe.toml");
    let body = std::fs::read_to_string(path).ok()?;
    Some(parse_toml(&body))
}

/// Serialize back to TOML (stable field order = struct declaration order).
pub fn to_toml(recipe: &Recipe) -> Result<String, String> {
    toml::to_string_pretty(recipe).map_err(|e| format!("recipe.toml 序列化失败：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apt_recipe() -> Recipe {
        Recipe {
            schema: SCHEMA_VERSION,
            name: "htop".into(),
            description: "进程监视器".into(),
            category: "shell".into(),
            version: "bookworm 源".into(),
            homepage: "https://htop.dev".into(),
            install: Install::Apt { packages: vec!["htop".into()] },
        }
    }

    #[test]
    fn toml_round_trips_all_types() {
        let recipes = vec![
            apt_recipe(),
            Recipe {
                name: "fd".into(),
                install: Install::GithubRelease {
                    repo: "sharkdp/fd".into(),
                    tag: "v10.2.0".into(),
                    asset: "fd-v10.2.0-x86_64-unknown-linux-gnu.tar.gz".into(),
                    kind: UrlKind::Tarball,
                    bins: vec!["fd-v10.2.0-x86_64-unknown-linux-gnu/fd".into()],
                },
                ..apt_recipe()
            },
            Recipe {
                name: "scc".into(),
                install: Install::Url {
                    url: "https://example.com/scc".into(),
                    kind: UrlKind::Binary,
                    bins: vec![],
                },
                ..apt_recipe()
            },
            Recipe {
                name: "安裝脚本".into(),
                install: Install::Script {
                    script: "make install".into(),
                    check: None,
                },
                ..apt_recipe()
            },
            Recipe {
                name: "weird".into(),
                install: Install::Raw { dockerfile: "RUN true".into() },
                ..apt_recipe()
            },
        ];
        for r in recipes {
            let text = to_toml(&r).unwrap();
            let back = parse_toml(&text).unwrap();
            assert_eq!(back, r, "round-trip failed for\n{text}");
        }
    }

    #[test]
    fn json_round_trips_tagged_enum() {
        let r = Recipe {
            name: "fd".into(),
            install: Install::GithubRelease {
                repo: "sharkdp/fd".into(),
                tag: "latest".into(),
                asset: "fd.tar.gz".into(),
                kind: UrlKind::Tarball,
                bins: vec!["fd".into()],
            },
            ..apt_recipe()
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains(r#""type":"github-release""#), "{json}");
        let back: Recipe = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn resolved_url_handles_latest() {
        let gh = Install::GithubRelease {
            repo: "o/r".into(),
            tag: "latest".into(),
            asset: "a.gz".into(),
            kind: UrlKind::Binary,
            bins: vec![],
        };
        assert_eq!(
            gh.resolved_url().unwrap(),
            "https://github.com/o/r/releases/latest/download/a.gz"
        );
        let pinned = Install::GithubRelease {
            repo: "o/r".into(),
            tag: "v1".into(),
            asset: "a.gz".into(),
            kind: UrlKind::Binary,
            bins: vec![],
        };
        assert_eq!(
            pinned.resolved_url().unwrap(),
            "https://github.com/o/r/releases/download/v1/a.gz"
        );
    }

    #[test]
    fn validate_rejects_field_errors() {
        let mut r = apt_recipe();
        r.schema = 2;
        assert!(validate(&r, "htop").unwrap_err().contains("schema"));

        let mut r = apt_recipe();
        r.install = Install::Apt { packages: vec![] };
        assert!(validate(&r, "htop").unwrap_err().contains("包名"));

        let mut r = apt_recipe();
        r.install = Install::Apt { packages: vec!["Qpdf".into()] };
        assert!(validate(&r, "htop").unwrap_err().contains("包名"));

        let mut r = apt_recipe();
        r.install = Install::Url {
            url: "ftp://x".into(),
            kind: UrlKind::Binary,
            bins: vec![],
        };
        assert!(validate(&r, "x").unwrap_err().contains("https"));

        let mut r = apt_recipe();
        r.install = Install::Url {
            url: "https://x/y.tgz".into(),
            kind: UrlKind::Tarball,
            bins: vec![],
        };
        assert!(validate(&r, "x").unwrap_err().contains("bins"));

        let mut r = apt_recipe();
        r.install = Install::Url {
            url: "https://x/y.tgz".into(),
            kind: UrlKind::Tarball,
            bins: vec!["../evil".into()],
        };
        assert!(validate(&r, "x").unwrap_err().contains(".."));

        let mut r = apt_recipe();
        r.install = Install::GithubRelease {
            repo: "owneronly".into(),
            tag: "v1".into(),
            asset: "a".into(),
            kind: UrlKind::Binary,
            bins: vec![],
        };
        assert!(validate(&r, "x").unwrap_err().contains("owner/name"));

        let mut r = apt_recipe();
        r.install = Install::Script {
            script: "# comment\necho hi".into(),
            check: None,
        };
        assert!(validate(&r, "x").unwrap_err().contains("#"));

        let mut r = apt_recipe();
        r.install = Install::Raw { dockerfile: "# --- tool: x ---\nRUN true".into() };
        assert!(validate(&r, "x").unwrap_err().contains("标记"));

        let mut r = apt_recipe();
        r.category = "weird".into();
        assert!(validate(&r, "htop").unwrap_err().contains("分类"));
    }

    #[test]
    fn check_cmd_defaults_and_basenames() {
        let apt = Install::Apt { packages: vec!["qpdf".into(), "jq".into()] };
        assert_eq!(apt.check_cmd("x").unwrap(), "qpdf");

        let bin = Install::Url {
            url: "https://e/b".into(),
            kind: UrlKind::Binary,
            bins: vec![],
        };
        assert_eq!(bin.check_cmd("fd-find").unwrap(), "fd-find");

        let tar = Install::GithubRelease {
            repo: "s/f".into(),
            tag: "v1".into(),
            asset: "a.gz".into(),
            kind: UrlKind::Tarball,
            bins: vec!["fd-v10/fd".into()],
        };
        assert_eq!(tar.check_cmd("x").unwrap(), "fd");

        let script = Install::Script { script: "true".into(), check: None };
        assert_eq!(script.check_cmd("mytool").unwrap(), "mytool");

        let raw = Install::Raw { dockerfile: "RUN true".into() };
        assert!(raw.check_cmd("x").is_none());
    }
}
