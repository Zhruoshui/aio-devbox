# Replication Scope - recommendation

Premise: server is closed (prebuilt image only); SDK + docs are open. "Replicate" =
rebuild a server the open SDKs can talk to. Difficulty varies enormously by layer.

## Per-layer difficulty

| Layer | Methods | Difficulty | Why |
|-------|---------|-----------|-----|
| **file** | 17 | Medium | Mostly straightforward FS ops, but `str_replace_editor` (Aider-style) + file watching add real complexity |
| **bash** | 7 | Low | Pipe-based exec with session state - easy |
| **shell** | 12 | Low-Medium | PTY + WebSocket terminal; well-trodden path (xterm.js + node-pty) |
| **code / nodejs / jupyter** | ~14 | Medium | code/nodejs = subprocess runners; jupyter = jupyter_client kernel mgmt |
| **mcp hub** | 3 | Low-Medium | Aggregate a few MCP servers; standard MCP protocol |
| **auth** | 2 | Low | API-key + JWT ticket |
| **sandbox (context/hooks)** | ~14 | Medium | `getContext`/packages easy; **observe*** (live screen recording/replay) is hard |
| **proxy** | 11 | Low | tinyproxy wrapper - config API over an existing proxy |
| **skills / util / display** | ~8 | Low-Medium | skills = registry; util = markitdown; display = screen record |
| **browser + browserPage + tabs/cookies/network/state/captcha** | ~55 | **Very High** | Playwright-class automation over CDP, VNC display, screenshot, HAR, network routes. This is the dominant cost. |
| **code-server / VSCode** | - | Low (integration) | Just run code-server behind the gateway; little custom code |
| **dashboard / gateway** | - | Medium | Reverse proxy + the 4-tab web UI |
| **isolation runtime** | - | Variable | Docker default = trivial; nsjail/gVisor/Firecracker = real work |

The **browser layer is ~half the API and most of the complexity**. Everything else is
achievable incrementally.

## Scope options

**Option A - Full replica (replace runtime)**
Reimplement all 20 resources + 12 services + isolation. Largest effort; the browser
layer alone is a multi-week CDP+VNC project. Only choose if you need a fully
self-hosted, no-black-box system.

**Option B - Core-sandbox subset (recommended starting point)**
Reimplement only the "agent execution" core: `file`, `bash`, `shell`, `code`,
`nodejs`, `jupyter`, `mcp`, `auth`, `sandbox.getContext/packages/hooks`. Skip browser,
VNC, vscode, dashboard, observe/recording. Delivers a usable code-execution sandbox
the SDK can drive, at a fraction of the cost. Browser/vscode can be added later as
independent child tasks.

**Option C - Thin clone on the official image**
Don't rebuild the server; just write your own SDK/CLI/docs against the published
`ghcr.io/agent-infra/sandbox:latest` image. Smallest effort, but you still depend on
the closed image (defeats "replicate").

**Option D - SDK + server-contract only**
Extract the Fern/OpenAPI contract, publish a clean spec, implement a minimal
reference server for a few resources as a proof-of-concept. Useful if the goal is
interoperability rather than parity.

## Recommendation

Start with **Option B** (core-sandbox subset). It is the highest value-per-effort,
produces a working agent sandbox the open SDKs can drive, and leaves the expensive
browser/VNC/vscode layers as optional add-on child tasks you can sequence later.
Within Option B, suggested child-task order:

1. Container base + gateway + auth (API key)
2. `sandbox.getContext` + `file` (read/write/list/grep/str_replace)
3. `bash` + `shell` (PTY/WS terminal)
4. `code` + `nodejs` + `jupyter` runtimes
5. `mcp` hub + one MCP server
6. (optional) `proxy`, `skills`, `util`, `observe`

## Open questions for the developer (decide before build tasks)

1. **Goal**: full self-hosted parity (A), a working core subset (B), or just
   interop (D)?
2. **Language/stack** for the server (the repo's open parts are TS+Python; the server
   itself could be Node, Python, Go, or Rust)?
3. **Isolation**: plain Docker (default), or stronger (nsjail/gVisor/Firecracker)?
4. **Reuse the open SDKs as-is** (SDK-first), or also fork/rebuild the SDKs?

These answers determine the parent/child task tree for the build phase.
