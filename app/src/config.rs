// Service registry + manifest builder.
//
// Two sources of buttons, merged at manifest time:
//   1. `services.toml` - baked into the binary at compile time (include_str!).
//      The built-in buttons: code-server, vnc, terminal, opencode. Versioned
//      with the image; editing needs an app rebuild.
//   2. `/root/.aio/buttons.toml` - runtime, on the persistent workspace
//      volume. User-registered buttons (MVP: type=agent only). Survives
//      container recreate; shared across browsers. Written via POST/DELETE
//      /api/buttons.
//
// `enabled` (button visible?) is computed live per request:
//   - type=web : TCP-probe `target` (400ms). Mirrors compose profiles - an
//     absent code-server/vnc container fails fast and the button hides.
//   - type=agent : command_exists - the binary is on the login-shell PATH.
//     Replaces the old hardcoded ENABLE_* env (which produced dead panes when a
//     scenario wasn't baked in). terminal (cmd="") is always present.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

const SERVICES_TOML: &str = include_str!("../services.toml");

/// How long a resolved login-shell PATH is trusted before re-resolving. PATH
/// changes rarely (only on tool install), so a coarse TTL avoids forking a
/// login shell on every manifest request while still picking up runtime
/// `~/.local/bin` installs within a minute.
const PATH_CACHE_TTL: Duration = Duration::from_secs(60);

/// Wrapper for the `[[service]]` array in `services.toml` (TOML's array-of-
/// tables deserializes to a map with one key, not a bare sequence).
#[derive(Debug, Deserialize)]
struct ServicesFile {
    service: Vec<Service>,
}

/// Wrapper for the `[[button]]` array in `buttons.toml` (same TOML quirk).
/// `Serialize` so the CRUD handler can round-trip the file.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ButtonsFile {
    pub button: Vec<ButtonDef>,
}

/// One user-registered button in `buttons.toml`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ButtonDef {
    pub id: String,
    pub label: String,
    /// "agent" (default) or "web" (dev server port preview via /preview/<port>/).
    #[serde(rename = "type", default = "default_button_type")]
    pub button_type: String,
    pub cmd: String,
    /// Target port for web buttons (dev server in the shared netns). Absent
    /// for agent buttons (`skip_serializing_if` keeps the file diff minimal
    /// and old files - written before this field - deserialize unchanged).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

fn default_button_type() -> String {
    "agent".to_string()
}

/// A service/button (from either source). `deletable` distinguishes user
/// buttons (true, from buttons.toml) from built-ins (false, from services.toml)
/// so the UI only offers delete on user buttons.
#[derive(Debug, Clone, Deserialize)]
pub struct Service {
    pub id: String,
    #[serde(rename = "type")]
    pub service_type: ServiceType,
    /// `host:port` to TCP-probe for liveness (type=web only).
    pub target: Option<String>,
    /// Gateway path the iframe opens (type=web only).
    pub url: Option<String>,
    /// Display name; falls back to `id` in the manifest.
    #[serde(default)]
    pub label: Option<String>,
    /// True for user-registered buttons (deletable in the UI).
    #[serde(default)]
    pub deletable: bool,
    /// Command launched in the pty; "" = default shell (type=agent only).
    pub cmd: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ServiceType {
    Web,
    Agent,
    /// A native page provided by the app itself (not an iframe/pty). Always
    /// enabled — no TCP probe or command_exists check (design §1 manifest).
    Page,
}

/// One entry in the JSON manifest returned by `GET /api/manifest`.
///
/// `url` is present only for `type=web`; `cmd` only for `type=agent` (the
/// inapplicable field is omitted, not null).
#[derive(Debug, Serialize)]
pub struct ManifestEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub service_type: ServiceType,
    pub enabled: bool,
    pub label: String,
    pub deletable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cmd: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Manifest {
    pub services: Vec<ManifestEntry>,
}

/// Cached login-shell PATH: the dirs to search, and when they were resolved.
/// Shared via `Arc<RwLock<PathCache>>` in `AppState`; refreshed on TTL expiry.
#[derive(Debug, Default)]
pub struct PathCache {
    dirs: Vec<PathBuf>,
    fetched_at: Option<Instant>,
}

impl PathCache {
    fn is_expired(&self) -> bool {
        match self.fetched_at {
            None => true,
            Some(t) => t.elapsed() >= PATH_CACHE_TTL,
        }
    }
}

/// Return the login-shell PATH dirs, refreshing the cache on TTL expiry.
///
/// Resolves via `bash -lc 'printf %s "$PATH"'` so the result matches what the
/// pty's login shell sees (incl. `~/.local/bin` runtime installs added by
/// profile.d). Falls back to this process's own PATH if bash is unavailable.
pub async fn resolve_path_dirs(cache: &Arc<RwLock<PathCache>>) -> Vec<PathBuf> {
    // Fast path: a read lock + unexpired cache.
    {
        let c = cache.read().await;
        if !c.is_expired() {
            return c.dirs.clone();
        }
    }
    // Slow path: write lock, double-check (another request may have refreshed),
    // then resolve.
    let mut c = cache.write().await;
    if c.is_expired() {
        c.dirs = resolve_login_path().await;
        c.fetched_at = Some(Instant::now());
    }
    c.dirs.clone()
}

/// Spawn a login shell and capture its `$PATH`.
async fn resolve_login_path() -> Vec<PathBuf> {
    let out = tokio::process::Command::new("bash")
        .arg("-lc")
        .arg("printf %s \"$PATH\"")
        .output()
        .await;
    let path_str = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => std::env::var("PATH").unwrap_or_default(),
    };
    std::env::split_paths(&path_str).collect()
}

