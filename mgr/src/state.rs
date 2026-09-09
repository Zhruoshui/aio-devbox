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
    /// Shared reqwest client for the Phase 4 model routes (discover/test
    /// probes + the models.dev catalog fetch) and the /api/usage sandbox
    /// fan-out. Built once; per-request timeouts live on the
    /// RequestBuilder (app/src/state.rs pattern) so each route picks its
    /// own budget. reqwest::Client is internally synchronized - no extra
    /// locking around it.
    pub http: reqwest::Client,
    /// /api/usage fan-out cache (Phase 4, usage.rs): maps `<name>:<window>`
    /// to a cached per-sandbox result, TTL 30s. Behind a std Mutex like
    /// `db`: entries are small owned values, cloned-and-released inside
    /// usage.rs helpers, and the lock is NEVER held across an await (the
    /// fan-out itself runs lock-free; only the map read/update take it).
    pub usage_cache: Arc<Mutex<HashMap<String, CachedUsage>>>,
}

/// One cached /api/usage per-sandbox result (usage.rs). `at` drives the TTL.
#[derive(Debug, Clone)]
pub struct CachedUsage {
    /// Insertion instant (TTL check: `at.elapsed() < 30s`).
    pub at: std::time::Instant,
    /// Serialized per-sandbox entry (the exact JSON the response carries:
    /// `{name, error, usage}` - errors are cached too, so a down sandbox
    /// does not get re-probed on every mgr-web poll tick).
    pub entry: serde_json::Value,
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
        // No default timeout (each route sets its own via
        // RequestBuilder::timeout); keep-alive pool so repeated discover /
        // test / usage polls reuse connections (app/src/state.rs pattern).
        let http = reqwest::Client::builder()
            .pool_idle_timeout(std::time::Duration::from_secs(60))
            .build()
            .expect("reqwest client build");
        AppState {
            repo,
            data,
            db: Arc::new(Mutex::new(db)),
            jobs: Arc::new(Mutex::new(HashMap::new())),
            http,
            usage_cache: Arc::new(Mutex::new(HashMap::new())),
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

    /// Test constructor (usage.rs / models.rs / proxy.rs): an in-memory db
    /// with the FULL schema + fresh caches, no filesystem side effects. The
    /// reqwest client is real but built with proxying DISABLED: tests dial
    /// hostnames that must not resolve (an unreachable upstream is itself
    /// an assertion target, proxy.rs), and a dev machine's proxy env would
    /// otherwise answer single-label aliases on the proxy's own terms.
    /// Production mgr keeps the env-aware client (contract: rustls-tls keeps
    /// reqwest env-proxy aware) - this is a test-only deviation.
    #[cfg(test)]
    pub fn new_for_test() -> Self {
        let conn = rusqlite::Connection::open_in_memory().expect("open in-memory db");
        crate::db::init_schema(&conn).expect("init test schema");
        let http = reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("no-proxy reqwest client build");
        AppState {
            repo: PathBuf::from("/tmp"),
            data: PathBuf::from("/tmp"),
            db: Arc::new(Mutex::new(conn)),
            jobs: Arc::new(Mutex::new(HashMap::new())),
            http,
            usage_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}
