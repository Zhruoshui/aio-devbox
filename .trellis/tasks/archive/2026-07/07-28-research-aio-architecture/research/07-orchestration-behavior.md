# Research: Orchestration / Auxiliary Resources Behavioral Deep-Dive

- **Query**: Behavioral deep-dive of `sandbox`, `mcp`, `auth`, `skills`, `proxy`, `display`, `util` resources so a reimplementation knows exactly what to build.
- **Scope**: mixed (Python SDK raw clients + OpenAPI + docs)
- **Date**: 2026-07-28

## Findings

### Files Found

| File Path | Description |
|---|---|
| `/tmp/aio-sandbox-ref/sdk/python/agent_sandbox/sandbox/raw_client.py` | Sandbox raw HTTP client (context, packages, hooks, observe*) |
| `/tmp/aio-sandbox-ref/sdk/python/agent_sandbox/sandbox/types/observe_start_mode.py` | `ObserveStartMode` enum |
| `/tmp/aio-sandbox-ref/sdk/python/agent_sandbox/mcp/raw_client.py` | MCP REST management client |
| `/tmp/aio-sandbox-ref/sdk/python/agent_sandbox/auth/raw_client.py` | Auth: tickets + auth_request handler |
| `/tmp/aio-sandbox-ref/sdk/python/agent_sandbox/skills/raw_client.py` | Skills registry client (multipart upload) |
| `/tmp/aio-sandbox-ref/sdk/python/agent_sandbox/proxy/raw_client.py` | Proxy control client (upstream/mappings/excludes/diagnose/health) |
| `/tmp/aio-sandbox-ref/sdk/python/agent_sandbox/display/raw_client.py` | Display screen-recording client |
| `/tmp/aio-sandbox-ref/sdk/python/agent_sandbox/util/raw_client.py` | Util: convert-to-markdown client |
| `/tmp/aio-sandbox-ref/website/docs/public/v1/openapi.json` | OpenAPI spec (FastAPI, v1.9.4) |
| `/tmp/aio-sandbox-ref/website/docs/en/guide/basic/{sandbox,mcp,skills,proxy,display,util}.md(x)` | Per-resource guides |
| `/tmp/aio-sandbox-ref/website/docs/en/guide/basic/authentication.md` | JWT + ticket flow |
| `/tmp/aio-sandbox-ref/website/docs/en/guide/advanced/{lifecycle,security,proxy-network,env-config}.md` | Lifecycle hooks, security, proxy/network env config |
| `/tmp/aio-sandbox-ref/README.md` | API-key auth mechanism (3 submission methods) |

### Common Response Envelope

All REST responses share the wrapper `{success: bool, message: str|null, data: T|null, hint: str|null}`. `hint` is described as "Context hint for AI agents (e.g. tab changes)" (OpenAPI `Response` schema). The SDK unwraps `.data` in the high-level `*Client` and exposes `.with_raw_response` for the raw envelope.

---

## sandbox

### HTTP Routes

| Method | Path | Op | Request | Response (data) |
|---|---|---|---|---|
| GET | `/v1/sandbox` | `get_sandbox_context` | — | `SandboxResponse` |
| GET | `/v1/sandbox/packages/python` | `python_packages` | — | `Response` (data = list of `{name, version}`) |
| GET | `/v1/sandbox/packages/nodejs` | `nodejs_packages` | — | `Response` (data = list of `{name, version}`) |
| GET | `/v1/sandbox/hooks?event=` | `list_hooks` | query `event?: str` | `Response_list_SandboxHook_` |
| POST | `/v1/sandbox/hooks` | `register_hook` | `RegisterHookRequest` | `Response_SandboxHook_` |
| DELETE | `/v1/sandbox/hooks/{name}` | `remove_hook` | path `name` | `Response` |
| GET | `/v1/sandbox/observe/status` | `observe_status` | — | `Response_ObservationStatus_` |
| GET | `/v1/sandbox/observe/live?top_rows=` | `observe_live` | query `top_rows?: int` | `Response_ObservationLiveSnapshot_` |
| POST | `/v1/sandbox/observe/start` | `observe_start` | `ObserveStartRequest` | `Response_ObservationStartResult_` |
| POST | `/v1/sandbox/observe/stop` | `observe_stop` | `ObserveStopRequest` | `Response_ObservationStopResult_` |
| POST | `/v1/sandbox/observe/export` | `observe_export` | `ObserveExportRequest` | `Response_ObservationExportResult_` |
| GET | `/v1/sandbox/observe/reports` | `observe_reports` | — | `Response_list_ObservationReportInfo_` |
| GET | `/v1/sandbox/observe/reports/{report_id}` | `observe_report_download` | path `report_id` | **binary stream** (no JSON schema; `{}` in OpenAPI) |
| DELETE | `/v1/sandbox/observe/reports/{report_id}` | `observe_report_delete` | path `report_id` | `Response_ObservationReportInfo_` |

