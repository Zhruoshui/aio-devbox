// GET /api/manifest - live service manifest consumed by the workspace UI.
//
// Computed per request so it reflects current availability:
//   - type=web: whether the compose-profile container is TCP-reachable.
//   - type=agent: whether the command exists on the login-shell PATH.
// Built-in buttons (services.toml) are merged with user buttons (buttons.toml
// on the workspace volume).

use axum::{extract::State, Json};

use crate::config::{build_manifest, load_buttons, merge_services, resolve_path_dirs, Manifest};
use crate::state::AppState;

pub async fn manifest(State(state): State<AppState>) -> Json<Manifest> {
    let user = load_buttons(&state.buttons_file);
    let merged = merge_services(&state.builtin, user);
    let dirs = resolve_path_dirs(&state.path_cache).await;
    let manifest = build_manifest(&merged, &dirs).await;
    Json(manifest)
}
