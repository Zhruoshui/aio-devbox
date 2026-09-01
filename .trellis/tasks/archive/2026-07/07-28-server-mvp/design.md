# Design: AIO-style Dev Sandbox MVP

> Technical design for `07-28-server-mvp`. Pairs with `prd.md` (requirements) and
> `implement.md` (execution plan). All architecture decisions are resolved - see
> `prd.md` "Decisions & Open Questions".

## 1. Overview

Multi-container Docker Compose stack. A single caddy gateway publishes `:8080`
(basicauth) and reverse-proxies to an axum app server and to pluggable service
containers (code-server, VNC). The axum app serves the React workspace UI, a
terminal pty WebSocket, an opencode launcher (reuses the pty), a service-manifest
endpoint, and reserved `/api` `/v1` `/mcp` seams. All workspace-touching containers
share one named volume at `/home/gem` (uid 1000) for unified persistence.
Pluggability is via Compose profiles (code-server, vnc) + env flags (terminal,
opencode); the manifest reflects what is actually running and the UI shows only
those panes. **Services are data-driven** (`services.toml` + generic panes) so
adding a component is configuration, not new UI code - see §14.

## 2. Container topology

```
                    host :8080
                       |
                caddy gateway   (basicauth + reverse proxy)
                   /     |      \
   /code-server/*  |     |   everything else (/, /api/*, /v1/*, /mcp, /api/term/ws)
            |      |     |             |
      code-server  |     |          app (axum + dev env)
         :8200     |     |          :8088
                   |     |            |- serves React UI (static)
            /vnc/* |     |            |- GET /api/manifest
               |   |     |            |- GET /api/term/ws?cmd=<opt>   (pty WS)
             vnc   |     |            |- /api /v1 /mcp  (reserved seams)
           :6080   |
   Xvnc:5900 + websockify + openbox + Chromium

  shared named volume `workspace` -> /home/gem (uid 1000) on app, code-server, vnc
```

| Service | Image | Port | Profile | Role |
|---|---|---|---|---|
| gateway | caddy | 8080 (published) | always | basicauth + reverse proxy |
| app | sandbox-base + axum | 8088 | always | UI, terminal pty, opencode, manifest, seams |
| code-server | sandbox-base + code-server | 8200 | `code-server` | VSCode in browser |
| vnc | sandbox-base + browser stack | 6080 | `vnc` | Chromium over VNC |

`sandbox-base` = Debian/Ubuntu + node + python + git + common dev tools + opencode
+ user `gem` (uid 1000) + WORKDIR `/home/gem`. Built once; `app`, `code-server`,
`vnc` derive from it so the dev-env layer is not duplicated.

**terminal & opencode are not separate containers** - they are axum routes that
spawn pty shells locally in the app container (which has the dev env + opencode).
Gated by env flags `ENABLE_TERMINAL` / `ENABLE_OPENCODE` (default true).

## 3. Routing (Caddyfile)

```caddyfile
:8080 {
  basicauth { {$SANDBOX_USER} {$SANDBOX_PASSWORD_HASH} }
  @codeserver path /code-server/*
  reverse_proxy @codeserver code-server:8200
  @vnc path /vnc/*
  reverse_proxy @vnc vnc:6080
  reverse_proxy app:8088
}
```
WebSocket upgrade is handled automatically by caddy for `/code-server/`, `/vnc/`,
and the terminal `/api/term/ws`. basicauth applies to WS upgrades too. If a
pluggable service's container is absent, its route 502s - acceptable because the
manifest hides that pane. **Adding a new web service = one more `@name path` +
`reverse_proxy` block** (see §14).

## 4. axum app

Routes (all behind caddy basicauth):
- `GET /` and `/assets/*` - serve the React SPA build (static files).
- `GET /api/manifest` - JSON list of enabled services, **data-driven from
  `services.toml`** (baked into the image). Each entry:
  `{ id, type, enabled, url|cmd }`. `type=web` (containerized web service, iframe
  pane): `enabled` = TCP reachability to its `target` (reflects compose profiles -
  single source of truth, no env to keep in sync). `type=agent` (pty-launched CLI,
  xterm pane): `enabled` = env flag (default true). The frontend renders panes
  generically by `type`, so adding a service is config-only on the UI side (§14).
- `GET /api/term/ws` (WebSocket) - pty bridge. Optional `?cmd=` launches a
  non-interactive command (e.g. `opencode`) instead of the default shell. Uses
  `portable-pty`; shell runs as uid 1000 in `/home/gem`.
- `/api`, `/v1`, `/mcp` (catch-all) - reserved seam: `502` with
  `{ "error": "seam reserved" }`. Proves the seam is not swallowed by code-server.

