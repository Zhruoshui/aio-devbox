// Shared app state (sandbox-mgr Phase 1, design §3.2/§3.3).
//
// mgr owns two roots:
//   repo - the AIO repo checkout (scenarios/, Dockerfile.base.head/tail,
//          app|code-server|vnc Dockerfiles). Read-only usage + docker build
//          context. Containerized form mounts it read-only - the build
//          context is shipped over the docker socket, so ro is enough.
//   data - mgr-data/, mgr's own runtime state (state.db + per-sandbox
//          instances/<name>/{Dockerfile.base, compose.yml, gateway/}).
//          Never committed (gitignored).
//
// The SQLite connection sits behind a std Mutex: rusqlite is sync and !Sync,
// and mgr only ever issues microsecond-scale queries. The lock is acquired
// inside db.rs helpers and ALWAYS dropped before the caller awaits anything
// (no MutexGuard is ever held across an await - that would deadlock the
// runtime's blocking of the thread).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

/// Shared handle passed to every handler.
#[derive(Clone)]
pub struct AppState {
    /// Repo root (see module comment). Env `MGR_REPO`, default cwd.
    pub repo: PathBuf,
    /// Runtime data root. Env `MGR_DATA`, default `<repo>/mgr-data`.
    pub data: PathBuf,
    /// SQLite state.db. Single connection behind a std Mutex (module comment).
    pub db: Arc<Mutex<Connection>>,
    /// Live job status, keyed by job id. Mirrors the jobs table for
    /// O(1) polling without touching SQLite; the table is the durable copy
    /// (jobs survive restart as `error: interrupted`). Values are shared
    /// handles so the running task and the poller read the same cell.
    pub jobs: Arc<Mutex<HashMap<i64, Arc<tokio::sync::Mutex<JobShared>>>>>,
}

/// One job's shared status (design §3.4: tokio task + in-memory + SQLite).
#[derive(Debug, Clone, serde::Serialize)]
pub struct JobShared {
    pub id: i64,
    /// "create" | "recreate" | "delete"
    pub kind: String,
    /// Sandbox name the job operates on (denormalized for the poller UI).
    pub sandbox: Option<String>,
    /// "running" | "ok" | "error"
    pub status: String,
    pub error: Option<String>,
    /// Tail of the build/compose output for the progress view (design §3.6).
    pub log: String,
}

impl AppState {
    pub fn new(repo: PathBuf, data: PathBuf, db: Connection) -> Self {
        AppState {
            repo,
            data,
            db: Arc::new(Mutex::new(db)),
            jobs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// mgr-data/instances/sbx-<name>/ - per-sandbox generated artifacts (D2).
    /// The sbx- prefix is load-bearing: compose derives the default project
    /// name from the compose file's directory, so naming the dir sbx-<name>
    /// makes host-side hand-over work WITHOUT -p (A10:
    /// `docker compose -f .../instances/sbx-x/compose.yml ps` just works).
    pub fn instance_dir(&self, name: &str) -> PathBuf {
        self.data.join("instances").join(format!("sbx-{name}"))
    }
}