Citations: `sandbox/raw_client.py:36,70,104,138,190,270,319,353,403,485,540,608,642,693`.

### Behavior & Models

**getContext** (`GET /v1/sandbox`) — `SandboxResponse` fields: `success, message, data (str|null), hint (str|null), home_dir (str), workspace (str|null), version (str), detail: SandboxDetail`. `SandboxDetail` = `{system: SystemEnv, runtime: RuntimeEnv, utils: [ToolCategory]}` (OpenAPI `SandboxDetail` schema). Docs example (`sandbox.mdx:137-160`) shows `data` is a human-readable text block (OS, user, home dir, timezone, occupied ports, dev tools, available utils). `home_dir` is a top-level field (e.g. `/home/gem`).

**Packages** — `Response.data` is a list of `{name, version}` objects, e.g. `[{"name":"requests","version":"2.32.5"}]` (`sandbox.mdx:174`).

**Hooks model** — Runtime API supports only `event="shutdown"`. Per `lifecycle.md:36-50`, shutdown hooks are registered at runtime via `POST /v1/sandbox/hooks` to flush state / write markers before exit.

- `RegisterHookRequest`: `name (str, required), command (str, required), event (str="shutdown"), timeout (float, per-hook seconds), priority (int, lower=earlier; same-priority hooks run in parallel)` (`sandbox/raw_client.py:190-268`).
- `SandboxHook` response adds `source (str)`: `"env"` or `"api"` (OpenAPI `SandboxHook` schema).
- `remove_hook` docstring: *"Remove a hook by name. ENV hooks cannot be removed."* (`sandbox/raw_client.py:270-275`). Only `source="api"` hooks are deletable.
- `list_hooks` accepts `?event=shutdown` filter.

**Startup hooks (env-only, NOT via this API):** `RUN_HOOK_INIT`, `RUN_HOOK_PRE_SERVICES`, `RUN_HOOK_POST_READY` — set as env vars at container start, executed in order: init → (user/workspace prepared) → pre-services → (services start) → (readiness) → post-ready (`lifecycle.md:7-15,48-54`). These are distinct from the runtime `shutdown` API hooks.

**observe\* model — NOT screen recording.** It is a **resource/guardrail sampling session** over cgroup CPU/memory, disk, top processes, and recent events. Two modes via `ObserveStartMode = Literal["guardrail","capture"]` (`sandbox/types/observe_start_mode.py:5`).

- `ObserveStartRequest`: `mode∈{guardrail,capture}, idempotency_key (str, max 128), duration_seconds (int), interval_seconds (float, sampling interval), include_processes (bool), include_disk (bool)` (`sandbox/raw_client.py:403-458`).
- `ObservationStartResult`: `session_id, mode, started_at, ends_at (datetime|null), interval_seconds, runtime_dir` — `runtime_dir` is where samples/reports are stored.
- `observe_status`: `mode, running, session_id?, started_at?, ends_at?, interval_seconds?, last_sample_at?, runtime_dir?, report_count` (`required: mode, running`).
- `observe_live?top_rows=N`: returns a point-in-time `ObservationLiveSnapshot` = `{captured_at, mode, cgroup: ObservationCgroupSnapshot, disk: [], top_processes: [], recent_events: []}`. `cgroup` carries `cpu_usage_pct, cpu_usage_usec, cpu_nr_periods, cpu_nr_throttled, cpu_throttled_usec, mem_current_bytes, mem_peak_bytes, ...`. `top_rows` limits process rows.
- `observe_stop {session_id?}`: `ObservationStopResult = {session_id, stopped, report_ready}` — `report_ready` signals whether an exportable report is available immediately.
- `observe_export {idempotency_key?, session_id?, reason}`: forces a report build; `ObservationExportResult = {report_id, session_id?, reason, created_at, path, size_bytes}`. `session_id` optional (defaults to active session). `idempotency_key` (max 128) for safe retries.
- `observe_reports`: lists `ObservationReportInfo` (`report_id, session_id?, reason, created_at, path, size_bytes`).
- `observe_report_download {report_id}`: **streamed binary** (context-manager in SDK, `sandbox/raw_client.py:642-691`), supports `chunk_size` request option.
- `observe_report_delete {report_id}`: returns the deleted `ObservationReportInfo`.

