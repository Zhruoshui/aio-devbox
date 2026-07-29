// AIO-style dev sandbox - axum app server.
//
// Phase B: data-driven service manifest (services.toml baked in), reserved
// `/api` `/v1` `/mcp` seam stubs, and a static placeholder SPA. The Phase A
// `GET / -> "ok"` is replaced by ServeDir serving the (Phase C: React build)
// static tree.
//
// Phase E: `GET /api/term/ws` - terminal pty WebSocket bridge (terminal.rs +
// pty.rs). Spawns a login shell (or `?cmd=`) as uid 1000 in /home/gem and
// bridges it to the client's xterm.js over the WS.
//
// Routing shape (see design §4):
//   GET /api/manifest            -> manifest handler (static route wins)
//   GET /api/term/ws             -> terminal pty WS handler (static route wins)
//   /api, /api/, /api/*          -> 502 seam (any method)
//   /v1, /v1/, /v1/*  /mcp, ...  -> 502 seam (any method)
//   everything else              -> ServeDir (static SPA + index.html fallback)
//
// Why three routes per prefix (`/api`, `/api/`, `/api/*rest`): axum 0.7's
// matchit 0.7.3 catch-all `*rest` matches `/api/<non-empty>` but NOT the bare
// `/api` or the trailing-slash `/api/` (a catch-all needs at least one char
// after the `/`). Without the explicit `/api` and `/api/` routes those two
// would leak to the ServeDir fallback and serve the SPA on a reserved seam
// path. The three forms coexist without conflict (verified against matchit
// 0.7.3). Static routes like `/api/manifest` and `/api/term/ws` are ranked
// higher than the `/api/*rest` catch-all, so they are never swallowed.

use axum::{
    routing::{any, get},
    Router,
};
use tower_http::services::{ServeDir, ServeFile};

mod config;
mod pty;
mod routes;
mod state;

use routes::{manifest::manifest, seam::seam, terminal::terminal_ws};
use state::AppState;

/// Where the static SPA tree lives in the image. Must be outside `/home/gem`
/// (that path is the persistent workspace volume mount point).
const STATIC_DIR: &str = "/app/static";

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let state = AppState::new(config::load_services());

    // Static SPA tree. `/` serves index.html (dir index); unknown paths fall
    // back to index.html (SPA client-side routing, used from Phase C onward).
    let serve_dir = ServeDir::new(STATIC_DIR).fallback(ServeFile::new(format!(
        "{}/index.html",
        STATIC_DIR
    )));

    let app = Router::new()
        // Real route: live service manifest for the workspace UI.
        .route("/api/manifest", get(manifest))
        // Real route: terminal pty WebSocket bridge (Phase E). Static segments
        // rank above the `/api/*rest` catch-all below, so this wins (same
        // mechanism as `/api/manifest`). Sibling `/api/term/*` still hits the
        // seam.
        .route("/api/term/ws", get(terminal_ws))
        // Reserved seams (design §13/§14C): 502 stub on any method. The bare
        // prefix, the trailing slash, and every sub-path are each covered.
        // `/api/manifest` and `/api/term/ws` above are static segments so they
        // win over the `/api/*rest` catch-all - never swallowed.
        .route("/api", any(seam))
        .route("/api/", any(seam))
        .route("/api/*rest", any(seam))
        .route("/v1", any(seam))
        .route("/v1/", any(seam))
        .route("/v1/*rest", any(seam))
        .route("/mcp", any(seam))
        .route("/mcp/", any(seam))
        .route("/mcp/*rest", any(seam))
        .fallback_service(serve_dir)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8088")
        .await
        .expect("failed to bind 0.0.0.0:8088");
    tracing::info!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.expect("server error");
}

