// Per-agent renderers (cc-switch mode). Each renderer takes an injectable
// home `&Path` (tests use temp dirs; routes pass `home_dir()`) and the
// agent-specific config, and returns an `ApplyResult` describing which files
// were written (with backup paths) and which failed (with messages).
//
// claude/codex are switch-style (full snapshot replace);
// pi/opencode are incremental (read-modify-merge, all providers coexist).
//
// All apply paths hold the process-wide `models_lock` (routes/models/mod.rs
// `apply_agent`), so the renderers don't need their own serialization.

pub mod claude;
pub mod codex;
pub mod common;
pub mod opencode;
pub mod pi;

pub use common::{home_dir, Agent, ApplyResult};