> Reimplementation note: `observe*` is a sampling/telemetry session with exportable report artifacts (file at `path`, `size_bytes`), NOT video. Screen recording is the separate `display` resource. Both `observe_start` and `observe_export` accept `idempotency_key` (maxLength 128) for retry-safety.

---

## mcp

### HTTP Routes

| Method | Path | Op | Request | Response (data) |
|---|---|---|---|---|
| GET | `/v1/mcp/servers?include_hidden=` | `list_mcp_servers` | query `include_hidden?: bool` | `Response_List_str_` (list of server names) |
| GET | `/v1/mcp/{server_name}/tools` | `list_mcp_tools` | path `server_name` | `Response_ListToolsResultModel_` |
| POST | `/v1/mcp/{server_name}/tools/{tool_name}` | `execute_mcp_tool` | body `Arguments` (dict) | `Response_CallToolResultModel_` |

Citations: `mcp/raw_client.py:26,82,155`.

### MCP Streamable-HTTP (protocol, NOT in OpenAPI paths)

The OpenAPI `info.description` states: *"MCP — Streamable HTTP: [/mcp](/mcp) or [/v1/mcp](/v1/mcp)"*. These are **MCP JSON-RPC streamable-HTTP transport endpoints** (the aggregated hub), distinct from the REST management routes above. Clients POST `{"method":"tools/call","params":{"name":"...","arguments":{...}}}` to `/mcp` (`mcp.md:44-90`). The bare `/mcp` and `/v1/mcp` paths do **not** appear in `openapi.json` `paths` (only the `/v1/mcp/servers`, `/v1/mcp/{name}/tools` REST routes do).

### Hub Aggregation

Docs (`mcp.md:11-32`) list **four** aggregated MCP servers: **browser**, **file**, **terminal**, **markitdown**. (The task prompt mentioned browser/markitdown/chrome-devtools; docs say browser/file/terminal/markitdown — reimplementation should follow the docs.) Tool examples: `browser_navigate`, `browser_click`, `browser_screenshot`, `file_read/write/list/search/replace`, `terminal_execute`, `terminal_session`, `terminal_kill`, `sandbox_execute_bash` (supports `truncate` param), `markitdown_convert`, `markitdown_extract` (`mcp.md:92-117`).

### Schemas

- `list_mcp_servers` returns server-name strings; `include_hidden=true` includes hidden servers (`mcp/raw_client.py:155-184`).
- `execute_mcp_tool` body is a free-form `Arguments` dict (`request: Dict[str, Optional[Any]]`, `mcp/raw_client.py:82-128`), sent as JSON with `content-type: application/json`.
- `CallToolResultModel` (MCP spec): `{content: [TextContent|ImageContent|AudioContent|ResourceLink|EmbeddedResource], structuredContent?: object, isError: bool (default false), _meta?: object}`, `required: [content]` (OpenAPI `Response_CallToolResultModel_`). `TextContent` = `{type:"text", text, annotations?, _meta?}`; `ImageContent` = `{type:"image", data, mimeType, ...}`.

---

## auth

### HTTP Routes

| Method | Path | Op | Request | Response |
|---|---|---|---|---|
| POST | `/tickets` | `create_ticket` | — | `Dict[str, Any]` → `{ticket, expires_in}` |
| GET | `/auth` | `authenticate` | (nginx auth_request subrequest) | `Dict[str, str]` |

Citations: `auth/raw_client.py:17,55`.

### Behavior & Models

