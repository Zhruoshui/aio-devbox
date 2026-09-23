# Research: mgr backend — routes, docker lifecycle, jobs, proxy-route placement, deps, envhash/composegen

- **Query**: mgr/src route table, main.rs composition, docker.rs lifecycle + profile flags, jobs.rs pattern, where a generic sandbox-app proxy route (HTTP+WS to sbx-<name>-piweb:8088 over aio-mgr-net) would live, Cargo deps (hyper/reqwest/tower-http), envhash/composegen profile wiring.
- **Scope**: internal
- **Date**: 2026-09-09

## 1. Exact route table (mgr/src/routes.rs:27-57)

```rust
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/scenarios", get(scenarios))
        .route("/api/sandboxes", get(list_sandboxes).post(create_sandbox))
        .route("/api/sandboxes/adopt", post(adopt_sandbox))          // static segment wins over :name
        .route("/api/sandboxes/:name", get(get_sandbox).put(put_sandbox).delete(delete_sandbox))
        .route("/api/sandboxes/:name/start", post(start_sandbox))
        .route("/api/sandboxes/:name/stop", post(stop_sandbox))
        .route("/api/sandboxes/:name/restart", post(restart_sandbox))
        .route("/api/sandboxes/:name/entry_url", get(entry_url))
        .route("/api/images", get(list_images))
        .route("/api/jobs/:id", get(get_job))
        .merge(crate::models::router())   // /api/models/config GET+PUT, /api/models/sync GET,
                                          // /api/models/import/pi POST, /api/models/discover POST,
                                          // /api/models/test POST, /api/models/catalog GET (models.rs:133-141)
        .merge(crate::usage::router())    // /api/usage GET (usage.rs:71-73)
        .route("/api", any(api_not_found))
        .route("/api/", any(api_not_found))
        .route("/api/*rest", any(api_not_found))
}
```

- The `/api` seam trio (routes.rs:54-56) returns `{"error": "no such API route"}` 404 (routes.rs:60-65) so unmatched API paths never hit the SPA fallback. Comment cites matchit 0.7.3 three-route discipline (same as app main.rs).
- Error shape: `ApiError {status, message}` → `{"error": msg}` (routes.rs:72-93). `ApiError::bad` = 400; `From<anyhow::Error>` = 500.
- `sandbox_json` list-item fields (routes.rs:441-459): `name/status/live/adopted/created_at/cpus/mem_mb/env/image/entry_url/piweb_url/services[]`; `entry_url = http://sbx-<name>.mgr.localhost/`, `piweb_url = http://sbx-<name>-piweb.mgr.localhost/` (inline, mirror of caddy.rs render + composegen aliases — three-way identity).
- `live` semantics (routes.rs:401-436): `running|stopped|gone|unknown`; a ps FAILURE maps to `unknown` (not gone).
- PUT merge semantics (routes.rs:504-521): env replaced wholesale; cpus/mem absent=keep, explicit 0=clear, >0=set.
- Reserved names: `mgr`, `sbx-` prefix (routes.rs:192-194). Name slug validate: routes.rs:135-147.

## 2. main.rs composition (mgr/src/main.rs)

- Modules: `caddy, composegen, db, docker, envhash, jobs, models, routes, state, usage` (main.rs:24-33).
- Env: `MGR_BIND` (default 0.0.0.0:8089), `MGR_REPO` (default cwd), `MGR_DATA` (default `<repo>/mgr-data`), `MGR_WEB_DIR` (default `<repo>/mgr-web/dist`) — main.rs:47-61.
- Boot sequence: open db + `fail_orphan_jobs` (:73-77) → AppState::new (:79) → `docker::ensure_network("aio-mgr-net")` FATAL on failure (:83-85) → `caddy::regenerate(&state)` best-effort (:90-92).
- Static serving:
  ```rust
  let serve_dir = ServeDir::new(&web_dir).fallback(ServeFile::new(web_dir.join("index.html")));  // main.rs:99-100
  let app = routes::router().fallback_service(serve_dir).with_state(state.clone());              // main.rs:102-104
  ```
