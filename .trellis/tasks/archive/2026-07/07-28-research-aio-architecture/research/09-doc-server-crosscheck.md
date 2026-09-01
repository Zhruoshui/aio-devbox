# Research: Doc <-> Server bidirectional cross-check

- **Query**: Do the documentation's feature claims correspond to the actual server, both ways?
- **Scope**: internal (cloned docs + openapi + reverse-engineered image)
- **Date**: 2026-07-28
- **Method**: Enumerated every `website/docs/en/guide/basic/*` feature page and mapped it
  to (a) an OpenAPI tag/ops, (b) an nginx gateway route, (c) a supervisord process - then
  reversed the check: every server tag/route/process looked up in the docs. Sources:
  `05-openapi-contract.md`, `08-server-architecture-reverse-engineered.md`, the cloned
  docs, and the extracted nginx configs.

## Verdict

**Strong 1:1 correspondence.** The 18 `guide/basic/*` feature pages map cleanly onto the
15 OpenAPI resource tags (140 ops) + the gateway routes + the supervisord processes. The
docs accurately describe what the server implements, and the server implements what the
docs claim. A handful of **minor doc-lag gaps** (below), no fundamental mismatches.

## Direction 1: Doc feature -> Server implementation

| Doc page (`guide/basic/`) | OpenAPI tag (ops) | nginx route | Process | Status |
|---|---|---|---|---|
| `aio-cli.md` (AIO CLI) | - (CLI wraps the API) | - | `/usr/local/bin/aio` v0.3.14 | ✓ in image; ✗ not in OSS repo (`cli/` empty) |
| `authentication.md` | `auth` (2) | `/auth`, `/tickets` (compat) + nginx auth gateway | python-server `/auth` | ✓ |
| `bash.md` (Bash Pipe) | `bash` (7) | `/v1/bash/*` | python-server | ✓ |
| `browser.mdx` (Browser & VNC) | `browser`+`browserPage`+`tabs`+`cookies`+`network`+`state`+`captcha` (52) | `/browser-ui`, `/cdp/`, `/vnc/` | mcp-server-browser + Xvnc + websocat + Chrome | ✓ |
| `code.md` (Unified Code Execution) | `code` (2) | `/v1/code/*` | python-server | ✓ |
| `code-server.md` (Code Server) | - (no API) | `/code-server/` | code-server :8200 | ✓ |
| `display.md` (Display Recording) | `display` (1) | `/v1/display/*` | ffmpeg on X11 (on-demand) | ✓ |
| `error-handling.md` | - | - | - | meta doc (cross-cutting, no endpoint) |
| `jupyter.md` (Jupyter) | `jupyter` (6) | `/v1/jupyter/*` + `/jupyter` | jupyter :8888 | ✓ |
| `mcp.md` (MCP Integration) | `mcp` (3) | `/mcp`, `/v1/mcp` | python-server (FastMCP) + mcp-server-browser | ✓ |
| `nodejs.md` (Node.js) | `nodejs` (7) | `/v1/nodejs/*` | 3× node REPL (8092/8192/8392) | ✓ |
| `proxy.md` (Preview Proxy) | `proxy` (11) | `/proxy/{port}/`, `/absproxy/{port}/` + gost :8118 | code-server preview + gost | ⚠ doc/blog says **TinyProxy**, image uses **gost** |
| `shell.md` (Shell Terminal) | `shell` (12) | `/v1/shell/ws` + `/terminal` (UI) | python-server (PTY) | ✓ |
| `skills.md` (Skills) | `skills` (5) | `/v1/skills/*` | python-server | ✓ |
| `util.md` (Utilities) | `util` (1) | `/v1/util/*` | python-server (markitdown) | ✓ |
| `code-execution.mdx` | - | - | - | overview doc (ties code+nodejs+jupyter) |
| `file.mdx` (File Operations) | `file` (11) + `file-watch` (6) | `/v1/file/*` | python-server | ✓ (watch endpoints listed in doc) |
| `sandbox.mdx` (Sandbox Info) | `sandbox` (14) | `/v1/sandbox/*` | python-server | ⚠ partial: covers context/packages/hooks, **NOT `observe*`** |