`services.toml` shape:
```toml
[[service]]
id = "codeServer"
type = "web"                 # iframe pane; enabled = TCP reachability
target = "code-server:8200"
url = "/code-server/"
[[service]]
id = "vnc"
type = "web"
target = "vnc:6080"
url = "/vnc/vnc.html?autoconnect=1&resize=scale"
[[service]]
id = "terminal"
type = "agent"               # xterm pane; enabled = env flag
enable = "ENABLE_TERMINAL"   # default true
cmd = ""                     # empty = default shell
[[service]]
id = "opencode"
type = "agent"
enable = "ENABLE_OPENCODE"
cmd = "opencode"
```

Structure:
```
app/
  Cargo.toml
  src/
    main.rs        - build router, serve on 0.0.0.0:8088
    config.rs      - parse services.toml + env flags + manifest builder
    state.rs       - AppState
    routes/{manifest,terminal,seam}.rs
    pty.rs         - portable-pty <-> WebSocket bridge
  services.toml    - service registry (single place to add a service)
  static/          - React build output (copied at image build)
```
Key crates: `axum`, `tokio`, `hyper`, `tower`, `portable-pty`, `serde`,
`serde_json`, `toml`, `tracing`, `tracing-subscriber`. The app process runs as
uid 1000 (gem); pty shells inherit that.

## 5. Frontend (React + golden-layout)

```
web/
  package.json (react, react-dom, golden-layout, @xterm/xterm, @xterm/addon-fit, vite)
  src/
    main.tsx
    App.tsx          - fetch /api/manifest; map services to generic panes; build tree
    panes/{IframePane,XtermPane}.tsx   # generic, keyed by service.type
    layout.ts        - golden-layout config + dynamic tree from manifest
  vite.config.ts     - build output -> ../app/static
```
- Generic panes keyed by `type` (no per-service component needed):
  - `IframePane` (type=web): `<iframe src={service.url}>`.
  - `XtermPane` (type=agent): xterm.js + fit addon; opens WS
    `/api/term/ws?cmd={service.cmd}` (empty cmd = default shell).
- `App.tsx` fetches `/api/manifest`, maps each enabled service to its generic pane,
  and builds the golden-layout tree. **Adding a service = add a `services.toml`
  entry (+ container/profile/caddy route for `type=web`); no new React component.**
- golden-layout gives tiling: split rows/columns, tab groups, drag-to-rearrange,
  resize handles.
- iframe drag-capture handled with the standard transparent-overlay-on-drag trick
  (a div over iframes while dragging so the iframe doesn't swallow mouse events).
- Layout persistence to localStorage is a nice-to-have, not required.

Build: `npm run build` in `web/` -> `app/static/`; the app Dockerfile copies it.
Dev: vite dev server with a proxy for `/api`, `/code-server`, `/vnc` to the stack.

## 6. code-server container

From `sandbox-base` + install code-server. Run:
`code-server --host 0.0.0.0 --port 8200 --auth none --user-data-dir /home/gem/.local/share/code-server`
(`--auth none` because caddy does auth). `VSCODE_PROXY_URI=/proxy/{port}/` kept for
code-server's own port preview (only reaches ports started in code-server's
integrated terminal - see Out of Scope). Mounts the workspace volume at `/home/gem`.

## 7. VNC container

From `sandbox-base` + install: TigerVNC (`Xvnc`), `websockify` (+ noVNC web files),
`openbox`, Chromium. Under `s6-overlay` (one service per process: `Xvnc`, `openbox`,
`chromium`, `websockify`).
- `Xvnc :99` (display), listening `127.0.0.1:5900`, no auth (internal).
- `openbox` as WM on `:99`.
- Chromium launched on `:99` (anti-automation flags; optional CDP 9222).
- `websockify` on `:6080` serving noVNC web files + proxying WS -> `127.0.0.1:5900`.

caddy `/vnc/*` -> `vnc:6080`. The VncPane iframe opens `/vnc/vnc.html`. Mounts the
workspace volume (Chromium profile persists at `/home/gem/.config/chromium`).

Deviation from reference: we use `websockify` (serves noVNC files + WS proxy in
one) instead of `websocat` + nginx-served files, because we have no nginx serving
static and websockify is the canonical noVNC server.

## 8. Persistence & uid alignment (R6 / R9)

- Named volume `workspace` mounted at `/home/gem` on `app`, `code-server`, `vnc`.
- All three run as uid 1000 (`gem`); `sandbox-base` creates the user.
- `gateway` (caddy) runs as its own user; does not touch the volume.
- code-server data, Chromium profile, terminal/opencode state, npm/pip packages in
  `/home/gem` all persist across `compose down/up`.

## 9. Pluggability (R8)

- `code-server`, `vnc`: Compose profiles. `docker compose --profile code-server
  --profile vnc up`. Omit a profile -> its container doesn't start.
