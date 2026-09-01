# PRD: AIO-style Dev Sandbox MVP - axum gateway + 4 pluggable services + workspace UI

> Parent task. Owns the source requirement set, child-task map, and cross-child
> acceptance criteria. Implementation happens in child tasks.
>
> **Status: planning - requirements revised 2026-07-28.** This supersedes the
> earlier narrow MVP (caddy + code-server + volume + basic auth). Prior Decisions
> D1 (multi-container), D3 (caddy), D5 (basicauth) confirmed retained 2026-07-28;
> the python-FastAPI seam intent is dropped in favour of a Rust axum app server.
> See "Prior Decisions" and "Decisions & Open Questions".

## Goal & User Value

A self-hosted remote development environment: `docker compose up` brings up an
axum gateway plus the selected service containers, and end users open a web UI
presenting a freely-arrangeable workspace of panes - code-server (cloud IDE), a
Chromium browser over VNC, a cloud terminal, and an opencode agent launcher. All
four share one persistent filesystem. Services are pluggable: you select which of
the four are included. The server is written in Rust (axum); the frontend is the
implementer's choice.

Personal use. Minimal but coherent first implementation.

## Background

- Inspired by `agent-infra/sandbox` (AIO Sandbox). Its server is **closed**
  (prebuilt image only - no source, no Dockerfile). Reverse-engineering research
  archived at
  `.trellis/tasks/archive/2026-07/07-28-research-aio-architecture/research/`
  (9 files; `02` architecture, `08` reverse-engineered server).
- Reference topology (research 02/08): **single container**, ~12 services on
  internal loopback ports sharing one filesystem (`/home/gem`); nginx gateway on
  public `:8080` reverse-proxies to code-server `:8200`, noVNC ws `:6080`,
  python-server `:8091`, etc.; supervisord manages processes; some configs
  rendered at startup via envsubst. **We diverge from single-container to
  multi-container** (user choice), so the reference's supervisord-in-one-container
  model becomes one-process-per-container, except the VNC container which still
  needs several cooperating processes.
- The user has deployed the closed-source AIO image at host `:8080` as a live
  reference. **Note:** from this sandbox `host.docker.internal:8080` is blocked
  by the default-deny network policy (`Blocked by network policy: domain
  localhost:8080`). To let me inspect the reference UI, allow it on the host:
  `sbx policy allow network localhost:8080`. Not blocking; proceeding on
  research + user description for now.
- Earlier MVP PRD scoped only caddy + code-server + volume + basic auth with
  `/api` `/v1` `/mcp` seams reserved for a future python FastAPI server. The user
  has since expanded scope (VNC, terminal, opencode, workspace UI, pluggability,
  Rust axum backend). Those earlier decisions are under revision.

## Confirmed Facts (from research)

- AIO = single container; supervisord runs ~17 programs as user `gem` (uid 1000).
- code-server on `:8200` behind `/code-server/`; has a built-in port-preview
  proxy (`VSCODE_PROXY_URI={{port}}-{{host}}`) exposing `/proxy/{port}/`,
  `/absproxy/{port}/`, and `${port}-${host}` wildcard preview.
- VNC stack: TigerVNC `Xvnc :99` (port 5900, no-auth internal) + `websocat`
  bridging the noVNC websocket (`:6080`) to raw VNC TCP (:5900); openbox window
  manager; Chrome (anti-automation flags) with CDP on `:9222` runs on the X
  display.
- Web terminal: separate static UI at `/terminal/` + WebSocket shell API at
  `/v1/shell/ws` (pty bridge) - distinct from code-server's built-in terminal.
- Gateway auth: nginx dual-server `auth_request` (public `:8080` + internal
  `:8081` -> python-server `/auth`); API key (`X-AIO-API-Key` /
  `Authorization: Bearer` / `?api_key=`) + JWT tickets.
- Out-of-MVP-for-us reference components: python-server (FastAPI+FastMCP, the
  SDK/agent API surface), MCP hub, gost HTTP proxy, JupyterLab, node REPL
  servers. Stay out of scope unless explicitly added.

## Requirements (MVP, revised)

- **R1.** A `docker-compose.yml` that, on `docker compose up`, starts an axum
  gateway container plus the selected service containers; the gateway is the only
  published port. (Run form = multi-container, per D1.)
- **R2.** code-server (VSCode in browser) reachable behind the gateway.
- **R3.** Chromium browser accessible over VNC (noVNC in the browser), reachable
  behind the gateway.
- **R4.** A cloud terminal (browser terminal over a pty WebSocket), reachable
  behind the gateway - as its own workspace pane, not only code-server's
  built-in terminal.
- **R5.** An opencode agent launcher: a workspace pane that opens opencode (CLI
  coding agent) in a terminal. (codex is explicitly deferred.)
