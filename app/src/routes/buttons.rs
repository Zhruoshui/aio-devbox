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

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::config::{parse_button_defs, ButtonDef, ButtonsFile};
use crate::state::AppState;

const MAX_LEN: usize = 64;

/// Request body for POST /api/buttons.
#[derive(Debug, Deserialize)]
pub struct ButtonInput {
    pub label: String,
    pub cmd: String,
}

/// Response body for POST /api/buttons.
#[derive(Debug, Serialize)]
pub struct ButtonOut {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub button_type: String,
    pub cmd: String,
}

pub async fn create_button(
    State(state): State<AppState>,
    Json(input): Json<ButtonInput>,
) -> Result<(StatusCode, Json<ButtonOut>), (StatusCode, String)> {
    let label = input.label.trim();
    let cmd = input.cmd.trim();
    if label.is_empty() || cmd.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "label and cmd must be non-empty".to_string(),
        ));
    }
    if label.len() > MAX_LEN || cmd.len() > MAX_LEN {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("label and cmd must be <= {MAX_LEN} chars"),
        ));
    }

    let _guard = state.file_lock.lock().await;
    let mut defs = parse_button_defs(&state.buttons_file);
    let id = unique_id(slugify(label), &defs);
    let def = ButtonDef {
        id: id.clone(),
        label: label.to_string(),
        button_type: "agent".to_string(),
        cmd: cmd.to_string(),
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
        }),
    ))
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
        }];
        assert_eq!(unique_id("htop".into(), &defs), "htop-2");
        let defs2 = vec![
            ButtonDef { id: "htop".into(), label: "x".into(), button_type: "agent".into(), cmd: "x".into() },
            ButtonDef { id: "htop-2".into(), label: "x".into(), button_type: "agent".into(), cmd: "x".into() },
        ];
        assert_eq!(unique_id("htop".into(), &defs2), "htop-3");
    }
}