**create_ticket** — *"Create and return a short-lived authentication ticket. This is a non-idempotent action; each call creates a new, unique ticket."* (`auth/raw_client.py:17-34`). Docs (`authentication.md:65-77`): default TTL **30s**, configurable via env `TICKET_TTL_SECONDS`. Response body: `{"ticket": "...", "expires_in": 30}`. Requires a valid JWT in `Authorization: Bearer` to obtain.

**authenticate** — *"This endpoint receives authentication subrequests (e.g., from Nginx auth_request). It validates the request based on either a ticket in the 'x-original-uri' header or a JWT in the 'Authorization' header."* (`auth/raw_client.py:55-79`). Returns `Dict[str, str]` (key/value pairs, e.g. user info). Nginx calls `GET /auth` internally; the ticket is parsed from the `x-original-uri` header (set by nginx from the original request's `?ticket=` query).

### Auth Mechanisms (enforced at gateway/nginx, NOT in OpenAPI securitySchemes)

The OpenAPI defines **no** `securitySchemes` and **no** global/per-op `security` — auth is enforced at the nginx/gateway layer in front of the FastAPI app. Three mechanisms (env-config + README):

1. **API key** — env `SANDBOX_API_KEY`. Three submission methods (`README.md:36-37`):
   - `X-AIO-API-Key` request header
   - `Authorization: Bearer <key>` header
   - `?api_key=<key>` query parameter
   - Without `SANDBOX_API_KEY`, services remain open (backward compatible).
2. **JWT** — env `JWT_PUBLIC_KEY` (base64-encoded RSA public key). Client sends `Authorization: Bearer <jwt>` (`security.md:5-30`, `env-config.md:45`). RS256; `exp` claim validated.
3. **Short-lived tickets** — obtained via `POST /tickets` (requires JWT). For clients that cannot set headers (e.g. VNC/websockify), pass via `?ticket=<ticket>` query (`authentication.md:43-91`). TTL via `TICKET_TTL_SECONDS`.
4. `AUTH_TOKEN` — *"Shared token for simple deployments, when supported by the running image"* (`env-config.md:46`).

> Reimplementation note: the `/auth` endpoint is the nginx `auth_request` target. To reproduce, run an nginx/gateway that intercepts all routes, extracts `?ticket=` / `?api_key=` / `Authorization` / `X-AIO-API-Key`, and issues an internal `GET /auth` subrequest with `x-original-uri` set; the FastAPI `/auth` validates ticket/JWT and returns 2xx/4xx.

---

## skills

### HTTP Routes

| Method | Path | Op | Request | Response (data) |
|---|---|---|---|---|
| POST | `/v1/skills/register` | `register_skills` | **multipart form**: `file` (zip), `path?`, `name?` | `Response_SkillRegistrationResult_` |
| GET | `/v1/skills/metadatas?names=` | `list_skills_metadata` | query `names?: str` | `Response_SkillMetadataCollection_` |
| GET | `/v1/skills/{name}/content` | `get_skill_content` | path `name` | `Response_SkillContentResult_` |
| DELETE | `/v1/skills/{name}` | `delete_skill` | path `name` | `Response_SkillMetadata_` (deleted skill's metadata) |
| DELETE | `/v1/skills` | `clear_skills` | — | `Response_dict_` |

Citations: `skills/raw_client.py:29,95,145,177,224`.

### Behavior & Models

Claude-Skills-style registry. A skill is a directory with `SKILL.md` (YAML frontmatter `{name, description, ...}` + markdown body) plus optional `scripts/`, `templates/`, `requirements.txt`, `package.json` (`skills.md:8-29`).

- **register** — two modes (`skills.md:33-47`):
  - Register an **existing in-sandbox directory**: `-F "path=/home/gem/skills/report-writer"` (no file).
  - **Upload a zip** and extract under a dest dir: `-F "file=@report-writer.zip" -F "path=/home/gem/skills" -F "name=report-writer"`.
  - SDK uses `force_multipart=True` always (`skills/raw_client.py:55-68`).
- `SkillRegistrationResult`: `{count: int, registered: [SkillMetadata]}` (`required: [count]`).
- `SkillMetadata`: `{name, path (absolute path to skill dir), metadata (object, parsed from SKILL.md frontmatter), dependency_commands: [DependencyCommandResult]}` (`required: [name, path]`).
- `list_metadata?names=` — `names` filters by skill name (comma-separated).
- `get_content` — returns `SkillContentResult = {name, path, content}` where `content` is the SKILL.md body **excluding frontmatter**.
- `delete_skill` — returns the deleted skill's `SkillMetadata` (not a generic Response).
- `clear_skills` — deletes all; returns `Response_dict_`.

---

## proxy

tinyproxy-style control plane (actually GOST + nginx under the hood; `health` checks both).

### HTTP Routes

| Method | Path | Op | Request | Response (data) |
|---|---|---|---|---|
| GET | `/v1/proxy/upstream` | `get_proxy_upstream` | — | `Response_Union_ProxyUpstreamInfo__NoneType__` (null if direct) |
| PUT | `/v1/proxy/upstream` | `set_proxy_upstream` | `ProxyUpstreamUpdateRequest` | `Response_ProxyUpstreamInfo_` |
| DELETE | `/v1/proxy/upstream` | `remove_proxy_upstream` | — | `Response` |
| GET | `/v1/proxy/mappings` | `list_proxy_mappings` | — | `Response_list_ProxyMappingRoute_` |
| POST | `/v1/proxy/mappings` | `add_proxy_mapping` | `ProxyMappingAddRequest` | `Response_ProxyMappingRoute_` |
| DELETE | `/v1/proxy/mappings/{source}` | `remove_proxy_mapping` | path `source` | `Response` |
| GET | `/v1/proxy/excludes` | `list_proxy_excludes` | — | `Response_list_str_` |
| POST | `/v1/proxy/excludes` | `add_proxy_exclude` | `ProxyBypassRequest` | `Response` |
| DELETE | `/v1/proxy/excludes` | `remove_proxy_exclude` | `ProxyBypassRequest` (**body**, not path) | `Response` |
| GET | `/v1/proxy/diagnose?url=` | `diagnose_proxy` | query `url` | `Response_ProxyDiagnoseResult_` |
| GET | `/v1/proxy/health` | `proxy_health_check` | — | `Response_ProxyHealthCheck_` |

Citations: `proxy/raw_client.py:31,67,128,177,213,270,327,380,416,452,519`.

### Behavior & Models

**Upstream** (outbound HTTP/HTTPS proxy for browser/tools):
- `ProxyUpstreamUpdateRequest`: `server (str, required)` — `host:port` or `user:pass@host:port`; `auth_cmd (str?)` — *"Optional shell command to obtain proxy credentials. The command stdout should be `username:password`. When set, the result is injected into the server URL, replacing any inline credentials."* (`proxy/raw_client.py:452-492`). **Takes effect immediately — no browser restart.**
- `ProxyUpstreamInfo`: `{addr (host:port), username?, password?}` (`required: [addr]`).
- `remove_upstream` → direct mode.
- Startup env equivalent: `PROXY_SERVER` (+ `PROXY_EXCLUDE` / `PROXY_INCLUDE` allow/deny lists, `PROXY_MAP` static mappings) (`proxy-network.md:9-21`, `env-config.md:16-19`).

**Mappings** (domain→port routing):
- `ProxyMappingAddRequest`: `source (str)` = `[protocol://]host[:port][/path]`, supports wildcard `*` in host (e.g. `*.example.com`); `target (str)` = `[host:]port[/path]`, host defaults to `127.0.0.1` (`proxy/raw_client.py:67-101`).
- `ProxyMappingRoute` response: `{source, target, source_host, source_path, internal_port}` — `internal_port` is the internal nginx listen port for that domain group; `source_host`/`source_path` are extracted for GOST hosts / nginx location config.
- `remove_mapping` takes `{source}` as a **path param** (`proxy/raw_client.py:128-150`).

**Excludes** (bypass/direct-connect):
- `ProxyBypassRequest`: `pattern (str)` — domain (`*.example.com`, `.example.com`) **or CIDR** (`10.0.0.0/8`) (`proxy/raw_client.py:213-243`).
- `add_exclude` and `remove_exclude` both POST/DELETE the **same JSON body** `{"pattern": "..."}` — `remove_exclude` is a **DELETE with body** (not a path param) (`proxy/raw_client.py:270-300`).

**diagnose** — `GET /v1/proxy/diagnose?url=`: `ProxyDiagnoseResult = {url, matched_mapping: ProxyMappingRoute|null, resolved_target: str|null, target_reachable: bool, route: str}` (`required: [url, route]`).

**health** — `GET /v1/proxy/health`: `ProxyHealthCheck = {healthy, gost_alive, nginx_alive, config_consistent, inconsistencies: []}`. `config_consistent` is *"Domain sets in proxy-map.json, gost-hosts.txt, and nginx conf are consistent"* (`proxy/raw_client.py:380-414`, OpenAPI schema desc).

**Inbound preview proxy** (separate from the REST control plane, served by nginx): `/proxy/{port}/` (backend/relative), `/absproxy/{port}/` (frontend/absolute), `x-aio-proxy-port` header (numeric only; trusted-gateway-set), subdomain `${port}-${domain}` (`proxy.md:7-25`, `proxy-network.md:27-43`).

---

## display

### HTTP Routes

| Method | Path | Op | Request | Response (data) |
|---|---|---|---|---|
| POST | `/v1/display/record` | `record` | `DisplayRecordRequest` | `Response_DisplayRecordResult_` |

Citation: `display/raw_client.py:24-90`.

### Behavior & Models

**This IS screen recording** (full X11 desktop via `ffmpeg x11grab`), distinct from `sandbox.observe*` (resource sampling) and browser page recording. *"Records the entire X11 desktop using ffmpeg x11grab, including browser UI, multiple tabs, popups, terminal, etc."* (`display/raw_client.py:37-41`).

`DisplayRecordRequest`:
- `action (str, required)` ∈ `{start, stop, status}` (enum `DisplayRecordRequestAction`).
- `save_path (str?)` — default `/tmp/recordings/recording_{timestamp}.mp4`.
- `fps (int?)`, `crf (int?)` — H.264 CRF quality (0=lossless, 51=worst).
- `max_duration (float?)` — max recording seconds.
- `width (int, exclusiveMinimum 0)?`, `height (int?)` — auto-detected from X11 if omitted.

`DisplayRecordResult`: `{status, save_path?, duration, file_size_bytes?}` (`required: [status]`). Docs tip (`display.md:34-38`): stop recording before deleting the container to get a complete playable file; store under workspace for retrieval via file API.

---

## util

### HTTP Routes

| Method | Path | Op | Request | Response (data) |
|---|---|---|---|---|
| POST | `/v1/util/convert_to_markdown` | `convert_to_markdown` | `UtilConvertToMarkdownRequest` | `Response` (data = markdown text) |

Citation: `util/raw_client.py:23-53`.

### Behavior & Models

`UtilConvertToMarkdownRequest`: `{uri (str, required)}` — the URI of the resource to convert (`util/raw_client.py:42-53`). Backed by **markitdown** (the same engine exposed as the `markitdown_convert` MCP tool). Response is the generic `Response` envelope with markdown text in `data`. Usage patterns (`util.md:20-23`): convert public web pages before summarization, convert downloaded documents after saving to sandbox FS, normalize content before indexing.

---

## Edge Cases / Non-Obvious Behavior

1. **observe\* ≠ screen recording.** `sandbox.observe*` is a cgroup/disk/process **sampling session** with two modes (`guardrail`, `capture`) and exportable report artifacts (file `path` + `size_bytes`, downloadable as a binary stream). Screen recording is `display.record` (ffmpeg x11grab). A reimplementation must not conflate the two.
2. **Hook removability depends on `source`.** `SandboxHook.source ∈ {"env","api"}`; `remove_hook` rejects env hooks: *"ENV hooks cannot be removed."* (`sandbox/raw_client.py:270-275`). Runtime API only registers `event="shutdown"`; startup hooks (`RUN_HOOK_INIT/PRE_SERVICES/POST_READY`) are env-only and not exposed via this API (`lifecycle.md:7-15`).
3. **Hook priority semantics:** lower number = earlier; **same priority runs in parallel** (`sandbox/raw_client.py:217-218`).
4. **observe idempotency:** both `observe_start` and `observe_export` accept `idempotency_key` (maxLength 128) for safe retries; `observe_stop` does not. `observe_stop` returns `report_ready` indicating whether a report is immediately available.
5. **MCP has two surfaces.** REST management (`/v1/mcp/servers`, `/v1/mcp/{name}/tools[/{tool}]`) is in OpenAPI; the MCP **streamable-HTTP JSON-RPC** hub at `/mcp` or `/v1/mcp` is **not** in OpenAPI paths (only mentioned in `info.description`). A reimplementation must serve both.
6. **MCP hub servers (per docs):** browser, file, terminal, markitdown (`mcp.md:11-32`). The task prompt's "browser/markitdown/chrome-devtools" does not match docs — file + terminal are the documented aggregated servers.
7. **Auth is gateway-enforced, not FastAPI-enforced.** OpenAPI defines **no** `securitySchemes`. `SANDBOX_API_KEY` accepts three submission forms: `X-AIO-API-Key` header, `Authorization: Bearer`, `?api_key=` query (`README.md:36-37`). `/auth` is the nginx `auth_request` subrequest target, validating ticket (from `x-original-uri` header) or JWT (from `Authorization`). Ticket TTL default 30s via `TICKET_TTL_SECONDS`.
8. **`POST /tickets` is non-idempotent** — each call mints a new unique ticket (`auth/raw_client.py:20-22`).
9. **proxy.remove_exclude is DELETE-with-body**, not DELETE-with-path. Both `add_exclude` and `remove_exclude` send `{"pattern": "..."}` JSON body (`proxy/raw_client.py:213-300`). Contrast `remove_mapping`, which uses path param `{source}`.
10. **proxy.set_upstream `auth_cmd`:** stdout must be exactly `username:password`; it is injected into the server URL **replacing any inline credentials** (`proxy/raw_client.py:469-471`). Takes effect immediately, no browser restart.
11. **proxy.health config consistency:** checks that domain sets in `proxy-map.json`, `gost-hosts.txt`, and nginx conf agree (`ProxyHealthCheck.config_consistent` + `inconsistencies[]`).
12. **skills.register is always multipart** (`force_multipart=True`, `skills/raw_client.py:66`). Two modes: in-sandbox dir (`path` only) vs zip upload (`file` + `path` dest + `name`).
13. **skills.get_content** returns SKILL.md body **excluding** frontmatter (`SkillContentResult.content`). `skills.delete_skill` returns the deleted skill's `SkillMetadata`, not a generic Response.
14. **Response envelope `hint` field** — "Context hint for AI agents (e.g. tab changes)" — present on all wrappers; a reimplementation should propagate it for agent-facing context signals.
15. **diagnose `route`** is a required string describing the resolved routing decision; `target_reachable` reflects TCP reachability of the resolved target.

## Caveats / Not Found

- **No `DisplayRecordRequestAction` schema** in OpenAPI components (referenced by SDK as a separate type, `display/types/display_record_request_action.py`); the enum values `start/stop/status` are documented only in the SDK docstring and `display.md`. Confirmed via SDK raw client.
- **`ListToolsResultModel` / `CallToolResultModel` / `Arguments`** are not top-level named schemas in OpenAPI; they are inlined into the `Response_*_` wrapper `data` anyOf. The `CallToolResult` shape follows the MCP spec (`content`, `structuredContent`, `isError`, `_meta`).
- The `/mcp` and `/v1/mcp` streamable-HTTP transport behavior (session init, JSON-RPC envelope, SSE/streaming framing) is **not specified in OpenAPI**; a reimplementation should follow the MCP streamable-HTTP spec and the examples in `mcp.md`.
- `Response Create Ticket Tickets Post` and `Response Authenticate Request Auth Get` schemas are **empty `{}`** in OpenAPI (free-form); the `{ticket, expires_in}` shape is documented only in `authentication.md` and the SDK types as `Dict[str, Any]` / `Dict[str, str]`.
- `AUTH_TOKEN` env var is listed in `env-config.md:46` as "Shared token for simple deployments, when supported by the running image" — no endpoint detail; treat as an alias/legacy form of `SANDBOX_API_KEY`.
- The exact set of "hidden" MCP servers returned by `list_mcp_servers?include_hidden=true` is not enumerated in docs; reimplementation should defer to runtime `mcp-servers.json` config.