/// Does `cmd`'s executable resolve on the given PATH dirs? Empty cmd => true
/// (terminal: bash is always present). Only the FIRST whitespace-separated
/// token is probed - the rest are arguments (`htop -t`, `echo hi`). Mirrors
/// `command -v` for PATH executables (does not detect shell functions/aliases
/// - rare for our use case).
pub(crate) fn command_exists(cmd: &str, dirs: &[PathBuf]) -> bool {
    let name = cmd.trim();
    if name.is_empty() {
        return true;
    }
    let exe = name.split_whitespace().next().unwrap_or("");
    dirs.iter().any(|d| {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let p = d.join(exe);
            match std::fs::metadata(&p) {
                Ok(md) => md.is_file() && (md.permissions().mode() & 0o111 != 0),
                Err(_) => false,
            }
        }
        #[cfg(not(unix))]
        {
            d.join(exe).is_file()
        }
    })
}

/// Parse the baked-in `services.toml` once at startup.
///
/// Panics at startup if the source file is invalid - this is a compile-time
/// asset, so a parse error is a programmer bug, not a runtime condition.
pub fn load_services() -> Vec<Service> {
    toml::from_str::<ServicesFile>(SERVICES_TOML)
        .expect("services.toml is invalid - fix the source file")
        .service
}

/// Expand `{env:VAR:default}` placeholders in a string with the process env:
/// set VAR => its value; unset/empty VAR => `default`. Multiple placeholders
/// are each expanded; a malformed spec (`{env:}`, `{env:VAR}` with no third
/// colon, unbalanced braces) is left as-is so a typo in services.toml is
/// visible in the manifest rather than silently swallowed.
///
/// Runs once at startup (env doesn't change during the process lifetime), not
/// per request - see issue #3 / the piWeb `url` for why the port must follow
/// the host-side publish port (`PI_WEB_HOST_PORT`).
pub(crate) fn expand_placeholders(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find('{') {
        let (before, after_start) = rest.split_at(start);
        out.push_str(before);
        // Need a closing brace for a candidate; otherwise flush the rest.
        let Some(close_rel) = after_start.find('}') else {
            out.push_str(after_start);
            return out;
        };
        let candidate = &after_start[1..close_rel]; // between { and }
        out.push_str(&expand_one(candidate));
        rest = &after_start[close_rel + 1..];
    }
    out.push_str(rest);
    out
}

/// Expand a single `{...}` candidate: `{env:VAR:default}` on match, otherwise
/// the braced original (e.g. piWeb's `{host}`, handled client-side).
fn expand_one(candidate: &str) -> String {
    match candidate.strip_prefix("env:") {
        Some(rest) => match rest.split_once(':') {
            Some((var, default)) => std::env::var(var)
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| default.to_string()),
            // `{env:VAR}` without a default is a malformed spec: keep it
            // verbatim so the mistake is visible in the manifest.
            None => format!("{{{candidate}}}"),
        },
        None => format!("{{{candidate}}}"),
    }
}

