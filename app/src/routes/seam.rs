// Reserved-seam catch-all.
//
// `/api` (minus the real `/api/manifest` and `/api/term/ws`), `/v1`, and `/mcp`
// are reserved for a future agent/SDK API surface (design §13/§14C). Until then
// any method on these paths returns 502 with a stub JSON body, proving the seam
// is not swallowed by code-server or the static SPA.
//
// Wired as `any(seam)` on `/{*rest}` catch-all routes for each prefix, so it
// covers every method and every sub-path (including the bare prefix and the
// trailing slash). The static `/api/manifest` route is ranked higher than the
// `/api/{*rest}` catch-all by the router, so the manifest is never swallowed.

use axum::{http::StatusCode, Json};
use serde_json::{json, Value};

pub async fn seam() -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_GATEWAY,
        Json(json!({ "error": "seam reserved" })),
    )
}
