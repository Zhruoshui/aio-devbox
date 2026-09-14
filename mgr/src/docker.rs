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
pub async fn build(
    context: &Path,
    dockerfile: &Path,
    tag: &str,
    build_args: &[(&str, &str)],
) -> Result<String> {
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
                .with_context(|| {
                    format!("disconnect {container} from {network} before aliasing it as {alias}")
                })?;
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

/// Profile flags for a sandbox `up` (D4: code-server is on-demand, never
/// here — the workspace pane pulls it up through mgr's service-start route,
/// compose_service_up below; empirically (compose 5.2.0) an `up` without the
/// code-server profile never starts it and leaves an already-started one
/// running untouched). S1: vnc is per-sandbox optional — a sandbox created
/// without vnc carries no profile flag (its compose has no vnc service, so
/// `--profile vnc` would silently match nothing; harmless but noisy).
/// Adopted up (compose_up_file below) keeps the unconditional vnc flag:
/// the external compose is unknown territory and a compose without a vnc
/// service is unaffected by an unmatched profile.
fn up_profiles(include_vnc: bool) -> Vec<&'static str> {
    if include_vnc {
        vec!["--profile", "vnc"]
    } else {
        Vec::new()
    }
}

/// Profile flags that activate ONLY the on-demand code-server profile
/// (carried by the single-service commands below).
pub const CODE_SERVER_PROFILE: [&str; 2] = ["--profile", "code-server"];

/// Profile flags for every NON-up lifecycle command: stop/restart/down must
/// still SEE code-server to stop it and tear it down cleanly (契约 4's
/// second half - a started code-server dies with the sandbox, never
/// lingers). Empirically (compose 5.2.0) all of them also tolerate a
/// service with NO container at all: a fresh D4 sandbox that never opened a
/// code-server pane stops and tears down without error.
const SANDBOX_PROFILES: [&str; 4] = ["--profile", "code-server", "--profile", "vnc"];