/// Parse the raw `[[button]]` defs from `buttons.toml`. Missing/empty file =>
/// empty; malformed => empty + warn (a bad edit never breaks the manifest).
/// Shared by `load_buttons` (manifest) and the CRUD handler (read/modify/write).
pub fn parse_button_defs(path: &Path) -> Vec<ButtonDef> {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) if s.trim().is_empty() => return Vec::new(),
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            tracing::warn!("buttons.toml unreadable ({}): ignoring user buttons", e);
            return Vec::new();
        }
    };
    match toml::from_str::<ButtonsFile>(&raw) {
        Ok(f) => f.button,
        Err(e) => {
            tracing::warn!("buttons.toml invalid ({}): ignoring user buttons", e);
            Vec::new()
        }
    }
}

/// Parse user-registered buttons from `buttons.toml` on the workspace volume,
/// as `Service`s for the manifest. A missing/empty/malformed file yields an
/// empty list (first `make up`, empty volume, or a bad edit).
///
/// Web buttons map to the same `ServiceType::Web` path as built-ins: the
/// target is the dev server's loopback port on the shared netns (pty-spawned
/// processes share app's network stack), and the url is the axum-side dynamic
/// reverse proxy at `/preview/<port>/` (design: axum proxy, not a Caddy route,
/// because gateway cannot reach a loopback-bound dev server).
pub fn load_buttons(path: &Path) -> Vec<Service> {
    parse_button_defs(path)
        .into_iter()
        .filter_map(|b| match (b.button_type.as_str(), b.port) {
            ("web", Some(port)) => Some(Service {
                id: b.id,
                service_type: ServiceType::Web,
                target: Some(format!("127.0.0.1:{port}")),
                url: Some(format!("/preview/{port}/")),
                label: Some(b.label),
                deletable: true,
                cmd: None,
            }),
            // A web button without a port cannot be probed or proxied - drop
            // it with a warning instead of rendering a dead pane (a bad hand
            // edit of buttons.toml must never break the manifest).
            ("web", None) => {
                tracing::warn!("web button {:?} has no port; dropping", b.id);
                None
            }
            _ => Some(Service {
                id: b.id,
                service_type: ServiceType::Agent,
                target: None,
                url: None,
                label: Some(b.label),
                deletable: true,
                cmd: Some(b.cmd),
            }),
        })
        .collect()
}

/// Merge built-in services with user buttons. Built-in wins on id collision
/// (a user button shadowing a built-in is dropped + warned).
pub fn merge_services(builtin: &[Service], user: Vec<Service>) -> Vec<Service> {
    let mut out: Vec<Service> = builtin.to_vec();
    for u in user {
        if out.iter().any(|b| b.id == u.id) {
            tracing::warn!(
                "user button id {:?} collides with a built-in; dropping",
                u.id
            );
            continue;
        }
        out.push(u);
    }
    out
}

/// Build the live manifest: compute `enabled` for every service.
pub async fn build_manifest(services: &[Service], dirs: &[PathBuf]) -> Manifest {
    let mut entries = Vec::with_capacity(services.len());
    for svc in services {
        let enabled = match svc.service_type {
            ServiceType::Web => is_web_reachable(svc).await,
            ServiceType::Agent => command_exists(svc.cmd.as_deref().unwrap_or(""), dirs),
            ServiceType::Page => true,
        };
        let (url, cmd) = match svc.service_type {
            ServiceType::Web => (svc.url.clone(), None),
            ServiceType::Agent => (None, svc.cmd.clone()),
            ServiceType::Page => (None, None),
        };
        entries.push(ManifestEntry {
            id: svc.id.clone(),
            service_type: svc.service_type,
            enabled,
            label: svc.label.clone().unwrap_or_else(|| humanize_id(&svc.id)),
            deletable: svc.deletable,
            url,
            cmd,
        });
    }
    Manifest { services: entries }
}

/// Fallback display label for a built-in service that omits `label`. Maps the
/// known ids to friendly names; unknown ids get their raw id.
fn humanize_id(id: &str) -> String {
    match id {
        "codeServer" => "code-server".to_string(),
        "vnc" => "Chromium".to_string(),
        "terminal" => "Terminal".to_string(),
        "modelsConfig" => "Model config".to_string(),
        other => other.to_string(),
    }
}

