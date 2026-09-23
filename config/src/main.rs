// aio-config: scenario configurator for the AIO dev sandbox.
//
// Single subcommand on the shared scenario-discovery + manifest module:
//   gen - build-time; assembles Dockerfile.base from head + enabled scenario
//         fragments + tail.
//
// Operates on a repo root (`--repo`) so the Makefile mounts the repo once
// and calls it. Shared code lives in the aio_config lib
// (scenario.rs / manifest.rs / gen.rs - single owner of each cross-boundary
// payload - cross-layer-thinking-guide).

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use aio_config::gen;

#[derive(Parser)]
#[command(name = "aio-config", about = "AIO sandbox scenario configurator")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Assemble Dockerfile.base from head + enabled fragments + tail.
    Gen {
        /// Repo root (contains .aio/enabled.toml, scenarios/, Dockerfile.base.head/.tail).
        #[arg(long)]
        repo: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Gen { repo } => gen::run(&repo),
    }
}
