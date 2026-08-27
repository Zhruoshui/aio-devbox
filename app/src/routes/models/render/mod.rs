// Per-agent renderers (design §4). Each renderer takes an injectable home
// `&Path` (tests use temp dirs; routes pass `home_dir()`) and the canonical
// config, and returns an `ApplyResult` describing which files were written
// (with backup paths) and which failed (with messages).
//
// All apply paths hold the process-wide `models_lock` (routes/models/mod.rs
// `apply_agent`), so the renderers don't need their own serialization.

pub mod claude;
pub mod codex;
pub mod common;
pub mod opencode;
pub mod pi;

pub use common::{ApplyResult, Agent, ProviderPatch, home_dir};
