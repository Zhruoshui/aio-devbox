// docker / docker-compose CLI wrapper (sandbox-mgr Phase 1, design §3.4).
//
// D2: every docker interaction goes through the CLI (`docker ...` /
// `docker compose ...`), never the Docker API - the generated compose files
// must stay diffable and hand-operable on the host. All structured output is
// parsed from `--format json` (never text grep, design §3.4); a parse failure
// is an error, not an empty list (design §7).
//
// All commands inherit the environment (DOCKER_HOST etc.), so bare-metal and
// socket-mounted-container forms behave identically (D3).

use std::path::Path;
use std::process::Stdio;

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use tokio::process::Command;

/// Run `docker build` with the repo root as context; streams progress to the
/// returned String instead of the terminal (job log).
pub async fn build(context: &Path, dockerfile: &Path, tag: &str, build_args: &[(&str, &str)]) -> Result<String> {
    let mut args: Vec<String> = vec![
        "build".into(),
        "-f".into(),
        dockerfile.display().to_string(),
        "--tag".into(),
        tag.into(),
    ];
    for (k, v) in build_args {
        args.push("--build-arg".into());
        args.push(format!("{k}={v}"));
    }
    args.push(context.display().to_string());
    let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    run_capture("docker", &args_ref).await
}

/// Idempotent `docker network create` (design §1): "already exists" is
/// success. Plain-string match here (not `--format json`) because network
/// create has no structured output; the check is docker's own error text.
pub async fn ensure_network(name: &str) -> Result<()> {
    let out = tokio::process::Command::new("docker")
        .args(["network", "create", name])
        .output()
        .await
        .context("docker network create")?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("already exists") {
        Ok(())
    } else {
        bail!("docker network create {name}: {stderr}")
    }
}

/// Profile flags every mgr compose lifecycle command carries: the generated
/// sandbox compose gates code-server / vnc behind profiles (same shape as the
/// repo compose, design §3.5), but mgr sandboxes build ALL images and are
/// managed as a unit - the workbench without its panes is half a product, so
/// up starts them all (A3) and down must see them to tear them down.
const SANDBOX_PROFILES: [&str; 4] = ["--profile", "code-server", "--profile", "vnc"];

/// `docker compose -p <project> -f <file> up -d`. Modest -d output.
pub async fn compose_up(project: &str, compose_file: &Path, force_recreate: bool) -> Result<String> {
    let mut args = compose_prefix(project, compose_file);
    args.extend_from_slice(&SANDBOX_PROFILES);
    args.push("up");
    args.push("-d");
    if force_recreate {
        args.push("--force-recreate");
    }
    run_capture("docker", &args).await
}

pub async fn compose_down(project: &str, compose_file: &Path, volumes: bool) -> Result<String> {
    let mut args = compose_prefix(project, compose_file);
    args.extend_from_slice(&SANDBOX_PROFILES);
    args.push("down");
    if volumes {
        args.push("-v");
    }
    run_capture("docker", &args).await
}

pub async fn compose_restart(project: &str, compose_file: &Path) -> Result<String> {
    let mut args = compose_prefix(project, compose_file);
    args.extend_from_slice(&SANDBOX_PROFILES);
    args.push("restart");
    run_capture("docker", &args).await
}

pub async fn compose_stop(project: &str, compose_file: &Path) -> Result<String> {
    let mut args = compose_prefix(project, compose_file);
    args.extend_from_slice(&SANDBOX_PROFILES);
    args.push("stop");
    run_capture("docker", &args).await
}

/// `docker exec <container> caddy reload --config <path>` - the containerized
/// mgr stack's reload channel (caddy.rs Phase 2). Goes through the same
/// captured-run path as everything else so errors surface with stderr tails.
pub async fn caddy_reload_in_container(container: &str, config_path: &str) -> Result<String> {
    run_capture(
        "docker",
        &["exec", container, "caddy", "reload", "--config", config_path],
    )
    .await
}

fn compose_prefix<'a>(project: &'a str, compose_file: &'a Path) -> Vec<&'a str> {
    vec![
        "compose",
        "-p",
        project,
        "-f",
        // leak-free: display() of a Path with no weird chars; compose paths
        // under mgr-data never contain spaces (name slug charset guarantees).
        compose_file.to_str().unwrap_or_default(),
    ]
}

/// One row of `docker compose ps --format json`. compose v2 emits lowercase
/// keys (name/service/state/status) and compose 5.x PascalCase
/// (Name/Service/State/Status); accept both via aliases.
#[derive(Debug, Deserialize)]
pub struct ComposePsEntry {
    // NB: no "Names" alias - compose 5.x emits BOTH Name and Names, and two
    // aliases on one field is a serde "duplicate field" error.
    #[serde(default, alias = "Name")]
    pub name: String,
    #[serde(default, alias = "Service")]
    pub service: String,
    #[serde(default, alias = "State")]
    pub state: String,
    #[serde(default, alias = "Status")]
    pub status: String,
}

/// Live status of a sandbox project's services. `ps --format json` output
/// shape varies by compose version: a JSON ARRAY (compose v2 early), one
/// JSON object per LINE (v2.2x, observed on compose 5.2.0), or a single
/// object (v1). Accept all three. Parse failure = error (design §7), never
/// a silent empty list.
pub async fn compose_ps(project: &str, compose_file: &Path) -> Result<Vec<ComposePsEntry>> {
    let mut args = compose_prefix(project, compose_file);
    args.push("ps");
    args.push("--all");
    args.push("--format");
    args.push("json");
    let out = run_capture("docker", &args).await?;
    let trimmed = out.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    if trimmed.starts_with('[') {
        let entries: Vec<ComposePsEntry> = serde_json::from_str(trimmed)
            .with_context(|| format!("compose ps: parse array failed: {}", head(trimmed)))?;
        Ok(entries)
    } else if trimmed.contains('\n') {
        // JSONL: one object per line (one per service).
        trimmed
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                serde_json::from_str(l)
                    .with_context(|| format!("compose ps: parse line failed: {}", head(l)))
            })
            .collect()
    } else {
        let one: ComposePsEntry = serde_json::from_str(trimmed)
            .with_context(|| format!("compose ps: parse object failed: {}", head(trimmed)))?;
        Ok(vec![one])
    }
}

/// `docker images` existence check (skipping a build when the tag exists).
pub async fn image_exists(tag: &str) -> Result<bool> {
    let out = run_capture(
        "docker",
        &["image", "inspect", "--format", "{{.Id}}", tag],
    )
    .await;
    match out {
        Ok(_) => Ok(true),
        Err(e) => {
            // `docker image inspect` on a missing tag exits 1 with "No such
            // image" / "No such object" - the only benign failures; anything
            // else is a real error. Case-insensitive: docker's wording has
            // flipped between releases ("No such image:" vs "no such image:").
            let msg = format!("{e:#}").to_lowercase();
            if msg.contains("no such object") || msg.contains("no such image") {
                Ok(false)
            } else {
                Err(e)
            }
        }
    }
}

/// Captured run: stdout on success; anyhow error with stderr tail on failure.
async fn run_capture(program: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .with_context(|| format!("spawn {program}"))?;
    if !out.status.success() {
        bail!(
            "{program} {} exited {:?}: {}",
            args.join(" "),
            out.status.code(),
            tail_str(&String::from_utf8_lossy(&out.stderr), 2000)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}


fn tail_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("...{}", &s[s.len() - max..])
    }
}

fn head(s: &str) -> String {
    s.chars().take(120).collect()
}