- `terminal`, `opencode`: env flags on `app` (`ENABLE_TERMINAL`, `ENABLE_OPENCODE`,
  default true).
- `services.toml` is the single registry of known services; the manifest reflects
  actual availability (TCP reachability for `web`; env for `agent`), so the UI
  shows only present panes. Single source of truth: what is running.

## 10. Auth (R10)

caddy `basicauth` over everything (HTTP + WS). Single user/pass from env
(`SANDBOX_USER`, `SANDBOX_PASSWORD_HASH`). A `forward_auth` / API-key seam is
reserved (commented Caddyfile slot + the `/api` seam) for a future migration; not
built in MVP.

## 11. Out of scope (MVP) / deferred

- **Cross-container port preview** for dev servers started in the terminal pane.
  code-server's built-in `/proxy/{port}/` only reaches ports in the code-server
  container. A unified preview proxy (axum proxying `/proxy/{port}/` to the app
  container's localhost) is deferred - not in the stated requirements.
- codex launcher (only opencode) - but adding it later is config-only (§14A).
- python-server / `/v1` SDK API / MCP hub / gost / Jupyter / node REPL - but a
  service like Jupyter drops in via §14B.
- TLS (plain HTTP behind Tailscale/LAN).
- Multi-user / multi-workspace.

## 12. Key trade-offs

- Multi-container (vs single image): cleaner separation + independent restart, but
  shared-volume + uid alignment needed and "build-time selection" became up-time
  selection (compose profiles). Accepted (user choice).
- caddy fronting axum (vs axum-as-gateway): caddy trivially handles code-server
  subpath + noVNC WS; axum stays focused on app logic. Cost: two components.
- terminal/opencode in the app container (not their own): reuses one pty mechanism,
  no extra containers; cost: app image carries the dev env (shared via sandbox-base
  so not duplicated).
- websockify (vs websocat): simpler noVNC serving; cost: python in the vnc container
  (already present via sandbox-base).
- Data-driven manifest + generic panes (vs per-service components): maximal
  extensibility, adding a service is config-only on the UI; cost: a little more
  upfront abstraction (services.toml schema + generic pane components).

## 13. Compatibility / migration notes

- Reserved `/api` `/v1` `/mcp` seams let a future FastAPI+FastMCP (or axum-native)
  agent API slot in without re-architecting the gateway (§14C).
- The manifest contract (`GET /api/manifest` -> service list) is the UI<->backend
  contract; adding a service only needs a `services.toml` entry (+ pane config).
- `sandbox-base` as the shared dev-env layer means tool upgrades land in one image.

## 14. Extension Guide

The architecture is built to add components without re-architecting. Three
extension dimensions:

### A. Add a CLI agent (e.g. codex, or any terminal tool)

Agents are pty-launched CLIs. Reuses the existing terminal mechanism - **no new
container, no new route**.
1. Install the binary in `sandbox-base` (or `app` image).
2. Add a `services.toml` entry: `id=codex, type=agent, cmd=codex,
   enable=ENABLE_CODEX`.
3. Set `ENABLE_CODEX=true` in compose to show the pane.
The generic `XtermPane` opens `/api/term/ws?cmd=codex`. Done - no React change.

(Runtime-installed agents work too: `npm i -g <tool>` in the terminal persists in
the volume; you only add a `services.toml` entry to make it a first-class pane.)

### B. Add a containerized web service (e.g. Jupyter, like the source project)

Same pattern as code-server/vnc - a container behind a caddy path prefix, embedded
as an iframe.
1. `jupyter/Dockerfile`: `FROM sandbox-base`, install JupyterLab, serve on `:8888`
   under prefix `/jupyter/`.
2. `docker-compose.yml`: add service under profile `jupyter`, mount workspace,
   uid 1000.
3. `gateway/Caddyfile`: `@jupyter path /jupyter/*` -> `reverse_proxy jupyter:8888`.
4. `services.toml`: `id=jupyter, type=web, target=jupyter:8888, url=/jupyter/`.
The manifest auto-detects it via TCP reachability; the generic `IframePane` embeds
it. No React change.

### C. Add the agent/SDK API surface (the AIO python-server equivalent)

Reserved seams `/api` `/v1` `/mcp` already route to axum (or a future container).
Implement the API in axum (or drop in a FastAPI+FastMCP container) on those paths -
no gateway change. Post-MVP.

### Cost summary

| Extension | New container | New caddy route | New React component | Image rebuild |
|---|:---:|:---:|:---:|:---:|
| CLI agent (codex) | no | no | no | yes (install binary) |
| Web service (jupyter) | yes | yes | no | yes (new image) |
| Agent/SDK API | optional | no (seam exists) | no | yes |

The manifest + generic-pane design means the UI never needs a new component for a
new service - only `services.toml` (+ container/route for web services).