- **R6.** Unified persistent storage: code-server, terminal, opencode, and the
  browser profile all read/write one shared filesystem (a shared named volume or
  bind mount) that survives container restart - not per-service stores. Fixed
  uid 1000 aligned across all containers that touch it.
- **R7.** A web workspace UI that hosts the four functions as independently
  arrangeable panes (free layout / draggable), and dynamically shows only the
  panes whose services were selected.
- **R8.** Pluggability via **compose profiles**: select services at
  `docker compose --profile <svc> ... up`; all service images are always built,
  unselected services are simply not started, and the UI reads a live service
  manifest from the gateway so absent services show no pane. (Literal
  "build-time" selection relaxed to "up-time" - accepted trade-off.)
- **R9.** Non-root workspace user with a fixed uid, aligned across all processes
  that touch the shared filesystem (reference: uid 1000).
- **R10.** Auth on the public entry point (MVP: basic auth or equivalent); a
  forward-auth / API-key seam is reserved for later.

## Prior Decisions

Recorded by the earlier narrow-MVP PRD; current status:
- **D1** multi-container compose - **RETAINED** (user confirmed 2026-07-28).
- **D2** reserve `/v1` `/api` `/mcp` - retained.
- **D3** caddy reverse proxy - **RETAINED** (2026-07-28): caddy fronts :8080
  with basicauth, reverse-proxies `/code-server/` and `/vnc/` to their
  containers, and forwards `/` + `/api` + terminal WS to axum.
- **D4** plain HTTP behind Tailscale/LAN - retained.
- **D5** caddy basicauth - **RETAINED** for MVP (2026-07-28); API-key/JWT
  deferred.
- **D6** workspace user `gem` uid 1000 - retained (R9).
- **D7** local build only - retained.

## Out of Scope (MVP)

- python-server / FastAPI / FastMCP / `/v1` SDK API / MCP hub - seams only.
- codex agent launcher (only opencode for now).
- gost outbound HTTP-proxy control plane.
- JupyterLab / node REPL servers.
- Multi-user / multi-workspace session management.
- TLS termination (plain HTTP behind Tailscale/LAN for personal use).

## Decisions & Open Questions

### Resolved (2026-07-28)

- **Run form** = multi-container compose (D1 retained). Gateway container is the
  only published port.
- **Pluggability** = compose profiles (selection at `compose --profile ... up`;
  all images always built; UI reads the live service manifest from the gateway).
- **axum role** = app server behind a thin caddy gateway (D3 retained). caddy
  does basicauth + reverse-proxies `/code-server/` and `/vnc/` to their
  containers and forwards `/`, `/api`, and the terminal WS to axum. axum serves
  the workspace UI, the terminal pty WebSocket, the service-manifest endpoint,
  and the reserved `/api` `/v1` `/mcp` seams.
- **Terminal** = dedicated pty WebSocket in axum (xterm.js frontend,
  `portable-pty` in Rust; shells spawned as uid 1000 in the shared volume).
  Distinct from code-server's built-in terminal (satisfies R4).
- **opencode pane** = a terminal pane that auto-launches `opencode`, reusing the
  same pty-WS mechanism (no deeper integration for MVP).
- **Auth** = caddy `basicauth` for MVP (D5 retained); API-key/JWT deferred.
- **VNC container** = one container bundling Xvnc + websockify + openbox +
  Chrome under a tiny supervisor (s6-overlay preferred; supervisord fallback).
  Other service containers run one process each.
- **Workspace layout** = IDE-style tiling, React + golden-layout (2026-07-28):
  panes split the viewport, drag to rearrange + resize; iframes embed code-server
  & noVNC, xterm.js embeds the terminal/opencode panes.

### Open (blocking planning)

None - all architecture decisions resolved. Ready for `design.md` / `implement.md`
review and `task.py start`.

## Acceptance Criteria (high-level; refine after architecture settles)

- **AC1.** `docker compose up` (with all four enabled); browser hits the public
  port, auth prompt, then the workspace UI loads with four arrangeable panes.
- **AC2.** Each pane is functional: code-server edits, VNC drives Chromium,
  terminal runs commands, opencode launches.
- **AC3.** Edits/files/installed packages persist across container restart in
  the single shared filesystem.
- **AC4.** Run with a subset (e.g. VNC disabled) - the VNC container is absent,
  the UI shows no VNC pane, and the other three still work.
- **AC5.** `/api`, `/mcp`, `/v1` return a defined non-code-server response
  (reserved seam).

## Child Task Map (to be finalized after architecture)

Tentative: (A) compose base & lifecycle + shared volume/uid, (B) axum gateway +
workspace UI + service manifest, (C) code-server, (D) VNC/Chromium container,
(E) terminal, (F) opencode, (G) pluggability mechanism. Ordering and
parent/child structure to be set once Q1-Q2 resolve.