- Missing dist is a soft warn (API still served) — main.rs:66-71.

## 3. docker.rs lifecycle + the always-carry-profile code

**The load-bearing quote (docker.rs:107-115):**

```rust
/// Profile flags every mgr compose lifecycle command carries: the generated
/// sandbox compose gates code-server / vnc behind profiles (same shape as the
/// repo compose, design §3.5), but mgr sandboxes build ALL images and are
/// managed as a unit - the workbench without its panes is half a product, so
/// up starts them all (A3) and down must see them to tear them down. The
/// external-stack variants below carry them too: the repo stack has the same
/// profile-gated sidecars, and a start/stop that dropped them would maim an
/// adopted workbench.
const SANDBOX_PROFILES: [&str; 4] = ["--profile", "code-server", "--profile", "vnc"];
```

Project-based variants (all extend `SANDBOX_PROFILES`):
- `compose_up(project, compose_file, force_recreate)` — docker.rs:118-127: `compose -p <project> -f <file> --profile code-server --profile vnc up -d [--force-recreate]`.
- `compose_down(project, file, volumes)` — docker.rs:129-137 (+`-v` when volumes).
- `compose_restart` — docker.rs:139-144; `compose_stop` — docker.rs:146-151.

External-stack variants (NO `-p`; project derived from compose file's directory — section comment docker.rs:153-167; "the file is the identity"):
- `compose_ps_file` — docker.rs:175-180 (`ps --all --format json`).
- `compose_up_file` — docker.rs:184-189 (carries SANDBOX_PROFILES).
- `compose_stop_file` — docker.rs:191-196; `compose_restart_file` — docker.rs:198-203.
- `compose_ps` (project-based) — docker.rs:250-258; `parse_ps_output` accepts array / JSONL / single object (docker.rs:262-286); parse failure = error, never empty list.

**D4 implication (code-server lazy start)**: start_sandbox currently calls `docker::compose_up(...)` which ALWAYS carries both profiles (routes.rs:609-631). A lazy code-server start needs either a profile-free up plus a targeted `docker compose ... up -d code-server` / `start code-server` variant, or a new docker.rs function that omits/replaces SANDBOX_PROFILES — and correspondingly stop must still see the profile-gated services to tear them down (契约 4: down 必须看到它们才能拆干净; ps is the exception, `--all` lists profile-gated containers without flags). Contract anchor: `.trellis/spec/backend/sandbox-mgr-ops.md` 契约 4 (lines 99-115).

Other docker.rs pieces:
- `build(context, dockerfile, tag, build_args)` — docker.rs:21-36.
- `ensure_network(name)` — docker.rs:41-56 (idempotent, "already exists" = Ok).
- `network_connect_alias(network, container, alias)` — docker.rs:72-94: docker has no "add alias to existing membership"; on "already exists" it disconnects + reconnects. Empirical note: the "already exists" error fires for RUNNING containers only (docker 29.6.1) — a stopped container's second connect silently drops the alias (benign: aliases persist across stop/start, lost only on recreate).
- `network_disconnect` — docker.rs:101-105 (best-effort, warns inside).
- `caddy_reload_in_container` — docker.rs:208-214 (`docker exec <container> caddy reload --config <path>`).
- `image_exists(tag)` — docker.rs:289-310 ("No such image/object" = false).
- `run_capture` — docker.rs:313-330 (stdout on success, anyhow error with 2000-byte stderr tail on failure).

## 4. jobs.rs async job pattern

- Two spawns: `spawn_create(state, name, env, cpus, mem_mb, recreate)` (jobs.rs:31-80) and `spawn_delete(state, name, volumes)` (jobs.rs:191-260).
- Pattern: durable job row FIRST (`db::insert_job`, jobs.rs:41-44) → in-memory `JobShared` behind `tokio::sync::Mutex` in `state.jobs` map (:46-57) → `tokio::spawn` detached task (:59-77) → on completion set status ok/error, `db::update_sandbox_status`, `db::persist_job` (:62-77).
- `run_create` steps (jobs.rs:82-187): (1) env→manifest→assemble→hash (envhash.rs); (2) ensure_network; (3) build missing images, order base→app→code-server→vnc, app/cs use `--build-arg BASE_IMAGE`; per-sandbox `Dockerfile.base` written into the instance dir; (4) composegen generate + write; (5) `compose_up(force_recreate=recreate)`; (6) `update_sandbox_config` on success only (A5 correctness); (7) `caddy::regenerate` (failure reported in log, not fatal).
- Log tail cap `LOG_TAIL = 8KB` (jobs.rs:27); `append_log` truncates from the front (jobs.rs:262-269); `tail()` char-boundary safe (jobs.rs:273-284).
- Orphan handling: jobs left `running` at boot are marked error `interrupted (mgr restarted)` (db.rs:217-224).
- JobView polling: mgr-web `JobView.tsx` polls every 1.5s (`POLL_MS = 1500`).

## 5. Where a generic sandbox-app proxy route would live

### Existing precedent — the usage fan-out (usage.rs)

The only existing mgr→sandbox-app HTTP call path:
- URL built as `http://sbx-{name}-piweb:{APP_PORT}/api/models/usage?window=...` with `APP_PORT: u16 = 8088` (mgr/src/usage.rs:58-59, :157-158).
- Uses shared `state.http` (reqwest Client, state.rs:84-87) with a 5s per-sandbox timeout (usage.rs:55).
- Notes (usage.rs:17-22): the alias is `sbx-<name>-piweb` (the app service's aio-mgr-net alias from composegen), NOT `sbx-<name>` (that is the gateway's :8080 alias); mgr talks to the app DIRECTLY, not through the sandbox gateway.
- Adopted sandboxes are included (Phase 5 network-connects the same aliases — routes.rs:344-346).

So the task's target host `sbx-<name>-piweb:8088` matches the existing usage.rs convention exactly.

### Placement options grounded in the code

- **routes.rs router()**: new sub-router would be merged alongside `crate::models::router()` / `crate::usage::router()` (routes.rs:46-47). A new module (e.g. `mgr/src/proxy.rs`) with `pub fn router() -> Router<Arc<AppState>>` following the same pattern is the established convention; routes are merged BEFORE the `/api` seam trio (comment routes.rs:44-46: "merge keeps them ahead of the /api seam below (static segments win either way)").
- Route shape needs care with matchit 0.7.3: the seam trio is `/api`, `/api/`, `/api/*rest` — a new route like `/api/sbx/:name/*path` is a static-prefix segment that wins over the catch-all (same mechanism as `/api/manifest` in app main.rs:26-31 comments).
- **WebSocket passthrough precedent — app/src/routes/preview.rs** is the in-repo template for HTTP+WS proxying on axum 0.7:
  - HTTP: forward method/headers (strip hop-by-hop + Host; preview.rs:59-65, :130-138), stream response unbuffered via `reqwest` `bytes_stream()` → `Body::from_stream` (preview.rs:157-160).
  - WS: detect via `Option<WebSocketUpgrade>` + Upgrade header (preview.rs:80-83, :104-113), connect upstream with **tokio-tungstenite** `connect_async` (plaintext; 2s handshake timeout preview.rs:72), forward the negotiated `Sec-WebSocket-Protocol` back to the browser (preview.rs:207-218), then pump messages both ways splitting sinks/streams (preview.rs:220-271).
  - NOTE: app's axum is built with `features = ["ws"]` (app/Cargo.toml:7 `axum = { version = "0.7", features = ["ws"] }`).

### Crate dependencies available in mgr (mgr/Cargo.toml)

| Dep | mgr/Cargo.toml line | Features | Notes for a proxy |
|---|---|---|---|
| axum | :34 | **default only — NO `ws` feature** | must add `features = ["ws"]` for `WebSocketUpgrade` |
| reqwest | :30 | `json`, `rustls-tls` (default-features off, no `stream`) | usage.rs + models.rs probes; for streaming proxy bodies the `stream` feature would be needed (app has it: app/Cargo.toml reqwest line features `["json","rustls-tls","stream"]`) |
| tokio | :35 | `full` | |
| tower-http | :43 | `fs` only (0.5, shared lock entry with app) | ServeDir; no proxy features (there is no tower-http proxy util anyway) |
| futures-util | :33 | 0.3 | join_all for fan-out |
| hyper | NOT a direct dep | 1.11.0 in workspace lock (transitive via axum/reqwest) | available transitively; direct dep would need adding |
| tokio-tungstenite | NOT a dep of mgr | 0.23.1 in lock (app uses it for /preview WS) | must be added for the WS leg |
| aio-config / aio-models | :22, :25 | path crates | |

Cargo.lock aio-mgr dependency list confirms: aio-config, aio-models, anyhow, axum, futures-util, reqwest, rusqlite, serde, serde_json, sha2, tokio, tower-http 0.5.2, tracing, tracing-subscriber — nothing else.

## 6. envhash.rs (profile-adjacent wiring)

- `SandboxEnv { scenarios: Vec<String>, versions: BTreeMap<String,String> }` — envhash.rs:22-31. Scenarios must NOT be always_on (validation :38-79; always_on implies gen).
- `env_hash = sha256(assembled Dockerfile.base bytes)` — envhash.rs:91-96 (hash pins the same surface as gen's inputs).
- Image tags: `TAG_PREFIX = "sandbox-base-"`, `VNC_TAG = "sandbox-vnc"` (env-independent), `image_tags(hash) -> (sandbox-base-<h[:12]>, sandbox-app-<h[:12]>, sandbox-code-server-<h[:12]>)` — envhash.rs:101-112.
- `project_name(name) = "sbx-<name>"` — envhash.rs:115-117.

## 7. composegen.rs — code-server/vnc service definitions + env injection

Generated per-sandbox compose (`instances/sbx-<name>/compose.yml`, written by composegen::write :168-177):
- **app service** (composegen.rs:78-99): image `sandbox-app-<short>`; env injected:
  - `PI_WEB_URL: http://sbx-{name}-piweb.mgr.localhost/` (:85)
  - `PI_WEB_ALLOWED_HOSTS: app,sbx-{name}-piweb.mgr.localhost` (:86)
  - `MGR_URL: http://mgr-api:8089` (:91) — model-config pull (Phase 4)
  - aio-mgr-net alias `sbx-<name>-piweb` (:98); workspace volume `/root` (:93); optional deploy.resources.limits (:47-59).
- **gateway service** (:65-76): caddy:2, alias `sbx-<name>`, per-sandbox Caddyfile at `./gateway/Caddyfile` (relative bind — load-bearing for 契约 1 path identity).
- **code-server service** (composegen.rs:103-112): `profiles: [code-server]`, `network_mode: "service:app"`, `VSCODE_PROXY_URI: "/proxy/{{port}}/"`, workspace volume. Sidecar netns warning at :100-102 (operate via compose only, never `docker restart`).
- **vnc service** (:114-124): `profiles: [vnc]`, `network_mode: "service:app"`, shm_size 2gb, tmpfs /tmp (stale-lock fix).
- Per-sandbox Caddyfile (render_caddyfile :140-162): `:8080` with `handle_path /code-server/* → app:8200`, `/vnc/* → app:6080` (with no-store header), catch-all `→ app:8088`. No basicauth (D9, test-anchored :196-197).
- compose carries NO project name — passed via `-p` (module comment :22-23).

**D4/D6 implication**: the app service listens on `8088` (axum), `8200` (code-server sidecar is in app's netns → reachable as app:8200), `6080` (vnc), `30141` (pi-web autostart via entrypoint). All reachable from aio-mgr-net through the app container's alias `sbx-<name>-piweb` because network_mode sidecars share app's netns.

## 8. caddy.rs (total gateway) — routing facts relevant to a proxy decision

- render() always emits the mgr site block FIRST: `http://mgr.localhost → mgr-api:8089` (caddy.rs:44-54).
- Per sandbox: `http://sbx-<name>.mgr.localhost → sbx-<name>:8080` (the sandbox's OWN gateway) and `http://sbx-<name>-piweb.mgr.localhost → http://sbx-<name>-piweb:30141` with **`header_up Host sbx-<name>-piweb.mgr.localhost`** (契约 3 — pi-web request-security requires the .localhost-suffix public hostname, not the bare alias).
- regenerate() backs up via fs::copy and writes IN PLACE (契约 2 — bind-mount inode trap, caddy.rs:99-113); reload failures reported to kv `gateway_reload`, not fatal.
- Reload channel: `MGR_GATEWAY_CONTAINER` env → docker exec; else host caddy binary (caddy.rs:143-174).

**Alternative to an mgr-api proxy**: a new subdomain/site block in caddy.rs render could route `sbx-<name>.mgr.localhost/*` paths to the app directly (like the piweb block does to :30141). Note the existing sbx-<name> block goes to the sandbox's own gateway :8080 which already proxies code-server/vnc and catch-alls to app:8088 — so iframe subdomain URLs already work today for code-server/vnc/piWeb via `http://sbx-<name>.mgr.localhost/code-server/` etc. The distinguishing need for an mgr-api proxy (D6 terminal/agent WS) is that mgr-web's own origin (mgr.localhost) must serve the WS + API proxied per sandbox, so panes stay same-origin with mgr-web's api client (and the app's buttons.toml CRUD per D7 needs a per-sandbox proxied /api path).

## 9. mgr stack (mgr/compose.yml) — network topology

- `mgr-gateway`: caddy:2, container_name `aio-mgr-gateway-1`, `ports: 80:80` (the ONLY host port — 契约 9), Caddyfile bind-mounted ro from `mgr-data/caddy/Caddyfile`, on `mgr-net` + `aio-mgr-net`.
- `mgr-api`: build context repo root; env MGR_REPO/MGR_DATA/MGR_BIND/MGR_GATEWAY_CONTAINER; mounts docker.sock, repo ro, mgr-data rw — **at their own host absolute paths** (契约 1 PATH IDENTITY, compose.yml:5-19); networks mgr-net + aio-mgr-net **with alias `mgr-api`** (compose.yml:76-78) — this is the alias `MGR_URL` points at.
- PATH IDENTITY is load-bearing for any new bind-mount-based feature.

## 10. Caveats / Not Found

- mgr has NO existing WebSocket route of any kind (no `ws` feature, no WS handler) — the preview.rs port would be the first.
- mgr has no auth layer anywhere (D9, 契约 9) — a per-sandbox proxy inherits this (anyone with mgr.localhost access can reach every sandbox's app:8088, including the pty WS at /api/term/ws — the terminal pane's full-shell surface. The /api/models/sync endpoint already hands out plaintext keys on the same boundary, models.rs:197-204).
- No hyper direct dependency; tower-http is 0.5.2 fs-only. axum 0.7.9 in lock.
- `mgr/Dockerfile:35-47` dep-cache layer dummy-sources EVERY workspace member (契约 5) — adding a new mgr dep (tokio-tungstenite, axum ws feature) only changes Cargo.toml/lock, no Dockerfile change needed; but the lockfile `--locked` build means Cargo.lock must be regenerated.
