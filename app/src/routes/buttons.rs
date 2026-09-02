// POST /api/buttons      - register a user button (agent/TUI) in buttons.toml.
// DELETE /api/buttons/:id - remove a user button.
//
// buttons.toml lives on the persistent workspace volume (/root/.aio/), so
// registered buttons survive `docker compose down/up` and are shared across
// browsers. Writes are serialized by `AppState::file_lock` and done atomically
// (temp file + rename) so a crash mid-write never leaves a half file.
//
// `cmd` is an arbitrary string - same trust level as the generic
// `/api/term/ws?cmd=` (the user already has a full terminal), so there is no
// allowlist. We only validate shape (non-empty, length-capped).

use std::path::Path;
use std::time::Duration;

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::config::{parse_button_defs, ButtonDef, ButtonsFile};
use crate::state::AppState;

const MAX_LEN: usize = 64;

/// Request body for POST /api/buttons.
///
/// `type` defaults to "agent" so the original `{label, cmd}` payload still
/// registers a terminal button unchanged. `type=web` swaps `cmd` for `port`
/// (a dev server on the shared netns, previewed via /preview/<port>/).
#[derive(Debug, Deserialize)]
pub struct ButtonInput {
    pub label: String,
    #[serde(default)]
    pub cmd: String,
    #[serde(rename = "type", default)]
    pub button_type: String,
    pub port: Option<u16>,
}

/// Response body for POST /api/buttons.
#[derive(Debug, Serialize)]
pub struct ButtonOut {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub button_type: String,
    pub cmd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

pub async fn create_button(
    State(state): State<AppState>,
    Json(input): Json<ButtonInput>,
) -> Result<(StatusCode, Json<ButtonOut>), (StatusCode, String)> {
    let label = input.label.trim();
    if label.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "label must be non-empty".to_string()));
    }
    if label.len() > MAX_LEN {
        return Err((StatusCode::BAD_REQUEST, format!("label must be <= {MAX_LEN} chars")));
    }

    let (button_type, cmd, port) = validate_shape(&input)?;
    let _guard = state.file_lock.lock().await;
    let mut defs = parse_button_defs(&state.buttons_file);
    let id = unique_id(slugify(label), &defs);
    let def = ButtonDef {
        id: id.clone(),
        label: label.to_string(),
        button_type: button_type.to_string(),
        cmd,
        port,
    };
    defs.push(def.clone());
    write_buttons_atomic(&state.buttons_file, &defs)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("write buttons.toml: {e}")))?;

    Ok((
        StatusCode::CREATED,
        Json(ButtonOut {
            id: def.id,
            label: def.label,
            button_type: def.button_type,
            cmd: def.cmd,
            port: def.port,
        }),
    ))
}

/// Type-specific shape validation: returns the normalized `(type, cmd, port)`
/// triple to persist, or a 400. An absent `type` defaults to "agent" so the
/// original `{label, cmd}` payload shape still works unchanged.
///
/// Port 8088 is axum itself - proxying it would recurse
/// (`/preview/8088/preview/...`), so it is rejected here and again at the
/// proxy layer.
fn validate_shape(input: &ButtonInput) -> Result<(String, String, Option<u16>), (StatusCode, String)> {
    let button_type = if input.button_type.is_empty() { "agent" } else { input.button_type.as_str() };
    match button_type {
        "agent" => {
            let cmd = input.cmd.trim();
            if cmd.is_empty() {
                return Err((StatusCode::BAD_REQUEST, "cmd must be non-empty".to_string()));
            }
            if cmd.len() > MAX_LEN {
                return Err((StatusCode::BAD_REQUEST, format!("cmd must be <= {MAX_LEN} chars")));
            }
            Ok(("agent".to_string(), cmd.to_string(), None))
        }
        "web" => match input.port {
            Some(p) if p == 0 || p == 8088 => Err((
                StatusCode::BAD_REQUEST,
                format!("port {p} is not allowed (0 or the app's own 8088)"),
            )),
            // Web buttons store an empty cmd: the pty field is meaningless
            // here, and keeping the key makes hand-edits / round-trips stable.
            Some(p) => Ok(("web".to_string(), String::new(), Some(p))),
            None => Err((StatusCode::BAD_REQUEST, "web buttons require a port".to_string())),
        },
        other => Err((
            StatusCode::BAD_REQUEST,
            format!("unknown button type {other:?} (expected \"agent\" or \"web\")"),
        )),
    }
}

/// Query for GET /api/buttons/probe.
#[derive(Debug, Deserialize)]
pub struct ProbeQuery {
    pub port: String,
}

/// Response body for GET /api/buttons/probe.
#[derive(Debug, Serialize)]
pub struct ProbeOut {
    pub listening: bool,
}

/// TCP-probe `127.0.0.1:<port>` so the register dialog can warn about a port
/// with no dev server behind it before the user commits (UX: registering a
/// dead port silently yields a grey button / 502 preview). Non-blocking: a
/// `listening:false` is a hint, not an error. Same port rules as
/// `validate_shape` (1-65535, 8088 is the app itself), same 400 on violation,
/// same 400ms timeout as the manifest's liveness probe in `config.rs`.
pub async fn probe_port(
    Query(q): Query<ProbeQuery>,
) -> Result<Json<ProbeOut>, (StatusCode, String)> {
    let Ok(p) = q.port.parse::<u16>() else {
        return Err((StatusCode::BAD_REQUEST, format!("port {:?} is not a number", q.port)));
    };
    if p == 0 || p == 8088 {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("port {p} is not allowed (0 or the app's own 8088)"),
        ));
    }
    let target = format!("127.0.0.1:{p}");
    let connect = tokio::net::TcpStream::connect(&target);
    let listening = matches!(
        tokio::time::timeout(Duration::from_millis(400), connect).await,
        Ok(Ok(_))
    );
    Ok(Json(ProbeOut { listening }))
}

