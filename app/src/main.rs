// AIO-style dev sandbox - axum app server.
//
// Phase B: data-driven service manifest (services.toml baked in), reserved
// `/api` `/v1` `/mcp` seam stubs, and a static placeholder SPA. The Phase A
// `GET / -> "ok"` is replaced by ServeDir serving the (Phase C: React build)
// static tree.
//
// Phase E: `GET /api/term/ws` - terminal pty WebSocket bridge (terminal.rs +
// pty.rs). Spawns a login shell (or `?cmd=`) as root in /root and
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
    routing::{any, delete, get, post, put},
    Router,
};
use std::path::PathBuf;
use tower_http::services::{ServeDir, ServeFile};

mod config;
mod pty;
mod routes;
mod state;

use routes::{
    buttons::{create_button, delete_button}, manifest::manifest,
    models::{
        apply_agent, catalog::get_catalog, delete_live_provider, discover::discover,
        edit_live_provider, get_agents, get_config, import_pi, put_config,
        sync_live_provider, test::test, usage::usage,
    },
    seam::seam, stats::{stats, spawn_stats_sampler}, terminal::terminal_ws,
};
use state::AppState;

/// Where the static SPA tree lives in the image. Must be outside `/root`
/// (that path is the persistent workspace volume mount point).
const STATIC_DIR: &str = "/app/static";

/// Default location of the user-registered buttons file (on the workspace
/// volume, so it persists across recreate and is shared across browsers).
/// Override with `AIO_BUTTONS_FILE`.
const BUTTONS_FILE: &str = "/root/.aio/buttons.toml";

/// Default location of the canonical model config (providers + agent
/// assignments). Override with `AIO_MODELS_FILE`.
const MODELS_FILE: &str = "/root/.aio/models.json";

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let buttons_file =
        PathBuf::from(std::env::var("AIO_BUTTONS_FILE").unwrap_or_else(|_| BUTTONS_FILE.to_string()));
    let models_file =
        PathBuf::from(std::env::var("AIO_MODELS_FILE").unwrap_or_else(|_| MODELS_FILE.to_string()));
    let state = AppState::new(config::load_services(), buttons_file, models_file);

    // Background cgroup/statvfs sampler feeding GET /api/stats (2s period).
    spawn_stats_sampler(state.clone());

    // Static SPA tree. `/` serves index.html (dir index); unknown paths fall
    // back to index.html (SPA client-side routing, used from Phase C onward).
    let serve_dir = ServeDir::new(STATIC_DIR).fallback(ServeFile::new(format!(
        "{}/index.html",
        STATIC_DIR
    )));

    let app = Router::new()
        // Real route: live service manifest for the workspace UI.
        .route("/api/manifest", get(manifest))
        // Real route: container resource snapshot (CPU/MEM/DISK) for the
        // statusbar footer. Static segment wins over the `/api/*rest`
        // catch-all below, same as `/api/manifest`.
        .route("/api/stats", get(stats))
        // Real route: terminal pty WebSocket bridge (Phase E). Static segments
        // rank above the `/api/*rest` catch-all below, so this wins (same
        // mechanism as `/api/manifest`). Sibling `/api/term/*` still hits the
        // seam.
        .route("/api/term/ws", get(terminal_ws))
        // User-registered buttons: create/delete in buttons.toml on the volume.
        // Static segments win over the `/api/*rest` catch-all. `:id` is axum
        // 0.7 / matchit path-param syntax.
        .route("/api/buttons", post(create_button))
        .route("/api/buttons/:id", delete(delete_button))
        // Model config: canonical store (GET/PUT) + pi import (POST) +
        // M2 discover (POST /v1/models fetch) + test (minimal completion probe) +
        // M3 agent status + apply (render canonical -> native files).
        .route("/api/models/config", get(get_config).put(put_config))
        .route("/api/models/import/pi", post(import_pi))
        .route("/api/models/discover", post(discover))
        .route("/api/models/test", post(test))
        .route("/api/models/agents", get(get_agents))
        .route("/api/models/apply/:agent", post(apply_agent))
        // Live provider management on the incremental agents' native files
        // (08-27-agent-tabs-live-config): field-level edit, delete, sync to
        // the canonical library.
        .route(
            "/api/models/agents/:agent/provider/:id",
            put(edit_live_provider).delete(delete_live_provider),
        )
        .route("/api/models/agents/:agent/sync", post(sync_live_provider))
        // M4: per-(agent,model) token usage aggregation (design §6).
        .route("/api/models/usage", get(usage))
        // models.dev metadata catalog (1h cache, 08-27-provider-form-piweb).
        .route("/api/models/catalog", get(get_catalog))
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

