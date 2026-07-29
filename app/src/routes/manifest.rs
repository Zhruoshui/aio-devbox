// GET /api/manifest - live service manifest consumed by the workspace UI.
//
// Computed per request so it reflects current container availability (a
// type=web service whose compose profile is not started reports enabled=false).

use axum::{extract::State, Json};

use crate::config::{build_manifest, Manifest};
use crate::state::AppState;

pub async fn manifest(State(state): State<AppState>) -> Json<Manifest> {
    let manifest = build_manifest(&state.services).await;
    Json(manifest)
}
