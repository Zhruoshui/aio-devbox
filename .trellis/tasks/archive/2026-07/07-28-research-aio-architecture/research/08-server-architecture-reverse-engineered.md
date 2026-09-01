# Research: Reverse-engineered server architecture

- **Query**: Can the closed AIO Sandbox server architecture be reverse-engineered?
- **Scope**: internal (Docker image inspection) + prior repo research
- **Date**: 2026-07-28
- **Method**: Pulled the publicly distributed image `ghcr.io/agent-infra/sandbox:latest`
  (13.1 GB, amd64), `docker create` (not started), `docker cp /opt/gem` + key configs.
  **Architecture only, by inspecting entrypoint / process-manager / nginx configs.
  No server source code extracted or copied** - the Python `app` package stays a black
  box; its contract is `website/docs/public/v1/openapi.json` (see `05-openapi-contract.md`).

## Image basics

- Size 13.1 GB, multi-arch (amd64/arm64). Mirrored from Volcengine
  `vefaas-public/all-in-one-sandbox` (see `01-open-closed-map.md`).
- Entrypoint: `/opt/gem/run.sh`. User: `gem` (uid 1000). Exposed: `8080/tcp` only.
- Runtime: Python 3.12 (`/opt/python3.12`), Node 20/22/24 (`/opt/nodejs/{20,22,24}`),
  Go (`/usr/local/go`), uv, fnm. Locale en_US.UTF-8, TZ Asia/Singapore.
- Browser: `/usr/local/bin/browser` (Chrome wrapper) with CDP on 9222, Puppeteer-configured.

## Startup flow (three stages)

```
run.sh  ──►  gem_init.sh   (trusted init: browser-supervisor setup, stale-state cleanup)
        ──►  gem.sh        (runtime: env defaults, XDG/dbus/fcitx5, lifecycle hooks,
                             envsubst-render nginx/mcp configs, then exec supervisord)
        ──►  supervisord   (root, nodaemon, includes /opt/gem/supervisord/*.conf)
```

Lifecycle hooks (env-provided shell): `RUN_HOOK_INIT`, `RUN_HOOK_PRE_SERVICES`,
`RUN_HOOK_POST_READY`, `SANDBOX_SHUTDOWN_HOOKS`. `WAIT_PORTS=8091` - entrypoint blocks
until the Python server is up. Several configs are **rendered at startup via `envsubst`**
(`nginx.python_srv.conf`, `mcp-hub.json`, `python_srv_wrapper.sh` content), so they are
empty in the static image and only exist at runtime.

## Process model (supervisord, 17 programs)

`supervisord.conf` runs as root and `include`s `/opt/gem/supervisord/*.conf`. Each
program runs as user `gem` unless noted. Priorities set boot order.

| Program | Command (essence) | Port | Role |
|---|---|---|---|
| **python-server** | `/usr/local/bin/python-server --host 127.0.0.1 --port 8091 --mcp-config /opt/gem/mcp-hub.json --filter-mcp-servers sandbox --workspace $WORKSPACE` (via `python_srv_wrapper.sh`, OTEL PYTHONPATH injected) | 8091 | **The server**: FastAPI REST API (140 ops) + integrated MCP hub (FastMCP). The whole product. |
| nginx | `/opt/gem/nginx` (rendered conf) | 8080 (public), 8081 (auth) | Gateway + auth subrequest server |
| mcp-server-browser | `/usr/local/bin/mcp-server-browser --port 8100 --cdp-endpoint http://127.0.0.1:9222/json/version` | 8100 | Standalone browser MCP server (CDP-backed) |
| gost | `/usr/local/bin/gost -C /opt/gem/gost.yaml` | 8118 (fwd proxy), 18080 (API) | **HTTP proxy = gost, NOT tinyproxy** (env var name is legacy) |
| nodejs-repl-22 | `/usr/local/bin/node22 /opt/repl-servers/nodejs/server.js` | 8092 | Node.js code-exec REPL (one per version: 20=8192, 22=8092, 24=8392) |
| nodejs-repl-20 / -24 | same, `/opt/nodejs/{20,24}/...` | 8192 / 8392 | Multi-version Node execution |
| code-server | `/opt/gem/code-server.sh` (`VSCODE_PROXY_URI={{port}}-{{host}}`) | 8200 | VSCode in browser + built-in port-preview proxy |
| jupyter | `/opt/gem/jupyter-lab.sh` (Python 3.12, prefix `/jupyter/`) | 8888 | JupyterLab kernels |
| tigervnc | `Xvnc :99.0 -geometry 1280x1024 -SecurityTypes None` | 5900 | **TigerVNC**, not x11vnc; no-auth (internal) |
| websocat | `websocat ws-l:127.0.0.1:6080 tcp:localhost:5900` | 6080 | noVNC ws->tcp bridge (not websockify) |
| agent-browser | `/opt/gem/agent-browser-init.sh` | - | One-shot Chrome launcher (autorestart=false) |
| openbox | `/opt/gem/openbox.sh` | - | Window manager for the VNC desktop |
| fcitx5 / dbus / autocutsel | - | - | CJK IME + clipboard + dbus session |
| log_tail | - | - | Centralized log tailing |

