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

/// Connect a container to a network carrying a mgr alias (Phase 5 adopt,
/// design §3.8). docker has NO "add an alias to an existing membership"
/// operation: when the container is already on the network (daemon replies
/// "... already exists in network ..."), it must be disconnected and
/// reconnected - which also makes re-adopting the same stack idempotent. A
/// failing disconnect is a hard error (the reconnect below would just hit
/// "already exists" again, hiding the real problem).
///
/// Empirical (docker 29.6.1): the "already exists" error fires for RUNNING
/// containers only - a STOPPED container's second connect exits 0 and
/// silently DROPS the new alias. That corner is benign here: aliases (like
/// the membership) persist on the container across stop/start and are only
/// lost on recreate, and every caller connects after `compose up`, when the
/// containers are running.
pub async fn network_connect_alias(network: &str, container: &str, alias: &str) -> Result<String> {
    let out = run_capture(
        "docker",
        &["network", "connect", "--alias", alias, network, container],
    )
    .await;
    match out {
        Ok(o) => Ok(o),
        Err(e) => {
            if !format!("{e:#}").contains("already exists") {
                return Err(e);
            }
            run_capture("docker", &["network", "disconnect", network, container])
                .await
                .with_context(|| format!("disconnect {container} from {network} before aliasing it as {alias}"))?;
            run_capture(
                "docker",
                &["network", "connect", "--alias", alias, network, container],
            )
            .await
        }
    }
}

/// Best-effort `docker network disconnect` - alias cleanup when un-registering
/// an adopted stack. Failures are swallowed with a warning INSIDE this
/// function on purpose: the caller (delete path) must never block on a stale
/// alias, and a leftover alias is self-healing - network_connect_alias's
/// disconnect-reconnect repairs it when the name is registered again.
pub async fn network_disconnect(network: &str, container: &str) {
    if let Err(e) = run_capture("docker", &["network", "disconnect", network, container]).await {
        tracing::warn!(network = network, container = container, error = %format!("{e:#}"), "network disconnect failed (ignored)");
    }
}

/// Profile flags every mgr compose lifecycle command carries: the generated
/// sandbox compose gates code-server / vnc behind profiles (same shape as the
/// repo compose, design §3.5), but mgr sandboxes build ALL images and are
/// managed as a unit - the workbench without its panes is half a product, so
/// up starts them all (A3) and down must see them to tear them down. The
/// external-stack variants below carry them too: the repo stack has the same
/// profile-gated sidecars, and a start/stop that dropped them would maim an
/// adopted workbench.
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

// ── external-stack compose lifecycle (Phase 5 adopt, design §3.8) ────
//
// Adopted stacks are NOT ours, and their compose project name is whatever
// compose derives from the compose FILE's directory (a repo stack started
// via `make up` derives "aio"). Passing mgr's sbx-<name> project would
// target the WRONG project, so these variants carry NO `-p`: the file is
// the identity, and compose derives the project itself.
//
// Sharp edge (verified live): that derivation uses the directory NAME as
// mgr sees it, so the repo must not be re-mounted under a different last
// path segment (a /repo mount makes compose look for project "repo" and
// adopt fails with "no running services"). mgr's own containerized form is
// safe by construction - mgr/compose.yml mounts the repo at its own host
// absolute path (its PATH IDENTITY note) - and bare-metal mgr runs where
// the file really lives.

/// Prefix for external-stack commands (no `-p`, see section comment).
fn compose_file_prefix<'a>(compose_file: &'a Path) -> Vec<&'a str> {
    vec!["compose", "-f", compose_file.to_str().unwrap_or_default()]
}

/// `docker compose -f <file> ps --all --format json` for an external stack.
pub async fn compose_ps_file(compose_file: &Path) -> Result<Vec<ComposePsEntry>> {
    let mut args = compose_file_prefix(compose_file);
    args.extend_from_slice(&["ps", "--all", "--format", "json"]);
    let out = run_capture("docker", &args).await?;
    parse_ps_output(&out)
}

/// `up -d` for an external stack (start of an adopted row). No
/// force-recreate: recreating someone else's stack is not mgr's call.
pub async fn compose_up_file(compose_file: &Path) -> Result<String> {
    let mut args = compose_file_prefix(compose_file);
    args.extend_from_slice(&SANDBOX_PROFILES);
    args.extend_from_slice(&["up", "-d"]);
    run_capture("docker", &args).await
}

pub async fn compose_stop_file(compose_file: &Path) -> Result<String> {
    let mut args = compose_file_prefix(compose_file);
    args.extend_from_slice(&SANDBOX_PROFILES);
    args.extend_from_slice(&["stop"]);
    run_capture("docker", &args).await
}

pub async fn compose_restart_file(compose_file: &Path) -> Result<String> {
    let mut args = compose_file_prefix(compose_file);
    args.extend_from_slice(&SANDBOX_PROFILES);
    args.extend_from_slice(&["restart"]);
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
    parse_ps_output(&out)
}

/// Parse `docker compose ps --all --format json` stdout (the shared body of
/// compose_ps / compose_ps_file - see their comments for the shape contract).
fn parse_ps_output(out: &str) -> Result<Vec<ComposePsEntry>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ps_array_shape() {
        // compose v2 early: one JSON array.
        let out = r#"[{"name":"a-gateway-1","service":"gateway","state":"running","status":"Up"}]"#;
        let ps = parse_ps_output(out).unwrap();
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].service, "gateway");
        assert_eq!(ps[0].state, "running");
    }

    #[test]
    fn parse_ps_jsonl_shape() {
        // compose v2.2x / 5.x: one object per line. Blank lines tolerated.
        let out = "{\"Name\":\"a-gateway-1\",\"Service\":\"gateway\",\"State\":\"running\"}\n\
                   {\"name\":\"a-app-1\",\"service\":\"app\",\"state\":\"exited\"}\n\n";
        let ps = parse_ps_output(out).unwrap();
        assert_eq!(ps.len(), 2);
        // PascalCase aliases resolve to the same fields.
        assert_eq!(ps[0].name, "a-gateway-1");
        assert_eq!(ps[1].service, "app");
        assert_eq!(ps[1].state, "exited");
    }

    #[test]
    fn parse_ps_single_object_shape() {
        let ps = parse_ps_output(r#"{"name":"a-app-1","service":"app","state":"running"}"#).unwrap();
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].name, "a-app-1");
    }

    #[test]
    fn parse_ps_blank_output_is_empty() {
        // No services at all (fresh project, everything down + pruned).
        assert!(parse_ps_output("  \n").unwrap().is_empty());
    }

    #[test]
    fn parse_ps_garbage_is_error_not_empty() {
        // design §7: a parse failure must surface, never read as "gone".
        assert!(parse_ps_output("not json at all").is_err());
        assert!(parse_ps_output("[{\"name\": oops}]").is_err());
    }
}

