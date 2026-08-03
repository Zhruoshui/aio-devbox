// aio-config: scenario configurator for the AIO dev sandbox (design §1).
//
// Two subcommands share one scenario-discovery + manifest module:
//   tui - interactive ratatui checkbox picker; writes .aio/enabled.toml.
//   gen - build-time; assembles Dockerfile.base from head + enabled scenario
//         fragments + tail (design §2.4).
//
// Both operate on a repo root (`--repo`) so the Makefile mounts the repo once
// and calls either mode. The TUI never touches Dockerfile.base.head/tail; gen
// never starts a terminal. Shared code lives in scenario.rs / manifest.rs
// (single owner of each cross-boundary payload - cross-layer-thinking-guide).

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

mod gen;
mod manifest;
mod scenario;
mod tui;

#[derive(Parser)]
#[command(name = "aio-config", about = "AIO sandbox scenario configurator")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Interactive scenario picker; writes the selection manifest.
    Tui {
        /// Repo root (contains scenarios/ and .aio/enabled.toml).
        #[arg(long)]
        repo: PathBuf,
    },
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
        Cmd::Tui { repo } => tui::run(&repo),
        Cmd::Gen { repo } => gen::run(&repo),
    }
}
