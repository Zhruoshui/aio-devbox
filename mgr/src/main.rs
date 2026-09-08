// aio-mgr: multi-sandbox management control plane (sandbox-mgr Phase 1).
//
// Serves the mgr HTTP API (routes.rs) over SQLite state (db.rs) and drives
// per-sandbox docker compose projects (docker.rs / composegen.rs / jobs.rs).
// Design: .trellis/tasks/09-08-sandbox-mgr-tui/design.md §3.
//
// Two run forms (prd D3), identical code:
//   bare-metal  MGR_REPO=. MGR_DATA=mgrData cargo run -p aio-mgr
//   containerized  mgr compose stack; repo ro-mounted, docker.sock mounted
// Env:
//   MGR_BIND  listen address      (default 0.0.0.0:8089)
//   MGR_REPO  AIO repo root       (default cwd)
//   MGR_DATA  runtime data root   (default <repo>/mgr-data)

mod composegen;
mod db;
mod docker;
mod envhash;
mod jobs;
mod routes;
mod state;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};

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
    let bind: std::net::SocketAddr = std::env::var("MGR_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8089".into())
        .parse()
        .context("MGR_BIND")?;

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

    let app = routes::router().with_state(state.clone());
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(
        "aio-mgr listening on {bind} (repo {}, data {})",
        repo.display(),
        data.display()
    );
    axum::serve(listener, app).await.context("serve")?;
    Ok(())
}