## The server (python-server)

- Console script `python-server` -> `from app.cli import cli` (package literally named `app`,
  **cyclopts** CLI framework). Launched as `app.cli:cli` with the args above.
- Stack: **FastAPI** (openapi `info.title=FastAPI`, 140 ops) + **FastMCP** (the integrated
  MCP hub; `fastmcp` binary present in site-packages). OTEL auto-instrumented via
  `PYTHONPATH=/otel-auto-instrumentation-python/...` (`SRV_PYTHONPATH`).
- One process serves **both** the REST API (`/v1/*`) and the MCP hub (`/mcp`, `/v1/mcp`),
  on port 8091. `--mcp-config /opt/gem/mcp-hub.json --filter-mcp-servers sandbox` exposes a
  single aggregate "sandbox" MCP server.
- `mcp-hub.json` lists only `browser` as an **external** streamable-http MCP server
  (`http://127.0.0.1:8100/mcp`). `file` / `terminal` / `markitdown` MCP tools are served
  **internally** by the python-server itself. (Matches docs: hub aggregates
  browser/file/terminal/markitdown - **not** chrome-devtools as guessed earlier.)
- The `app` package source is **not** in `/opt/gem` and was not extracted; its contract is
  the OpenAPI spec.

## Gateway (nginx, dual-server auth)

`nginx.conf` (user `nobody:root`) includes a rendered `nginx-server-active.conf` chosen
from `nginx-server-with-auth.conf` / `without-auth.conf`. With auth enabled it is a
**dual-server** setup:

```
client -> :8080 public server
            │  map $request -> @proxy_with_auth (default) | @proxy_without_auth (static/ping)
            │  @proxy_with_auth:  auth_request /_auth_handler
            │                      └─► internal :8081 server ─► proxy_pass http://127.0.0.1:8091/auth
            └─► proxy_pass to backend per location
```

Auth logic lives in the python-server (`/auth`, `/tickets`); nginx 8081 is just an
internal subrequest forwarder. Auth methods: API key (`SANDBOX_API_KEY` via `X-AIO-API-Key`
/ `Authorization: Bearer` / `?api_key=`) and JWT tickets (`auth.createTicket` /
`auth.authenticate`).

### Route table (nginx -> backend)

| Route | Backend | Notes |
|---|---|---|
| `/index.html`, `/static/sandbox/` | static `/opt/aio`, `/var/www/app` | dashboard |
| `/terminal`, `/terminal/` | static `/opt/terminal/` | web terminal UI (separate from the WS API) |
| `/v1/*`, `/v1/shell/ws`, `/llms.txt`, `/health`, `/v1/ping` | python-server :8091 | REST API + shell WS |
| `/mcp`, `/v1/mcp` | python-server :8091 | MCP hub (streamable-http, 86400s timeout) |
| `/code-server/` | code-server :8200 | VSCode (ws-upgrade, 3600s) |
| `/proxy/{port}/`, `/absproxy/{port}/` | code-server :8200 | dev-server preview via code-server's proxy |
| `${port}-${host}` wildcard | code-server :8200 | wildcard-domain preview (`VSCODE_PROXY_URI`) |