## Direction 2: Server -> Doc (anything implemented but undocumented?)

| Server surface | Documented? | Note |
|---|---|---|
| All 15 OpenAPI tags | ✓ | each has a `basic/` page (except `file-watch` folded into `file.mdx`) |
| `/vnc/` (noVNC) | ✓ | `browser.mdx`, `quick-start.mdx`, `introduction.md` |
| `/cdp/devtools/` | ✓ | browser docs |
| `/jupyter` | ✓ | `jupyter.md` |
| `/codex`, `/opencode` | ✓ | `guide/advanced/codex.md`, `opencode.md` |
| **`/claudecode`** | ✗ | route + binary in image, **no doc page** (codex/opencode have one) |
| `sandbox.observe*` (14 ops: Start/Stop/Status/Live/Export/Reports/...) | ✗ | **no doc page**, not in `sandbox.mdx` (resource sampling/capture - see `07`) |
| gost (proxy process) | ⚠ | `proxy-network.md` is generic; **blog names TinyProxy** (stale) |
| compat routes `/screenshot`, `/actions` | ✗ | routed (gembrowser_compat) but **not in OpenAPI paths**; `examples/browser.md` references `/vnc/screenshot`, `/vnc/keyboard` (GUI-level ops) - contract unclear |
| infra: Xvnc, websocat, openbox, fcitx5, dbus, autocutsel, log_tail | - | plumbing, no feature doc needed |
| `aio` CLI | ✓ doc + ✓ image | but **not in OSS repo** (`cli/` empty) |
| Go SDK | ✓ doc | but `sdk/go/` is a README pointer to a separate repo |

## Discrepancies & gaps (ranked)

1. **TinyProxy (docs/blog) vs gost (image)** - the blog (`announcing-0.mdx`) and legacy
   naming (`TINYPROXY_PORT` env) say TinyProxy; the image actually runs `gost`
   (`/usr/local/bin/gost -C /opt/gem/gost.yaml`). Doc lag after a migration.
2. **`sandbox.observe*` undocumented** - 14 operations (resource sampling in
   guardrail/capture modes, exportable reports) with no doc page and not in `sandbox.mdx`.
   Server ahead of docs.
3. **`/claudecode` undocumented** - Claude Code is bundled (binary + `/claudecode` route)
   like Codex/OpenCode, but only Codex/OpenCode got `advanced/` doc pages.
4. **`sandbox.mdx` partial** - documents context/packages/hooks but omits the `observe*`
   half of the `sandbox` tag.
5. **Compat routes not in OpenAPI** - `/screenshot`, `/actions`, `/auth`, `/tickets` are
   routed to python-server but absent from `paths`; `examples/browser.md` uses
   `/vnc/screenshot` & `/vnc/keyboard` (GUI-level) whose contract is unclear.
6. **`aio` CLI closed in repo** - documented and present in the image, but the OSS repo's
   `cli/` is empty (built elsewhere, baked into image). Repo gap, not doc-server gap.
7. **Go SDK** - documented; repo `sdk/go/` is a placeholder to a separate repo. Repo gap.

## Confirmed matches (no discrepancy)

- 15 API resource tags ↔ 15 `basic/` doc pages (1:1, modulo the 2 meta overview docs and
  `file-watch` folded into `file.mdx`).
- Every gateway route in `nginx/*.conf` either has a feature doc or is infrastructure
  (`/health`, `/llms.txt`, `/v1/ping`, `/static`).
- Every supervisord process either has a doc or is plumbing.
- The reverse-engineered process model (08) is consistent with the docs' claimed
  component list (Browser+VNC, VSCode, Shell, File, MCP, Code Execute, Preview Proxy,
  Service Management) - including the corrections (gost, TigerVNC+websocat, integrated
  MCP hub) which the docs do not contradict (they just don't name the tools).

## Bottom line for the self-build

The docs are a **reliable spec** for the server's behavior - every documented feature is
really implemented, and the API contract (`openapi.json`) is complete for the `/v1/*`
surface. The only things to distrust slightly: the proxy tool name (use gost, not
tinyproxy), and the undocumented corners (`observe*`, `/claudecode`, compat routes) - which
you won't need for a personal WebUI dev environment anyway.
