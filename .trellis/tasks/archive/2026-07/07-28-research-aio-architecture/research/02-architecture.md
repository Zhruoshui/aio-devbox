# Architecture & Components - AIO Sandbox

Derived from `docker-compose.yaml` env vars, `website/docs/en/guide/start/introduction.md`,
and the SDK resource layout. The server source is closed; this is reconstructed topology.

## One container, many services

AIO Sandbox is a **single Docker container** running ~12 cooperating services on fixed
internal ports, all sharing one filesystem (`WORKSPACE=/home/gem`). The compose file
publishes only `8080` to the host; everything else is internal.

```
                    ┌──────────────────────────────────────────┐
   host:8080 ──────►│  PUBLIC :8080  gateway / dashboard /     │
                    │  reverse-proxy to internal services      │
                    └───────────────┬──────────────────────────┘
                                    │
   ┌──────────────┬──────────────┬──┴───────────┬──────────────┬──────────────┐
   ▼              ▼              ▼              ▼              ▼              ▼
 AUTH :8081   SANDBOX_SRV   MCP_HUB :8079   WS_PROXY :6080  VNC :5900     GEM :8088
 (JWT/API key)  :8091        (MCP hub)       (noVNC ws)      (x11vnc)      (?)
                main API
                server
                                │                                              │
                ┌───────────────┼───────────────┐                              │
                ▼               ▼               ▼                              │
          JUPYTER :8888   CODE_SERVER     BROWSER                              │
                          :8200           (Chrome + CDP :9222)                 │
                                          │                                     │
                          ┌───────────────┼───────────────┐                     │
                          ▼               ▼               ▼                     │
                    MCP_BROWSER    MCP_MARKITDOWN  MCP_CHROME_DEVTOOLS          │
                      :8100           :8101            :8102                    │
                                                                        TINYPROXY :8118
```

## Component port table (from `docker-compose.yaml`)

| Port | Env var | Service | Role |
|------|---------|---------|------|
| 8080 | `PUBLIC_PORT` | gateway | Dashboard `/index.html`, docs, reverse-proxies internal services, MCP at `/mcp` |
| 8081 | `AUTH_BACKEND_PORT` | auth backend | JWT / API-key auth (`SANDBOX_API_KEY`, `JWT_PUBLIC_KEY`) |
| 8091 | `SANDBOX_SRV_PORT` | **sandbox server** | **Main API server the SDK talks to** (Python README uses `base_url=...:8091`) |
| 8079 | `MCP_HUB_PORT` | MCP hub | Aggregates the 3 MCP servers; `WAIT_PORTS` waits on it |
| 6080 | `WEBSOCKET_PROXY_PORT` | noVNC ws proxy | WebSocket bridge for browser-based VNC viewer |
| 5900 | `VNC_SERVER_PORT` | VNC server | x11vnc over the X display driving Chrome |
| 8088 | `GEM_SERVER_PORT` | "gem" server | Unknown - likely the user/session/preview manager (home is `/home/gem`) |
| 8888 | `JUPYTER_LAB_PORT` | JupyterLab | Jupyter kernel sessions |
| 8200 | `CODE_SERVER_PORT` | code-server | In-browser VSCode |
| 9222 | `BROWSER_REMOTE_DEBUGGING_PORT` | Chrome CDP | DevTools Protocol endpoint for browser automation |
| 8100 | `MCP_SERVER_BROWSER_PORT` | MCP browser | MCP server wrapping browser ops |
| 8101 | `MCP_SERVER_MARKITDOWN_PORT` | MCP markitdown | MCP server for doc->markdown |
| 8102 | `MCP_SERVER_CHROME_DEVTOOLS_PORT` | MCP chrome-devtools | MCP server wrapping raw CDP |
| 8118 | `TINYPROXY_PORT` | tinyproxy | HTTP proxy for the container's outbound traffic |

`WAIT_PORTS=8079,8091` = the entrypoint blocks startup until MCP hub + sandbox server
are healthy. `seccomp:unconfined`, `shm_size: 2gb`, `mem_limit 8g`, `cpus 4` - the
browser needs shared memory + relaxed seccomp.

## HTTP route layout (from `introduction.md`)

| Route | Backend |
|-------|---------|
| `/index.html` | web dashboard (Code / Browser / Terminal / Jupyter tabs) |
| `/vnc/index.html` | noVNC viewer |
| `/code-server/` | code-server (VSCode) |
| `/v1/shell/ws` | WebSocket interactive terminal |
| `/v1/bash/*` | pipe-based bash exec |
| `/v1/code/*`, `/v1/jupyter/*`, `/v1/nodejs/*` | code runtimes |
| `/mcp` | MCP hub (aggregated MCP servers) |
| `/proxy/{port}/`, `/absproxy/{port}/` | dev preview proxy to in-sandbox apps |
| `${port}-${domain}` | wildcard-domain preview routing |

## Auth model

- Optional API key: `SANDBOX_API_KEY` env. Three injection methods: `X-AIO-API-Key`
  header, `Authorization: Bearer`, or `?api_key=` query. Without it, services are open
  (backward compatible).
- JWT: `JWT_PUBLIC_KEY` env enables ticket-based auth (`auth.createTicket` /
  `auth.authenticate` SDK methods).

## Isolation / "cloud-native lightweight sandbox"

The docs describe "cloud-native lightweight sandbox technology" but the runtime for it
is **not in the repo**. Observations: runs as user `gem` (`/home/gem`), container-level
isolation via Docker with `seccomp:unconfined`. The actual cell/microVM tech (if any)
is baked into the image. For a replica, **plain Docker container isolation is the
default substitute**; stronger options (nsjail / gVisor / Firecracker) are available
if needed.
