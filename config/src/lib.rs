// aio-config library surface.
//
// The scenario configurator's reusable modules live here: `scenario` (the
// single decoder of `scenarios/<id>/scenario.toml`), `manifest` (the single
// owner of the `.aio/enabled.toml` contract), and `gen` (the Dockerfile.base
// assembler).

pub mod gen;
pub mod manifest;
pub mod scenario;
