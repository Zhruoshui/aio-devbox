# Research AIO Sandbox architecture

## Goal

Determine what `agent-infra/sandbox` (AIO Sandbox) actually open-sources versus keeps
closed, map its runtime architecture and full API surface, and produce a replication
scope recommendation so the developer can decide **whether and how** to replicate it.

This is a **research / scoping task only**. No code is written here. Its output is the
set of notes under `research/` plus a scope recommendation delivered to the developer.

## Background

- Developer's original premise: "the project is not open source."
- Reality (confirmed by cloning `https://github.com/agent-infra/sandbox.git` to
  `/tmp/aio-sandbox-ref`): the **client SDKs, examples, evaluation, and docs are
  Apache-2.0 open**, but the **server runtime is shipped only as a prebuilt Docker
  image** (`ghcr.io/agent-infra/sandbox:latest`). No Dockerfile, no server source.
- The SDK's API surface is therefore the **contract** any server reimplementation
  must satisfy. The `docker-compose.yaml` env vars reveal the component architecture.

## Research questions

1. **Open / closed map** — exactly what is in the repo vs. what lives only in the
   published image? (→ `research/01-open-closed-map.md`)
2. **Architecture** — what services run inside the container, on what ports/routes,
   and how do they relate? (→ `research/02-architecture.md`)
3. **API surface** — what endpoints/operations must a replica server implement?
   (→ `research/03-api-surface.md`)
4. **Replication scope** — how hard is each layer to rebuild, and what are the
   realistic scope options + a recommendation? (→ `research/04-replication-scope.md`)

## Acceptance criteria

- [x] Reference repo cloned and structurally mapped (`/tmp/aio-sandbox-ref`)
- [x] `research/01-open-closed-map.md` — open/closed verdict with evidence
- [x] `research/02-architecture.md` — components, ports, routes
- [x] `research/03-api-surface.md` — full resource/method enumeration
- [x] `research/04-replication-scope.md` — difficulty + scope options + recommendation
- [x] Findings + recommendation presented to developer; await decision on next step

### Deep research (round 2 - developer opted to go deeper)

- [x] `research/05-openapi-contract.md` - master OpenAPI 3.1.0 contract (FastAPI v1.9.4, 140 ops, `Response[T]` envelope, 255 schemas, route table by tag)
- [x] `research/06-core-exec-behavior.md` - file/bash/shell/code/nodejs/jupyter behavior + state machines + 12-item reimpl checklist
- [x] `research/07-orchestration-behavior.md` - sandbox/hooks/observe/mcp/auth/skills/proxy/display/util behavior

- [x] `research/08-server-architecture-reverse-engineered.md` - reverse-engineered server architecture from the Docker image (supervisord process model, nginx dual-server gateway, FastAPI+FastMCP `app` server, TigerVNC+websocat+gost)

- [x] `research/09-doc-server-crosscheck.md` - bidirectional doc<->server cross-check (18 doc pages vs 15 openapi tags vs routes vs processes; 7 gaps flagged)

## Key findings (cross-cutting)

- Server is a **Python FastAPI** app (OpenAPI `info.title=FastAPI`, v1.9.4). Original stack confirmed.
- `website/docs/public/v1/openapi.json` is the **single source contract** (Fern generates all 3 SDKs from it) - a reimplementation can be driven directly from it.
- Unified `Response[T]` envelope (`success`/`message`/`data`/`hint`) on 126/140 ops; file-watch, file-download, and a few others deviate.
- `sandbox.observe*` is **resource sampling** (cgroup/disk/process, guardrail/capture modes, exportable reports), NOT screen recording; `display.record` is the screen recorder.
- Auth is **gateway-enforced** (Nginx `auth_request` -> `/auth`; ticket flow `/tickets`), not in OpenAPI security schemes.
- CDP (`/cdp/json/version`), Jupyter (`/jupyter`), MCP streamable-HTTP (`/mcp`, `/v1/mcp`) are **not in `paths`** - only a pointer in `info.description`; must be built without an OpenAPI contract.

## Out of scope

- Writing any implementation code
- Choosing the final replication scope (that is the developer's decision after research)
- Filling `.trellis/spec/` (deferred — spec should be derived from the chosen scope)

## Status

Deep research complete (round 2). Task remains in `planning` status (no `task.py start` —
there is no implementation to execute). Awaiting developer decision on whether to
proceed to build tasks and at what scope.
