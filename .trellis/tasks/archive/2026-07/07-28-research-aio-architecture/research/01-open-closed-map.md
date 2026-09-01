# Open / Closed Map - agent-infra/sandbox

Source: shallow clone of `https://github.com/agent-infra/sandbox.git` at
`/tmp/aio-sandbox-ref` (Apache-2.0). Inspected 2026-07-28.

## Verdict

The repo is **open source (Apache-2.0)**, but it open-sources only the **client side**:
SDKs, examples, evaluation, and docs. The **server runtime - the actual AIO Sandbox
that provides browser/shell/file/vscode/jupyter/mcp - is closed**, shipped solely as a
prebuilt Docker image `ghcr.io/agent-infra/sandbox:latest`. There is **no Dockerfile and
no server source** in the repo.

So the developer's original instinct ("it's not open source") is correct *for the part
that matters*: the server. The client surface is open.

## What is OPEN (source in the repo)

| Path | Contents | Notes |
|------|----------|-------|
| `sdk/js/` | TypeScript SDK, **Fern-generated** | `src/api/resources/*` enumerates the full server API contract. Tests + examples present. |
| `sdk/python/` | Python SDK (`agent_sandbox`), **Fern-generated** | Mirrors the JS SDK resource-for-resource. Sync + `AsyncSandbox`. |
| `sdk/fern/` | Fern API definition (generators config) | The single source spec that generates both JS + Python SDKs. |
| `sdk/go/` | **README only** - points to a separate repo | Real Go SDK lives at `github.com/agent-infra/sandbox-sdk-go` (not vendored here). |
| `examples/` | ~15 integration examples | browser-use, LangChain, OpenAI, ag2, minimax, playwright, oss-upload, etc. |
| `evaluation/` | Eval harness + dataset (XML) + results | `agent_loop.py`, `main.py`, datasets, MiniMax/OpenAI integration tests. |
| `website/` | Docs site (en + zh) | `guide/basic/*` and `guide/advanced/*` describe behavior per component. **Best behavioral spec available.** |
| `README.md`, `CONTRIBUTING.md`, `LICENSE` | Project docs | Architecture overview + quick start. |
| `docker-compose.yaml` | Compose that **pulls the prebuilt image** | Reveals the full component architecture via env vars (see `02-architecture.md`). |

## What is CLOSED (prebuilt image only)

Everything inside `ghcr.io/agent-infra/sandbox:latest`. Specifically these repo dirs are
**empty placeholders** (only a `.gitkeep`):

| Path | Expected contents | Actual |
|------|-------------------|--------|
| `docker/` | Dockerfile(s) to build the image | **empty** (`.gitkeep` only) |
| `cli/` | `aio` CLI tool (referenced in `guide/basic/aio-cli.md`) | **empty** (`.gitkeep` only) |

The closed server runtime must implement (inferred from the SDK + env vars + docs):
an auth backend, a noVNC websocket proxy, a VNC server, a "gem" server, an MCP hub,
the main sandbox API server, JupyterLab, code-server, three MCP servers
(browser / markitdown / chrome-devtools), tinyproxy, and a Chrome instance with CDP -
plus the orchestration/entrypoint that starts them all and the web dashboard at
`/index.html`.

## Implications for replication

- You **cannot read the server source** - you must reimplement from: the SDK API
  surface (contract), the docs (behavior), and the env-var/route layout (topology).
- The SDKs are **Fern-generated from a single API definition** (`sdk/fern/`). That
  Fern definition (if it contains the OpenAPI/IR spec) is the most valuable artifact
  for a reimplementation - worth extracting in any deeper research.
- A faithful full replica = reimplementing a ~12-service container plus ~120 API
  methods. See `04-replication-scope.md` for difficulty and options.