## Browser / VNC stack

Chrome (`/usr/local/bin/browser`, anti-automation flags, CDP 9222) runs on a virtual X
display `:99.0` managed by **TigerVNC `Xvnc`** (port 5900, no auth - internal only).
**`websocat`** bridges the noVNC websocket (6080) to the raw VNC TCP (5900). **openbox**
is the window manager; `agent-browser-init.sh` launches Chrome; `browser-supervisor.py`
(a trusted helper started in `gem_init.sh`) manages the browser lifecycle. CJK input via
fcitx5 + dbus. This whole stack exists so agents (and humans via `/vnc/`) can see/drive
a real browser.

## Code execution

- **Python**: executed by the python-server itself (subprocess / kernel).
- **Node.js**: three self-built REPL servers (`/opt/repl-servers/nodejs/server.js`), one
  per Node version (20/22/24), on ports 8192/8092/8392. The python-server dispatches to
  the selected version.
- **Jupyter**: JupyterLab (port 8888) with persistent kernels; `/jupyter/` prefix.

## Corrections vs earlier assumptions

| Earlier guess (research 01-07) | Actual (from image) |
|---|---|
| HTTP proxy = tinyproxy (env `TINYPROXY_PORT`) | **`gost`** (`/opt/gem/gost.yaml`); env var name is legacy |
| VNC = x11vnc + websockify | **TigerVNC `Xvnc`** + **`websocat`** ws->tcp |
| MCP hub = separate service | **Integrated in python-server** (FastMCP); only mcp-server-browser is external |
| MCP hub aggregates browser/markitdown/chrome-devtools | Aggregates **browser/file/terminal/markitdown** |
| Server tech = "Python FastAPI" | Confirmed: **FastAPI + FastMCP**, package `app`, CLI `app.cli:cli`, cyclopts, OTEL-instrumented |
| Auth = "gateway-enforced" | **nginx dual-server**: public 8080 + internal auth 8081 (subrequest) -> python-server `/auth` |

## Implications for a self-build (the user's goal)

For a **personal Docker + WebUI dev environment** (no SDK/agent), you need only a small
slice of this. Map the reference to a minimal build:

| Reference component | Self-build MVP | Keep optional |
|---|---|---|
| nginx gateway + auth | **yes** - caddy/nginx reverse proxy + basic auth | JWT tickets (skip) |
| code-server (8200) | **yes** - the IDE+terminal, the main event | - |
| `/proxy/{port}/` preview | **yes** - caddy path-proxy or code-server built-in | wildcard domain |
| volume on `/home/gem` | **yes** - persistence | - |
| python-server (8091) | **no** - that's the SDK/agent API surface you're skipping | add later behind `/api` if you extend |
| MCP hub / mcp-server-browser | no | add later behind `/mcp` |
| TigerVNC + websocat + openbox + Chrome | no | only if you want a browser-in-browser |
| gost proxy | no | only if you need outbound proxy control |
| node REPL servers / jupyter | no | only if you want in-browser code exec |

**Extensible seam**: keep the reverse proxy as the single entry point and reserve an
`/api` (and `/mcp`) path. When you later want SDK/agent support, drop in a FastAPI +
FastMCP service on those paths - exactly how AIO structures it. The reference's
supervisord + nginx + render-on-startup pattern is a good template for a multi-service
container even if you only run 2-3 services.

## Caveats

- `python_srv_wrapper.sh`, `supervisord.nginx.conf`, `nginx.python_srv.conf`,
  `mcp-hub.json`, `gost.yaml` are **rendered at startup** (envsubst / heredoc), so the
  static image has empty/templated versions; I read the templates + the supervisord
  commands that consume them.
- The `app` Python package (server source) was **not** extracted - by design (architecture
  only, not source reproduction). Contract = `openapi.json`.
- A few minor services (autocutsel, dbus, fcitx5, log_tail) are environment plumbing; not
  detailed.
