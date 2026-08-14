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

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use crate::config::{PathCache, Service};

#[derive(Clone)]
pub struct AppState {
    pub builtin: Arc<Vec<Service>>,
    pub buttons_file: PathBuf,
    pub path_cache: Arc<RwLock<PathCache>>,
    pub file_lock: Arc<Mutex<()>>,
}

impl AppState {
    pub fn new(builtin: Vec<Service>, buttons_file: PathBuf) -> Self {
        Self {
            builtin: Arc::new(builtin),
            buttons_file,
            path_cache: Arc::new(RwLock::new(PathCache::default())),
            file_lock: Arc::new(Mutex::new(())),
        }
    }
}
