// aio-mgr: multi-sandbox management control plane (sandbox-mgr Phase 1).
//
// Serves the mgr HTTP API (routes.rs) over SQLite state (db.rs) and drives
// per-sandbox docker compose projects (docker.rs / composegen.rs / jobs.rs).
// Design: .trellis/tasks/09-08-sandbox-mgr-tui/design.md §3.
//
// Phase 3: also serves the mgr-web SPA statically (the same ServeDir +
// index.html-fallback pattern as app/src/main.rs). The API routes stay
// explicit and take precedence; every other path falls back to the SPA tree.
// mgr-web itself keeps its views in in-memory state (no router library), so
// the fallback is hard-load robustness rather than routing support, and the
// /api seam routes in routes.rs keep unmatched API paths off the SPA.
//
// Two run forms (prd D3), identical code:
//   bare-metal  MGR_REPO=. MGR_DATA=mgrData cargo run -p aio-mgr
//   containerized  mgr compose stack; repo ro-mounted, docker.sock mounted
// Env:
//   MGR_BIND  listen address      (default 0.0.0.0:8089)
//   MGR_REPO  AIO repo root       (default cwd)
//   MGR_DATA  runtime data root   (default <repo>/mgr-data)
//   MGR_WEB_DIR mgr-web dist tree (default <repo>/mgr-web/dist; the image
//               bakes it to /app/static - mgr/Dockerfile web-builder stage)

mod caddy;
mod composegen;
mod db;
mod docker;
mod envhash;
mod jobs;
mod models;
mod proxy;
mod routes;
mod state;
mod usage;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use tower_http::services::{ServeDir, ServeFile};

use state::AppState;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let repo = PathBuf::from(std::env::var("MGR_REPO").unwrap_or_else(|_| ".".into()))
        .canonicalize()
        .context("MGR_REPO")?;
    let data = match std::env::var("MGR_DATA") {
        Ok(p) => PathBuf::from(p),
        Err(_) => repo.join("mgr-data"),
    };
    let web_dir = match std::env::var("MGR_WEB_DIR") {
        Ok(p) => PathBuf::from(p),
        Err(_) => repo.join("mgr-web").join("dist"),
    };
    let bind: std::net::SocketAddr = std::env::var("MGR_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8089".into())
        .parse()
        .context("MGR_BIND")?;

    // A missing dist tree is a soft condition (bare-metal form before any
    // `npm run build`): warn and keep serving the API, so a half-configured
    // host gets a clear diagnostic instead of a broken proxy.
    if !web_dir.join("index.html").is_file() {
        tracing::warn!(
            "mgr-web dist not found at {} (MGR_WEB_DIR overrides); serving API only",
            web_dir.display()
        );
    }

    let conn = db::open(&data.join("state.db"))?;
    let orphans = db::fail_orphan_jobs(&conn)?;
    if orphans > 0 {
        tracing::warn!("marked {orphans} orphaned job(s) from a previous run as error");
    }

    let state = Arc::new(AppState::new(repo.clone(), data.clone(), conn));

    // Shared external network every generated sandbox joins (design §1).
    // Idempotent; failure is fatal at boot (routing is core to mgr's job).
    docker::ensure_network("aio-mgr-net")
        .await
        .context("ensure aio-mgr-net")?;

    // Converge the total-gateway Caddyfile at boot: sandboxes deleted while
    // mgr was down leave stale site blocks; regenerating heals the drift.
    // Reload failures are already reported inside (kv + log), not fatal.
    if let Err(e) = caddy::regenerate(&state).await {
        tracing::warn!("startup Caddyfile convergence: {e:#}");
    }

    // Static mgr-web tree (Phase 3): mounted on EXPLICIT routes (`/` plus the
    // `/*path` catch-all - matchit cannot hang a catch-all on the bare root,
    // same reason the /api seam lists /api and /api/ explicitly), NOT via
    // fallback_service: routes.rs' default fallback is what keeps the
    // trailing-slash forms of /api paths (matchit 0.7.3 gap, e.g.
    // /api/sbx/<name>/) off the SPA, and a fallback_service here would
    // override it back to serving HTML. ServeDir's own index.html fallback
    // keeps the hard-load robustness: any non-file path still serves the SPA.
    let serve_dir =
        ServeDir::new(&web_dir).fallback(ServeFile::new(web_dir.join("index.html")));

    let app = routes::router()
        .route("/", axum::routing::any_service(serve_dir.clone()))
        .route("/*path", axum::routing::any_service(serve_dir))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(
        "aio-mgr listening on {bind} (repo {}, data {}, web {})",
        repo.display(),
        data.display(),
        web_dir.display()
    );
    axum::serve(listener, app).await.context("serve")?;
    Ok(())
}