/// `docker compose -p <project> -f <file> [--profile vnc] up -d
/// [--force-recreate]`. D4: vnc comes up with the sandbox (when installed,
/// S1), code-server does not (see compose_service_up).
pub async fn compose_up(
    project: &str,
    compose_file: &Path,
    force_recreate: bool,
    with_vnc: bool,
) -> Result<String> {
    let profiles = up_profiles(with_vnc);
    run_args(&up_args(
        &compose_prefix(project, compose_file),
        &profiles,
        force_recreate,
    ))
    .await
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
fn compose_file_prefix(compose_file: &Path) -> Vec<&str> {
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
/// force-recreate: recreating someone else's stack is not mgr's call. The
/// D4 profile split applies here too - only vnc comes up with the stack; an
/// adopted code-server (if the compose carries one behind a profile) is
/// pulled up on demand like a native one.
pub async fn compose_up_file(compose_file: &Path) -> Result<String> {
    // Adopted stacks are unknown territory: carry the vnc profile (pre-S1
    // unconditional behavior; an adopted compose without a vnc service is
    // unaffected - an unmatched --profile matches nothing).
    let profiles = ["--profile", "vnc"];
    run_args(&up_args(
        &compose_file_prefix(compose_file),
        &profiles,
        false,
    ))
    .await
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

// ── on-demand single-service lifecycle (D4, unified Phase 3) ────────
//
// code-server is profile-gated and deliberately NOT started by `up`
// (up_profiles above); these commands address exactly ONE profile-gated
// service by name. Every one of them carries the service's own profile:
// compose only sees a profile-gated service when its profile is active.

/// `docker compose -p <project> -f <file> --profile code-server up -d
/// code-server` - bring up ONE on-demand service. `up -d <svc>`, NOT
/// `start <svc>`: a profile-gated service that no `up` ever created has no
/// container for `start` to find. This form also HEALS a stale sidecar:
/// after the app container was replaced (a recreate job, or hand-driven
/// compose on the host), a leftover code-server container keeps "running"
/// attached to the REMOVED app's netns (network_mode: service:app) - dead
/// network, and the next full-profile restart errors on it; `up -d <svc>`
/// recreates the service against the CURRENT app container (verified live
/// on compose 5.2.0).
///
/// `up` also starts the service's dependencies (app, via network_mode) -
/// callers must ensure the sandbox itself is running (routes.rs
/// service_start guards that: a stopped stack would come back up
/// half-started, without gateway/vnc).
pub async fn compose_service_up(
    project: &str,
    compose_file: &Path,
    profiles: &[&str],
    service: &str,
) -> Result<String> {
    run_args(&service_up_args(
        &compose_prefix(project, compose_file),
        profiles,
        service,
    ))
    .await
}

/// External-stack variant (no `-p`, the file is the identity): an adopted
/// stack with a code-server service behind a profile is pulled up the same
/// way as a native sandbox's.
pub async fn compose_service_up_file(
    compose_file: &Path,
    profiles: &[&str],
    service: &str,
) -> Result<String> {
    run_args(&service_up_args(
        &compose_file_prefix(compose_file),
        profiles,
        service,
    ))
    .await
}

/// `docker compose -p <project> -f <file> --profile code-server rm --force
/// --stop code-server` - targeted container removal, run by the recreate
/// job BEFORE a force-recreate `up`. Without it the old code-server
/// container survives the app replacement as the running-but-unreachable
/// zombie described on compose_service_up, and the next full-profile
/// restart fails on it ("joining network namespace of container: No such
/// container" - verified live). Idempotent: removing a service with no
/// container at all is a no-op ("No stopped containers", exit 0).
pub async fn compose_service_rm(
    project: &str,
    compose_file: &Path,
    profiles: &[&str],
    service: &str,
) -> Result<String> {
    run_args(&service_rm_args(
        &compose_prefix(project, compose_file),
        profiles,
        service,
    ))
    .await
}

/// `docker exec <container> caddy reload --config <path>` - the containerized
/// mgr stack's reload channel (caddy.rs Phase 2). Goes through the same
/// captured-run path as everything else so errors surface with stderr tails.
pub async fn caddy_reload_in_container(container: &str, config_path: &str) -> Result<String> {
    run_capture(
        "docker",
        &[
            "exec",
            container,
            "caddy",
            "reload",
            "--config",
            config_path,
        ],
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

/// Build the `up -d` argv shared by the project and external-file variants
/// (pure, test-asserted): `[prefix...] [profiles...] up -d [--force-recreate]`.
fn up_args<'a>(prefix: &[&'a str], profiles: &[&'a str], force_recreate: bool) -> Vec<&'a str> {
    let mut args = prefix.to_vec();
    args.extend_from_slice(profiles);
    args.extend_from_slice(&["up", "-d"]);
    if force_recreate {
        args.push("--force-recreate");
    }
    args
}

/// Build the single-service `up -d <svc>` argv (pure, test-asserted).
fn service_up_args<'a>(prefix: &[&'a str], profiles: &[&'a str], service: &'a str) -> Vec<&'a str> {
    let mut args = up_args(prefix, profiles, false);
    args.push(service);
    args
}

/// Build the single-service `rm --force --stop <svc>` argv (pure,
/// test-asserted).
fn service_rm_args<'a>(prefix: &[&'a str], profiles: &[&'a str], service: &'a str) -> Vec<&'a str> {
    let mut args = prefix.to_vec();
    args.extend_from_slice(profiles);
    args.extend_from_slice(&["rm", "--force", "--stop"]);
    args.push(service);
    args
}

/// The one place that turns a built argv into a captured `docker` run.
async fn run_args(args: &[&str]) -> Result<String> {
    run_capture("docker", args).await
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
    let out = run_capture("docker", &["image", "inspect", "--format", "{{.Id}}", tag]).await;
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

/// The image-name prefixes mgr owns and may delete (S3). Anything else is
/// refused by image_rmi — a defensive line against deleting host/user
/// images through the web UI (which only ever lists these prefixes).
pub const OWNED_IMAGE_PREFIXES: [&str; 4] = [
    "sandbox-base-",
    "sandbox-app-",
    "sandbox-code-server-",
    "sandbox-vnc",
];

/// Whether a tag is one mgr may delete (S3 whitelist, see OWNED_IMAGE_PREFIXES).
pub fn is_owned_image_tag(tag: &str) -> bool {
    OWNED_IMAGE_PREFIXES.iter().any(|p| tag.starts_with(p))
}

/// `docker rmi -f <tag>` — forced because base/app/cs share a FROM chain,
/// so deleting a group sequentially would otherwise trip "image is being
/// used by ..." on the intermediate tags. Refuses tags outside the mgr-owned
/// prefixes. The exit-on-missing-tag is NOT an error here (delete job
/// iterates a group where a service may be off — skip it), so callers use
/// image_exists() first.
pub async fn image_rmi(tag: &str) -> Result<String> {
    if !is_owned_image_tag(tag) {
        bail!("refusing to delete non-mgr image {tag:?}");
    }
    run_capture("docker", &["rmi", "-f", tag]).await
}

/// `docker image inspect --format {{.Size}} <tag>` → size in bytes. A missing
/// tag errors (the delete job uses image_exists() to skip; the list route
/// turns an inspect failure into "—").
pub async fn image_size(tag: &str) -> Result<u64> {
    let out = run_capture(
        "docker",
        &["image", "inspect", "--format", "{{.Size}}", tag],
    )
    .await?;
    let trimmed = out.trim();
    let size: u64 = trimmed
        .parse()
        .with_context(|| format!("parse image size {trimmed:?} for {tag}"))?;
    Ok(size)
}

/// `docker builder prune -f` — build-cache cleanup. Returns the full output
/// (the "Total reclaimed space: <X>" line is what the cleanup job reports).
pub async fn builder_prune() -> Result<String> {
    run_capture("docker", &["builder", "prune", "-f"]).await
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
        // Byte slicing must land on a char boundary: build output can carry
        // multibyte UTF-8 (observed: a Chinese char inside '二'), and cutting
        // mid-char would panic with "not a char boundary" (a tokio task
        // abort that silently killed a create job, hiding the real build
        // error). Walk forward from the cut point to the next boundary.
        let mut idx = s.len() - max;
        while idx < s.len() && !s.is_char_boundary(idx) {
            idx += 1;
        }
        format!("...{}", &s[idx..])
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
        let ps =
            parse_ps_output(r#"{"name":"a-app-1","service":"app","state":"running"}"#).unwrap();
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].name, "a-app-1");
    }

    #[test]
    fn parse_ps_blank_output_is_empty() {
        // No services at all (fresh project, everything down + pruned).
        assert!(parse_ps_output("  \n").unwrap().is_empty());
    }

    #[test]
    fn owned_image_tag_whitelist() {
        // S3: only mgr-owned prefixes are deletable through the web UI.
        assert!(is_owned_image_tag("sandbox-base-abcdef123456"));
        assert!(is_owned_image_tag("sandbox-app-abcdef123456"));
        assert!(is_owned_image_tag("sandbox-code-server-abcdef123456"));
        assert!(is_owned_image_tag("sandbox-vnc"));
        assert!(!is_owned_image_tag("ubuntu:24.04"));
        assert!(!is_owned_image_tag("debian:bookworm-slim"));
        assert!(!is_owned_image_tag("node:20"));
        assert!(!is_owned_image_tag("registry.local/my-image"));
        // Prefix must be exact - "sandbox-app2" is NOT "sandbox-app-".
        assert!(!is_owned_image_tag("sandbox-app2-foo"));
    }

    #[test]
    fn parse_ps_garbage_is_error_not_empty() {
        // design §7: a parse failure must surface, never read as "gone".
        assert!(parse_ps_output("not json at all").is_err());
        assert!(parse_ps_output("[{\"name\": oops}]").is_err());
    }

    // ── D4 profile split: argv shapes (pure builders, no docker needed) ──

    #[test]
    fn up_args_carry_only_vnc_profile_when_enabled() {
        // D4 core + S1: `up` with vnc enabled brings vnc (pi agent-browser's
        // resident dependency) but NOT code-server (on-demand via
        // compose_service_up); with vnc disabled no profile flag at all.
        let args = up_args(
            &["compose", "-p", "sbx-dev1", "-f", "/x/compose.yml"],
            &up_profiles(true),
            false,
        );
        assert_eq!(
            args,
            vec![
                "compose",
                "-p",
                "sbx-dev1",
                "-f",
                "/x/compose.yml",
                "--profile",
                "vnc",
                "up",
                "-d",
            ]
        );
        assert!(!args.contains(&"code-server"));
        let no_vnc = up_args(
            &["compose", "-f", "/x/compose.yml"],
            &up_profiles(false),
            false,
        );
        assert_eq!(no_vnc, vec!["compose", "-f", "/x/compose.yml", "up", "-d"]);
    }

    #[test]
    fn up_args_force_recreate_appends_flag() {
        let args = up_args(
            &["compose", "-f", "/x/compose.yml"],
            &up_profiles(true),
            true,
        );
        assert!(args.ends_with(&["up", "-d", "--force-recreate"]));
    }

    #[test]
    fn service_up_args_shape() {
        // `up -d <svc>` (NOT `start <svc>` - no container exists for a
        // profile-gated service no `up` ever created), and the service's
        // OWN profile is carried so compose sees it at all.
        let args = service_up_args(
            &["compose", "-p", "sbx-dev1", "-f", "/x/compose.yml"],
            &CODE_SERVER_PROFILE,
            "code-server",
        );
        assert_eq!(
            args,
            vec![
                "compose",
                "-p",
                "sbx-dev1",
                "-f",
                "/x/compose.yml",
                "--profile",
                "code-server",
                "up",
                "-d",
                "code-server",
            ]
        );
    }

    #[test]
    fn service_rm_args_shape() {
        // Targeted pre-recreate removal: --force --stop tolerates a running
        // container; the service name scopes it to code-server only.
        let args = service_rm_args(
            &["compose", "-f", "/x/compose.yml"],
            &CODE_SERVER_PROFILE,
            "code-server",
        );
        assert_eq!(
            args,
            vec![
                "compose",
                "-f",
                "/x/compose.yml",
                "--profile",
                "code-server",
                "rm",
                "--force",
                "--stop",
                "code-server",
            ]
        );
    }

    #[test]
    fn non_up_commands_keep_full_profiles() {
        // 契约 4 second half: stop/restart/down still see code-server to
        // stop and tear it down cleanly. The constants themselves are the
        // contract - a single place to catch an accidental re-split.
        assert_eq!(
            SANDBOX_PROFILES,
            ["--profile", "code-server", "--profile", "vnc"]
        );
        // S1: vnc profile is now per-sandbox conditional via up_profiles().
        assert_eq!(up_profiles(true), ["--profile", "vnc"]);
        assert!(up_profiles(false).is_empty());
        assert_eq!(CODE_SERVER_PROFILE, ["--profile", "code-server"]);
    }
}