pub async fn delete_button(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<StatusCode, StatusCode> {
    let _guard = state.file_lock.lock().await;
    let mut defs = parse_button_defs(&state.buttons_file);
    let before = defs.len();
    defs.retain(|d| d.id != id);
    if defs.len() == before {
        return Err(StatusCode::NOT_FOUND);
    }
    write_buttons_atomic(&state.buttons_file, &defs).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Slugify a label into a stable id: lowercase, runs of non-alphanumeric -> `-`,
/// trimmed of leading/trailing `-`. Empty result (e.g. label was all symbols)
/// falls back to `button`.
fn slugify(label: &str) -> String {
    let s: String = label
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let s = s.trim_matches('-');
    if s.is_empty() {
        "button".to_string()
    } else {
        s.to_string()
    }
}

/// De-duplicate a base id against existing defs: `htop`, `htop-2`, `htop-3`, ...
fn unique_id(base: String, defs: &[ButtonDef]) -> String {
    if !defs.iter().any(|d| d.id == base) {
        return base;
    }
    let mut n = 2;
    loop {
        let candidate = format!("{base}-{n}");
        if !defs.iter().any(|d| d.id == candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Atomically write the button defs to `path` (temp file in the same dir, then
/// rename). Also `mkdir -p` the parent (first `make up` on an empty volume).
fn write_buttons_atomic(path: &Path, defs: &[ButtonDef]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let toml = toml::to_string(&ButtonsFile {
        button: defs.to_vec(),
    })
    .map_err(std::io::Error::other)?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, toml)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("htop"), "htop");
        assert_eq!(slugify("My Tool!"), "my-tool");
        assert_eq!(slugify("  spaced  "), "spaced");
        assert_eq!(slugify("???"), "button");
    }

    #[test]
    fn unique_id_no_collision() {
        let defs = vec![];
        assert_eq!(unique_id("htop".into(), &defs), "htop");
    }

    #[test]
    fn unique_id_appends_suffix() {
        let defs = vec![ButtonDef {
            id: "htop".into(),
            label: "htop".into(),
            button_type: "agent".into(),
            cmd: "htop".into(),
            port: None,
        }];
        assert_eq!(unique_id("htop".into(), &defs), "htop-2");
        let defs2 = vec![
            ButtonDef { id: "htop".into(), label: "x".into(), button_type: "agent".into(), cmd: "x".into(), port: None },
            ButtonDef { id: "htop-2".into(), label: "x".into(), button_type: "agent".into(), cmd: "x".into(), port: None },
        ];
        assert_eq!(unique_id("htop".into(), &defs2), "htop-3");
    }

    fn input(label: &str, cmd: &str, button_type: &str, port: Option<u16>) -> ButtonInput {
        ButtonInput {
            label: label.into(),
            cmd: cmd.into(),
            button_type: button_type.into(),
            port,
        }
    }

    #[test]
    fn validate_agent_unchanged() {
        // Original payload shape (no type field -> agent) keeps working.
        let (t, cmd, port) = validate_shape(&input("htop", "htop", "", None)).unwrap();
        assert_eq!((t.as_str(), cmd.as_str(), port), ("agent", "htop", None));
        let (t, _, port) = validate_shape(&input("htop", "htop", "agent", None)).unwrap();
        assert_eq!((t.as_str(), port), ("agent", None));
    }

    #[test]
    fn validate_web_requires_port() {
        let err = validate_shape(&input("webby", "", "web", None)).unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        let err = validate_shape(&input("webby", "", "web", Some(0))).unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        // 8088 is axum itself: rejected to avoid proxy self-recursion.
        let err = validate_shape(&input("webby", "", "web", Some(8088))).unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn validate_web_normalizes() {
        let (t, cmd, port) = validate_shape(&input("webby", "ignored", "web", Some(5173))).unwrap();
        assert_eq!((t.as_str(), cmd.as_str(), port), ("web", "", Some(5173)));
    }

    #[test]
    fn validate_unknown_type_rejected() {
        let err = validate_shape(&input("x", "y", "page", None)).unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn validate_agent_cmd_still_required() {
        let err = validate_shape(&input("x", "", "", None)).unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
    }

    // --- probe_port ---------------------------------------------------------

    #[tokio::test]
    async fn probe_live_port_listening() {
        // Bind an ephemeral listener so the test has a guaranteed-open port.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let out = probe_port(Query(ProbeQuery { port: port.to_string() }))
            .await
            .unwrap();
        assert!(out.listening);
    }

    #[tokio::test]
    async fn probe_dead_port_not_listening() {
        // 65535: the only u16 port nothing may listen on by construction here
        // (port 0 is a probe-parameter error, not an address to dial).
        let out = probe_port(Query(ProbeQuery { port: "65535".into() }))
            .await
            .unwrap();
        assert!(!out.listening);
    }

    #[tokio::test]
    async fn probe_rejects_invalid_ports() {
        for bad in ["0", "8088", "abc", "-1", "65536", ""] {
            let err = probe_port(Query(ProbeQuery { port: bad.into() }))
                .await
                .unwrap_err();
            assert_eq!(err.0, StatusCode::BAD_REQUEST, "port {bad:?} must be 400");
        }
    }
}
