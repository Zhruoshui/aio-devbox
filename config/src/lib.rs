// aio-config library surface (sandbox-mgr task Phase 0).
//
// The scenario configurator's reusable halves live here: `scenario` (the
// single decoder of `scenarios/<id>/scenario.toml`), `manifest` (the single
// owner of the `.aio/enabled.toml` contract), and `gen` (the Dockerfile.base
// assembler). The `tui` subcommand stays bin-only (main.rs) — it is terminal
// UI wiring with no library consumer.
//
// Both library and bin previously shared these modules via one bin crate;
// lib-ification keeps the modules themselves untouched (same file, same
// `crate::` paths now resolved through the lib target).

pub mod gen;
pub mod manifest;
pub mod scenario;