/// type=web liveness: TCP-connect to `target` with a short timeout. A down or
/// absent compose service fails fast (NXDOMAIN or refused) without hanging the
/// manifest. Any error (DNS / connect / timeout) => not enabled.
async fn is_web_reachable(svc: &Service) -> bool {
    let Some(target) = &svc.target else {
        return false;
    };
    let connect = tokio::net::TcpStream::connect(target);
    matches!(
        tokio::time::timeout(Duration::from_millis(400), connect).await,
        Ok(Ok(_))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_buttons_missing_file_is_empty() {
        let v = load_buttons(Path::new("/nonexistent/path/buttons.toml"));
        assert!(v.is_empty());
    }

    #[test]
    fn load_buttons_empty_file_is_empty() {
        let tmp = tempfile_path();
        std::fs::write(&tmp, "").unwrap();
        assert!(load_buttons(&tmp).is_empty());
        std::fs::remove_file(&tmp).unwrap();
    }

    #[test]
    fn load_buttons_parses_agent_button() {
        let tmp = tempfile_path();
        std::fs::write(
            &tmp,
            r#"
[[button]]
id = "htop"
label = "htop"
type = "agent"
cmd = "htop"
"#,
        )
        .unwrap();
        let v = load_buttons(&tmp);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].id, "htop");
        assert_eq!(v[0].service_type, ServiceType::Agent);
        assert!(v[0].deletable);
        assert_eq!(v[0].cmd.as_deref(), Some("htop"));
        assert_eq!(v[0].label.as_deref(), Some("htop"));
        std::fs::remove_file(&tmp).unwrap();
    }

    #[test]
    fn load_buttons_parses_web_button() {
        let tmp = tempfile_path();
        std::fs::write(
            &tmp,
            r#"
[[button]]
id = "my-vite"
label = "my vite"
type = "web"
cmd = ""
port = 5173
"#,
        )
        .unwrap();
        let v = load_buttons(&tmp);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].id, "my-vite");
        assert_eq!(v[0].service_type, ServiceType::Web);
        assert_eq!(v[0].target.as_deref(), Some("127.0.0.1:5173"));
        assert_eq!(v[0].url.as_deref(), Some("/preview/5173/"));
        assert_eq!(v[0].cmd, None);
        assert!(v[0].deletable);
        std::fs::remove_file(&tmp).unwrap();
    }

    #[test]
    fn load_buttons_web_without_port_is_dropped() {
        let tmp = tempfile_path();
        // Hand-edited file: a web button missing its port must not break the
        // manifest (dropped with a warning, other buttons survive).
        std::fs::write(
            &tmp,
            r#"
[[button]]
id = "broken"
label = "broken"
type = "web"
cmd = ""

[[button]]
id = "ok"
label = "ok"
cmd = "htop"
"#,
        )
        .unwrap();
        let v = load_buttons(&tmp);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].id, "ok");
        std::fs::remove_file(&tmp).unwrap();
    }

    #[test]
    fn load_buttons_legacy_file_without_port_field() {
        let tmp = tempfile_path();
        // File written before the port field existed: must deserialize fine.
        std::fs::write(
            &tmp,
            r#"
[[button]]
id = "htop"
label = "htop"
type = "agent"
cmd = "htop"
"#,
        )
        .unwrap();
        let v = load_buttons(&tmp);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].service_type, ServiceType::Agent);
        std::fs::remove_file(&tmp).unwrap();
    }

    #[test]
    fn load_buttons_malformed_is_empty() {
        let tmp = tempfile_path();
        std::fs::write(&tmp, "this is = not = valid toml {{{").unwrap();
        assert!(load_buttons(&tmp).is_empty());
        std::fs::remove_file(&tmp).unwrap();
    }

    #[test]
    fn command_exists_empty_cmd_is_true() {
        // terminal (cmd="") is always present.
        assert!(command_exists("", &[]));
        assert!(command_exists("   ", &[]));
    }

    #[test]
    fn command_exists_finds_executable() {
        let dir = std::env::temp_dir();
        let dirs = vec![dir.clone()];
        // /bin/sh is always executable; place a symlink/copy in temp is flaky
        // across CI, so resolve against a dir that holds a known binary.
        let bin_dirs: Vec<PathBuf> = std::env::split_paths(
            &std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
        )
        .collect();
        // `sh` exists on every Unix; arguments after the executable are
        // ignored by the probe.
        assert!(command_exists("sh", &bin_dirs));
        assert!(command_exists("sh -c 'true'", &bin_dirs));
        assert!(!command_exists(
            "definitely-not-a-real-binary-xyzzy",
            &bin_dirs
        ));
        assert!(!command_exists(
            "definitely-not-a-real-binary-xyzzy --flag",
            &bin_dirs
        ));
        let _ = dirs; // silence unused on non-unix
    }

    #[test]
    fn merge_services_builtin_wins_on_collision() {
        let builtin = vec![Service {
            id: "terminal".to_string(),
            service_type: ServiceType::Agent,
            target: None,
            url: None,
            label: None,
            deletable: false,
            cmd: Some(String::new()),
        }];
        let user = vec![Service {
            id: "terminal".to_string(), // collides
            service_type: ServiceType::Agent,
            target: None,
            url: None,
            label: Some("dup".to_string()),
            deletable: true,
            cmd: Some("echo".to_string()),
        }];
        let merged = merge_services(&builtin, user);
        assert_eq!(merged.len(), 1);
        assert!(!merged[0].deletable); // built-in kept, user dropped
    }

    #[test]
    fn expand_placeholders_env_overrides_default() {
        // `{host}` (client-side) passes through untouched; the env placeholder
        // resolves from the process env.
        let s = "http://{host}:{env:PI_WEB_HOST_PORT:30141}/";
        assert_eq!(expand_placeholders(s), "http://{host}:30141/");
    }

    #[test]
    fn expand_placeholders_missing_env_uses_default() {
        // Both VARS are namespaced to this test and very unlikely to be set;
        // `remove_var` first so the check can't be affected by outer env.
        std::env::remove_var("AIO_TEST_EXPAND_PORT");
        let s = "http://host:{env:AIO_TEST_EXPAND_PORT:8080}/";
        assert_eq!(expand_placeholders(s), "http://host:8080/");
    }

    #[test]
    fn expand_placeholders_set_env_uses_value() {
        std::env::set_var("AIO_TEST_EXPAND_PORT", "30142");
        let s = "http://host:{env:AIO_TEST_EXPAND_PORT:8080}/";
        assert_eq!(expand_placeholders(s), "http://host:30142/");
        std::env::remove_var("AIO_TEST_EXPAND_PORT");
    }

    #[test]
    fn expand_placeholders_empty_env_uses_default() {
        // An explicitly-empty env var is treated as unset: compose projects
        // often interpolate `PI_WEB_HOST_PORT=` from a half-edited .env, and
        // an empty port would yield "http://host:/".
        std::env::set_var("AIO_TEST_EXPAND_PORT", "");
        let s = "{env:AIO_TEST_EXPAND_PORT:30141}";
        assert_eq!(expand_placeholders(s), "30141");
        std::env::remove_var("AIO_TEST_EXPAND_PORT");
    }

    #[test]
    fn expand_placeholders_multiple_and_malformed() {
        std::env::set_var("AIO_TEST_EXPAND_A", "x");
        std::env::set_var("AIO_TEST_EXPAND_B", "y");
        // Multiple placeholders in one string.
        let s = "{env:AIO_TEST_EXPAND_A:1}-{env:AIO_TEST_EXPAND_B:2}";
        assert_eq!(expand_placeholders(s), "x-y");
        // Adjacent placeholders, no separator.
        assert_eq!(
            expand_placeholders("{env:AIO_TEST_EXPAND_A:1}{env:AIO_TEST_EXPAND_B:2}"),
            "xy"
        );
        // Malformed specs pass through verbatim (visible in the manifest,
        // not silently swallowed): {env:VAR} missing default, empty spec, and
        // a non-env braced group.
        std::env::set_var("AIO_TEST_EXPAND_A", "z");
        assert_eq!(
            expand_placeholders("{env:AIO_TEST_EXPAND_A}"),
            "{env:AIO_TEST_EXPAND_A}"
        );
        assert_eq!(expand_placeholders("{env:}"), "{env:}");
        assert_eq!(expand_placeholders("{notenv}"), "{notenv}");
        // No closing brace: rest flushed verbatim.
        assert_eq!(expand_placeholders("a{env:VAR:1"), "a{env:VAR:1");
        // No placeholder at all.
        assert_eq!(expand_placeholders("plain"), "plain");
        std::env::remove_var("AIO_TEST_EXPAND_A");
        std::env::remove_var("AIO_TEST_EXPAND_B");
    }

    #[test]
    fn humanize_id_maps_known_builtins() {
        assert_eq!(humanize_id("codeServer"), "code-server");
        assert_eq!(humanize_id("vnc"), "Chromium");
        assert_eq!(humanize_id("terminal"), "Terminal");
        assert_eq!(humanize_id("opencode"), "opencode");
    }

    /// A throwaway path under the temp dir for file-based tests. Unique per
    /// call (atomic counter) so parallel tests don't collide on one pid.
    fn tempfile_path() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut p = std::env::temp_dir();
        p.push(format!("aio-buttons-test-{}-{n}.toml", std::process::id()));
        p
    }
}
