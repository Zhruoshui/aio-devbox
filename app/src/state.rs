// Shared application state. Built once at startup and shared across handlers
// via `axum::State`.
//
// - `builtin`: services parsed from the baked-in `services.toml` (immutable).
// - `buttons_file`: path to the runtime `buttons.toml` on the workspace volume
//   (`AIO_BUTTONS_FILE` env, default `/home/gem/.aio/buttons.toml`). User-
//   registered buttons; read per manifest request, written under `file_lock`.
// - `path_cache`: cached login-shell PATH for command_exists (TTL-refreshed).
// - `file_lock`: serializes read-modify-write on buttons.toml so concurrent
//   POST/DELETE can't interleave.
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
    pub path_cache: Arc<RwLock<PathCache>>,
    pub file_lock: Arc<Mutex<()>>,
    pub stats: Arc<RwLock<StatsSnapshot>>,
}

impl AppState {
    pub fn new(builtin: Vec<Service>, buttons_file: PathBuf) -> Self {
        Self {
            builtin: Arc::new(builtin),
            buttons_file,
            path_cache: Arc::new(RwLock::new(PathCache::default())),
            file_lock: Arc::new(Mutex::new(())),
            stats: Arc::new(RwLock::new(StatsSnapshot::default())),
        }
    }
}
