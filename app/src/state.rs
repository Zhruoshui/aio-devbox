// Shared application state. Built once at startup and shared across handlers
// via `axum::State`.
//
// - `builtin`: services parsed from the baked-in `services.toml` (immutable).
// - `buttons_file`: path to the runtime `buttons.toml` on the workspace volume
//   (`AIO_BUTTONS_FILE` env, default `/root/.aio/buttons.toml`). User-
//   registered buttons; read per manifest request, written under `file_lock`.
// - `models_file`: path to the canonical model config (`AIO_MODELS_FILE` env,
//   default `/root/.aio/models.json`). Read/written under `models_lock`.
// - `path_cache`: cached login-shell PATH for command_exists (TTL-refreshed).
// - `file_lock`: serializes read-modify-write on buttons.toml so concurrent
//   POST/DELETE can't interleave.
// - `models_lock`: serializes read-modify-write on models.json (design §3).
// - `http`: a shared reqwest Client reused by the model discovery/test routes.
//   Process-wide; per-request timeouts are set on the RequestBuilder, not the
//   client, so different routes can pick their own budget.
// - `stats`: container resource snapshot (CPU/MEM/DISK) refreshed by the
//   background sampler in routes/stats.rs (2s); read per /api/stats request.

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use crate::config::{PathCache, Service};
use crate::routes::stats::StatsSnapshot;

#[derive(Clone)]
pub struct AppState {
    pub builtin: Arc<Vec<Service>>,
    pub buttons_file: PathBuf,
    pub models_file: PathBuf,
    pub path_cache: Arc<RwLock<PathCache>>,
    pub file_lock: Arc<Mutex<()>>,
    pub models_lock: Arc<Mutex<()>>,
    pub http: reqwest::Client,
    pub stats: Arc<RwLock<StatsSnapshot>>,
}

impl AppState {
    pub fn new(builtin: Vec<Service>, buttons_file: PathBuf, models_file: PathBuf) -> Self {
        let http = reqwest::Client::builder()
            // No default timeout: each route sets its own via RequestBuilder::timeout.
            // Pool with keep-alive so repeated discover/test calls reuse conns.
            .pool_idle_timeout(std::time::Duration::from_secs(60))
            .build()
            .expect("reqwest client build");
        Self {
            builtin: Arc::new(builtin),
            buttons_file,
            models_file,
            path_cache: Arc::new(RwLock::new(PathCache::default())),
            file_lock: Arc::new(Mutex::new(())),
            models_lock: Arc::new(Mutex::new(())),
            http,
            stats: Arc::new(RwLock::new(StatsSnapshot::default())),
        }
    }
}
