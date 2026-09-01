# Research: AIO OpenAPI Contract (master API-contract analysis for reimplementation)

- **Query**: Extract and document the full API contract from `/tmp/aio-sandbox-ref/website/docs/public/v1/openapi.json` for reimplementation — route table by tag, response envelope, error model, auth model, non-`/v1` routes, schema inventory, version.
- **Scope**: external (read-only analysis of a reference OpenAPI 3.1.0 spec + the Fern `generators.yml` that consumes it)
- **Date**: 2026-07-28

## Source provenance

- Spec file: `/tmp/aio-sandbox-ref/website/docs/public/v1/openapi.json` — 460,161 bytes, OpenAPI **3.1.0**, 123 paths, 140 operations, 255 schemas.
- `info.title` = **`FastAPI`**, `info.version` = **`1.9.4`**. The original server is a **Python FastAPI** app (schemas are pydantic-generated; enum/union naming like `Response[Union[A, B]]` and `app__models__browser_sdk__WaitRequest` confirm pydantic v2 + FastAPI).
- `/tmp/aio-sandbox-ref/sdk/fern/generators.yml` confirms this single file is the **only** source spec for all SDKs:
  - `api.specs: [../../website/docs/public/v1/openapi.json]`
  - `python-sdk` → `fernapi/fern-python-sdk@4.28.0` → package `agent_sandbox`, client class `Sandbox`
  - `js-sdk` → `fernapi/fern-typescript-sdk@3.28.4` → `../js/src`
  - `sdk-go` → `fernapi/fern-go-sdk@1.12.4` → `github.com/agent-infra/sandbox-sdk-go`
- 137 of 140 operations carry Fern SDK extensions (`x-fern-sdk-group-name`, `x-fern-sdk-method-name`), so method names in the generated SDKs are pinned by the spec.

## Findings

### 1. Top-level shape

- Top-level keys: `openapi`, `info`, `paths`, `components` only. **No `servers`, no `security`, no `tags` block.**
- `components` has **only** `schemas` — no `securitySchemes`, no `parameters`, no `responses`, no `requestBodies`. Everything is inline per-operation.
- Auth & base URL are **entirely out-of-band** (see section 5).

### 2. Route table by tag (full, 140 ops / 123 paths)

Tags come from each operation (no top-level `tags`). Every path is tagged.

#### `sandbox` (14)
```
GET    /v1/sandbox                          -- Get Sandbox Context
GET    /v1/sandbox/packages/python          -- Python Packages
GET    /v1/sandbox/packages/nodejs          -- Nodejs Packages
POST   /v1/sandbox/hooks                    -- Register Hook
GET    /v1/sandbox/hooks                    -- List Hooks
DELETE /v1/sandbox/hooks/{name}             -- Remove Hook
GET    /v1/sandbox/observe/status           -- Observe Status
GET    /v1/sandbox/observe/live             -- Observe Live
POST   /v1/sandbox/observe/start            -- Observe Start
POST   /v1/sandbox/observe/stop             -- Observe Stop
POST   /v1/sandbox/observe/export           -- Observe Export
GET    /v1/sandbox/observe/reports          -- Observe Reports
GET    /v1/sandbox/observe/reports/{report_id}  -- Observe Report Download
DELETE /v1/sandbox/observe/reports/{report_id}  -- Observe Report Delete
```

#### `shell` (12)
```
POST   /v1/shell/exec                       -- Exec Command
POST   /v1/shell/view                       -- View Shell
POST   /v1/shell/wait                       -- Wait For Process
POST   /v1/shell/write                      -- Write To Process
POST   /v1/shell/kill                       -- Kill Process
POST   /v1/shell/sessions/create            -- Create Session
POST   /v1/shell/sessions/update            -- Update Session
GET    /v1/shell/terminal-url               -- Get Terminal Url
GET    /v1/shell/sessions/stats             -- Get Session Stats
GET    /v1/shell/sessions                   -- List Sessions
DELETE /v1/shell/sessions                   -- Cleanup All Sessions
DELETE /v1/shell/sessions/{session_id}      -- Cleanup Session
```

#### `bash` (7)
```
POST   /v1/bash/exec                        -- Exec
POST   /v1/bash/output                      -- Output
POST   /v1/bash/write                       -- Write
POST   /v1/bash/kill                        -- Kill
GET    /v1/bash/sessions                    -- Sessions
POST   /v1/bash/sessions/create             -- Create Session
POST   /v1/bash/sessions/{session_id}/close -- Close Session
```

#### `file` (11)
```
POST   /v1/file/read                        -- Read File
POST   /v1/file/write                       -- Write File
POST   /v1/file/replace                     -- Replace In File
POST   /v1/file/search                      -- Search In File
POST   /v1/file/find                        -- Find Files
POST   /v1/file/grep                        -- Grep Files
POST   /v1/file/glob                        -- Glob Files
POST   /v1/file/upload                      -- Upload File
GET    /v1/file/download                    -- Download File
POST   /v1/file/list                        -- List Path
POST   /v1/file/str_replace_editor          -- Str Replace Editor
```

#### `file-watch` (6)
```
GET    /v1/file/watch                       -- List Watches
POST   /v1/file/watch                       -- Create Watch
GET    /v1/file/watch/{watcher_id}/events   -- Watch Events
POST   /v1/file/watch/{watcher_id}/poll     -- Poll Events
POST   /v1/file/watch/wait                  -- Wait For File
DELETE /v1/file/watch/{watcher_id}          -- Stop Watch
```

#### `jupyter` (6)
```
POST   /v1/jupyter/execute                  -- Execute Jupyter Code
GET    /v1/jupyter/info                     -- Jupyter Info
GET    /v1/jupyter/sessions                 -- List Sessions
DELETE /v1/jupyter/sessions                 -- Cleanup All Sessions
DELETE /v1/jupyter/sessions/{session_id}    -- Cleanup Session
POST   /v1/jupyter/sessions/create          -- Create Jupyter Session
```

#### `nodejs` (7)
```
POST   /v1/nodejs/execute                   -- Execute Nodejs Code
GET    /v1/nodejs/info                      -- Nodejs Info
POST   /v1/nodejs/sessions                  -- Create Nodejs Session
GET    /v1/nodejs/sessions                  -- List Nodejs Sessions
GET    /v1/nodejs/sessions/{session_id}     -- Get Nodejs Session
PATCH  /v1/nodejs/sessions/{session_id}     -- Update Nodejs Session
DELETE /v1/nodejs/sessions/{session_id}     -- Delete Nodejs Session
```

#### `mcp` (3)
```
GET    /v1/mcp/servers                      -- List Mcp Servers
GET    /v1/mcp/{server_name}/tools          -- List Mcp Tools
POST   /v1/mcp/{server_name}/tools/{tool_name}  -- Execute Mcp Tool
```

#### `browser` (52)
```
GET    /v1/browser/info                     -- Get Browser Info
GET    /v1/browser/screenshot               -- Take Screenshot
POST   /v1/browser/actions                  -- Execute Action
POST   /v1/browser/config                   -- Set Config
POST   /v1/browser/page/navigate            -- Navigate
POST   /v1/browser/page/back                -- Go Back
POST   /v1/browser/page/forward             -- Go Forward
POST   /v1/browser/page/reload              -- Reload
POST   /v1/browser/page/click               -- Click
POST   /v1/browser/page/fill                -- Fill
POST   /v1/browser/page/type                -- Type Text
POST   /v1/browser/page/press_key           -- Press Key
POST   /v1/browser/page/hot_key             -- Hot Key
POST   /v1/browser/page/hover               -- Hover
POST   /v1/browser/page/select_option       -- Select Option
POST   /v1/browser/page/check               -- Check
POST   /v1/browser/page/uncheck             -- Uncheck
POST   /v1/browser/page/upload_file         -- Upload File
POST   /v1/browser/page/fill_form           -- Fill Form
POST   /v1/browser/page/scroll              -- Scroll
POST   /v1/browser/page/scroll_to           -- Scroll To
POST   /v1/browser/page/scroll_to_element   -- Scroll To Element
GET    /v1/browser/page/screenshot          -- Page Screenshot
POST   /v1/browser/page/record              -- Page Record
GET    /v1/browser/page/html                -- Get Html
GET    /v1/browser/page/text                -- Get Text
GET    /v1/browser/page/markdown            -- Get Markdown
GET    /v1/browser/page/elements            -- Get Interactive Elements
GET    /v1/browser/page/console             -- Get Console Logs
POST   /v1/browser/page/console/export      -- Export Console Logs
POST   /v1/browser/page/evaluate            -- Evaluate
POST   /v1/browser/page/find_text           -- Find Text
POST   /v1/browser/page/wait                -- Wait
GET    /v1/browser/tabs                     -- List Tabs
POST   /v1/browser/tabs                     -- Create Tab
DELETE /v1/browser/tabs/{index}             -- Close Tab
PUT    /v1/browser/tabs/{index}/activate    -- Activate Tab
GET    /v1/browser/cookies                  -- Get Cookies
POST   /v1/browser/cookies                  -- Set Cookies
DELETE /v1/browser/cookies                  -- Clear Cookies
POST   /v1/browser/state/save               -- Save State
POST   /v1/browser/state/load               -- Load State
POST   /v1/browser/network/headers          -- Set Extra Headers
POST   /v1/browser/network/scoped_headers   -- Set Scoped Headers
POST   /v1/browser/network/route            -- Add Route
DELETE /v1/browser/network/route            -- Remove Route
GET    /v1/browser/network/requests         -- Get Requests
POST   /v1/browser/network/export_har       -- Export Har
GET    /v1/browser/captcha/detect           -- Detect Captcha
POST   /v1/browser/captcha/wait             -- Wait For Captcha
POST   /v1/browser/restart                  -- Restart
GET    /v1/browser/proxy.pac                -- Get Proxy Pac
```

#### `code` (2)
```
POST   /v1/code/execute                     -- Execute Code
GET    /v1/code/info                        -- Code Info
```

#### `util` (1)
```
POST   /v1/util/convert_to_markdown         -- Convert To Markdown
```

#### `skills` (5)
```
POST   /v1/skills/register                  -- Register Skills
GET    /v1/skills/metadatas                 -- List Skills Metadata
DELETE /v1/skills                           -- Clear Skills
DELETE /v1/skills/{name}                    -- Delete Skill
GET    /v1/skills/{name}/content            -- Get Skill Content
```

#### `proxy` (11)
```
GET    /v1/proxy/mappings                   -- List Mappings
POST   /v1/proxy/mappings                   -- Add Mapping
DELETE /v1/proxy/mappings/{source}          -- Remove Mapping
GET    /v1/proxy/excludes                   -- List Excludes
POST   /v1/proxy/excludes                   -- Add Exclude
DELETE /v1/proxy/excludes                   -- Remove Exclude
GET    /v1/proxy/diagnose                   -- Diagnose
GET    /v1/proxy/health                     -- Health Check
GET    /v1/proxy/upstream                   -- Get Upstream
PUT    /v1/proxy/upstream                   -- Set Upstream
DELETE /v1/proxy/upstream                   -- Remove Upstream
```

#### `display` (1)
```
POST   /v1/display/record                   -- Record
```

#### `auth` (2)
```
POST   /tickets                             -- Create Ticket
GET    /auth                                -- Authenticate Request
```

Path-format summary: 118 of 123 paths are under `/v1/...`. The 5 non-`/v1` paths in the paths table are: `/tickets`, `/auth`, and **none** for CDP/Jupyter/MCP-streamable (those are described in `info.description` only — see section 6).

### 3. Response envelope pattern (`Response[T]`)

There is a **single unified envelope**, generated by pydantic as a generic `Response[T]`. The base schema `Response` (title `"Response"`, desc *"Generic response model for API interface return results"*) is:

```json
{
  "success": { "type": "boolean", "default": true, "description": "Whether the operation was successful" },
  "message": { "anyOf": [{"type":"string"},{"type":"null"}], "default": "Operation successful", "description": "Operation result message" },
  "data":    { "anyOf": [{}, {"type":"null"}], "description": "Data returned from the operation" },
  "hint":    { "anyOf": [{"type":"string"},{"type":"null"}], "description": "Context hint for AI agents (e.g. tab changes)" }
}
```

For each typed endpoint FastAPI emits a concrete `Response[T]` schema named `Response_<T>_` (simple type) or `Response_Union_A__B__` (union). The `title` field preserves the Python generic form, e.g. `"Response[BashExecResult]"`, `"Response[Union[FileReadResult, FileOperationError]]"`. The four fields are identical across all envelopes; only the `data` `$ref` differs.

**Sample envelope response refs (200):**

| Operation | 200 schema `$ref` | Inner `data` type |
|---|---|---|
| `POST /v1/bash/exec` | `#/components/schemas/Response_BashExecResult_` | `BashExecResult \| null` |
| `POST /v1/file/read` | `#/components/schemas/Response_Union_FileReadResult__FileOperationError__` | `FileReadResult \| FileOperationError \| null` |
| `POST /v1/shell/exec` | `#/components/schemas/Response_ShellCommandResult_` | `ShellCommandResult \| null` |
| `POST /v1/browser/actions` | `#/components/schemas/ActionResponse` | `ActionData \| null` (envelope **subclass**) |
| `POST /v1/file/watch` | `{}` (empty schema) | plain object (see exceptions) |

**Concrete envelope shape — `Response_BashExecResult_`** (quote):
```json
{
  "properties": {
    "success": { "type": "boolean", "default": true, "description": "Whether the operation was successful" },
    "message": { "anyOf": [{"type":"string"},{"type":"null"}], "default": "Operation successful", "description": "Operation result message" },
    "data":    { "anyOf": [{"$ref":"#/components/schemas/BashExecResult"},{"type":"null"}], "description": "Data returned from the operation" },
    "hint":    { "anyOf": [{"type":"string"},{"type":"null"}], "description": "Context hint for AI agents (e.g. tab changes)" }
  },
  "type": "object",
  "title": "Response[BashExecResult]"
}
```

**Envelope subclass — `ActionResponse`** (browser `/v1/browser/actions` only). Adds two backward-compat fields on top of the four envelope fields, and its `data` is `ActionData` (`{status:"success", action_performed:string}`):
```json
{
  "success": bool (default true),
  "message": string|null,
  "data":    ActionData|null,
  "hint":    string|null,
  "status": { "const": "success", "default": "success" },
  "action_performed": { "type": "string", "default": "" }
}
```
Schema description states: *"Inherits from Response for unified API format, with backward compatibility: Old format: resp.json()['status'], resp.json()['action_performed']; New format: resp.json()['success'], resp.json()['message'], resp.json()['data']."*

**`SandboxResponse`** (`GET /v1/sandbox`) is another envelope extension: same four fields (`success/message/data/hint`, with `data: string|null`) plus `home_dir`, `workspace`, `version`, `detail: SandboxDetail`. So it is also envelope-shaped.

**Response-classification census (140 ops):**

| 200-response class | ops | notes |
|---|---|---|
| `Response[T]` typed envelope | 74 | the canonical case |
| bare `Response` (untyped `data: any`) | 51 | envelope with `data` schema `{}` — mostly browser page-action POSTs, deletes, util, proxy deletes |
| `ActionResponse` (envelope subclass) | 1 | `/v1/browser/actions` |
| `SandboxResponse` (envelope extension) | 1 | `GET /v1/sandbox` |
| empty/no-JSON schema | 11 | binary/raw responses (see below) |
| inline generic object | 2 | `/tickets` (`additionalProperties:true`), `/auth` (`additionalProperties:{type:string}`) |

So **126 of 140 operations** (90%) return the `Response[T]` envelope (typed + untyped + ActionResponse + SandboxResponse).

**Binary / non-JSON 200 responses (11 ops)** — these bypass the envelope and return raw bytes:
- `GET /v1/file/download` → `application/octet-stream`
- `GET /v1/browser/screenshot` → `image/png`, `image/jpeg`
- `GET /v1/browser/page/screenshot` → `image/png`
- `GET /v1/browser/proxy.pac` → `application/x-ns-proxy-autoconfig`
- `GET /v1/sandbox/observe/reports/{report_id}` → `application/gzip` (report download)
- `file-watch` endpoints (`GET/POST /v1/file/watch`, `GET .../events`, `POST .../poll`, `POST /v1/file/watch/wait`, `DELETE /v1/file/watch/{watcher_id}`) — 6 ops with empty `{}` schema (the watch payload shape is not formally modeled; implementer must infer from `CreateWatchRequest`/`PollRequest` request bodies and the `file-watch` tag).

### 4. Error model

**HTTP-level validation error** (the universal `422` on every operation that has a request body / typed response): `HTTPValidationError`
```json
{
  "detail": { "items": {"$ref": "#/components/schemas/ValidationError"}, "type": "array", "title": "Detail" }
}
```
`ValidationError` (FastAPI/pydantic standard):
```json
{
  "loc": { "items": {"anyOf":[{"type":"string"},{"type":"integer"}]}, "type": "array", "title": "Location" },
  "msg": { "type": "string", "title": "Message" },
  "type": { "type": "string", "title": "Error Type" }
}
```
`loc`, `msg`, `type` are all **required**. This is the stock FastAPI 422 shape — no customization.

**Domain-level error inside the envelope** — file operations return errors **not** as HTTP errors but as a union member inside `data`. `FileOperationError` (title `"FileOperationError"`, desc *"Structured file-tool execution error."*):
```json
{
  "path":          { "type": "string" },                      // required
  "operation":     { "type": "string" },                      // required
  "message":       { "type": "string" },                      // required
  "error_type":    { "type": "string" },                      // required, "Normalized file error category"
  "retryable":     { "type": "boolean", "default": false },
  "errno":         { "anyOf":[{"type":"integer"},{"type":"null"}] },
  "errno_name":    { "anyOf":[{"type":"string"},{"type":"null"}] },
  "exception_type":{ "anyOf":[{"type":"string"},{"type":"null"}] }
}
```
Eight `Response_Union_<FileResult>__FileOperationError__` envelopes exist (one per file op: read, write, replace, search, find, grep, glob, list, upload, str_replace_editor), so every file endpoint can return either its result type or `FileOperationError` inside `data` while still HTTP 200 with `success` reflecting the outcome.

**No other custom error schemas** exist. There is no top-level error envelope; non-422 business failures are conveyed via `success: false` + `message` + (optionally) a typed error in `data`.

### 5. Auth (out-of-band)

- `components` has **no `securitySchemes`**. Top-level **`security` is absent**. A full recursive grep of the spec JSON finds **0** occurrences of `"security"` and **0** of `"securitySchemes"`. No operation defines a `security` field either.
- **No `servers`** array — base URL is implied (the sandbox gateway, e.g. `https://<sandbox-host>`), supplied by the SDK client / runtime, not the spec.
- The two `auth`-tagged endpoints are **gateway plumbing**, not bearer-auth for the REST API:
  - `POST /tickets` — *"Create and return a short-lived authentication ticket. Non-idempotent; each call creates a new, unique ticket."* Response is a generic object (`additionalProperties:true`).
  - `GET /auth` — *"Authenticate a request using ticket or JWT. This endpoint receives authentication subrequests (e.g., from Nginx auth_request). Validates based on a ticket in the 'x-original-uri' header or a JWT in the 'Authorization' header."* Response is an object of strings.
- Conclusion for reimplementation: the REST API itself is **auth-agnostic at the spec level**. Real auth (API key `SANDBOX_API_KEY` / JWT) is enforced by an **external gateway** (Nginx `auth_request` → `/auth`, plus the ticket flow `/tickets`). A reimplementation can leave the OpenAPI auth-free and handle credentials at the gateway/proxy layer, matching the original.

### 6. Extra non-`/v1` routes (CDP / Jupyter / MCP-streamable)

These are declared **only in `info.description`** (the full 172-char description):
```
- Browser
    - CDP: /cdp/json/version
- Jupyter
    - Notebook: /jupyter
- MCP
    - Streamable HTTP: /mcp or /v1/mcp
```

They are **NOT present** in `paths`. Confirmed: no path contains `cdp`, and neither `/jupyter` nor `/mcp` nor `/v1/mcp` (as bare endpoints) exist as operations. (The `/v1/mcp/servers`, `/v1/mcp/{server_name}/tools` REST routes are separate from the `/v1/mcp` streamable-HTTP MCP transport endpoint.)

Implication: CDP (`/cdp/json/version` — Chrome DevTools Protocol discovery), the Jupyter notebook UI (`/jupyter`), and the MCP streamable-HTTP transport (`/mcp`, `/v1/mcp`) are **out-of-band endpoints served by the same origin** but **not formally specified**. A reimplementation must provide them as passthrough/proxy endpoints (CDP → browser backend, Jupyter → Jupyter server, MCP → MCP server) even though they carry no OpenAPI contract.

### 7. Schema inventory (255 total)

Breakdown:
- **64** `Response[T]` typed envelope schemas (`Response_<T>_` and `Response_Union_A__B__`)
- **1** `Response` base envelope + **1** `ActionResponse` subclass + **1** `SandboxResponse` extension = 66 envelope-related schemas
- **2** FastAPI error schemas: `HTTPValidationError`, `ValidationError`
- **~187** domain model schemas

Domain models grouped by resource prefix (names only):

| Resource | Count | Schema names |
|---|---|---|
| Bash | 10 | `BashCommandInfo`, `BashCommandStatus`, `BashExecRequest`, `BashExecResult`, `BashKillRequest`, `BashOutputRequest`, `BashOutputResult`, `BashSessionCreateRequest`, `BashSessionInfo`, `BashWriteRequest` |
| Shell | 15 | `ShellCommandResult`, `ShellCreateSessionRequest`, `ShellCreateSessionResponse`, `ShellExecRequest`, `ShellKillProcessRequest`, `ShellKillResult`, `ShellSessionInfo`, `ShellSessionStats`, `ShellUpdateSessionRequest`, `ShellViewRequest`, `ShellViewResult`, `ShellWaitRequest`, `ShellWaitResult`, `ShellWriteResult`, `ShellWriteToProcessRequest` |
| File | 21 | `FileContentEncoding`, `FileDownloadChangePolicy`, `FileFindRequest`, `FileFindResult`, `FileGlobRequest`, `FileGlobResult`, `FileGrepRequest`, `FileGrepResult`, `FileInfo`, `FileListRequest`, `FileListResult`, `FileOperationError`, `FileReadRequest`, `FileReadResult`, `FileReplaceRequest`, `FileReplaceResult`, `FileSearchRequest`, `FileSearchResult`, `FileUploadResult`, `FileWriteRequest`, `FileWriteResult` |
| StrReplaceEditor | 2 | `StrReplaceEditorRequest`, `StrReplaceEditorResult` |
| Browser (named `Browser*`) | 3 | `BrowserConfigRequest`, `BrowserInfoResult`, `BrowserViewport` |
| Browser Action types (`*Action`) | 16 | `ClickAction`, `DoubleClickAction`, `DragRelAction`, `DragToAction`, `HotkeyAction`, `KeyDownAction`, `KeyUpAction`, `MouseDownAction`, `MouseUpAction`, `MoveRelAction`, `MoveToAction`, `PressAction`, `RightClickAction`, `ScrollAction`, `TypingAction`, `WaitAction` (input list for `POST /v1/browser/actions`) |
| Browser/Action response | 2 | `ActionData`, `ActionResponse` |
| Jupyter | 6 | `JupyterCreateSessionRequest`, `JupyterCreateSessionResponse`, `JupyterExecuteRequest`, `JupyterExecuteResponse`, `JupyterInfoResponse`, `JupyterOutput` |
| Nodejs | 13 | `NodeJSCreateSessionRequest`, `NodeJSCreateSessionResponse`, `NodeJSDeleteSessionResponse`, `NodeJSExecuteRequest`, `NodeJSExecuteResponse`, `NodeJSOutput`, `NodeJSPackageInfo`, `NodeJSRuntimeInfo`, `NodeJSSessionInfo`, `NodeJSSessionListResponse`, `NodeJSSessionResponse`, `NodeJSUpdateSessionRequest`, `NodeJSUpdateSessionResponse` |
| Code | 4 | `CodeExecuteRequest`, `CodeExecuteResponse`, `CodeInfoResponse`, `CodeLanguageInfo` |
| Skill | 4 | `SkillContentResult`, `SkillMetadata`, `SkillMetadataCollection`, `SkillRegistrationResult` |
| Proxy | 7 | `ProxyBypassRequest`, `ProxyDiagnoseResult`, `ProxyHealthCheck`, `ProxyMappingAddRequest`, `ProxyMappingRoute`, `ProxyUpstreamInfo`, `ProxyUpstreamUpdateRequest` |
| Display | 2 | `DisplayRecordRequest`, `DisplayRecordResult` |
| Sandbox | 3 | `SandboxDetail`, `SandboxHook`, `SandboxResponse` |
| Observe / Observation | 13 | `ObserveExportRequest`, `ObserveStartRequest`, `ObserveStopRequest`, `ObservationCgroupSnapshot`, `ObservationDiskSnapshot`, `ObservationEvent`, `ObservationExportResult`, `ObservationLiveSnapshot`, `ObservationProcessSnapshot`, `ObservationReportInfo`, `ObservationStartResult`, `ObservationStatus`, `ObservationStopResult` |
| Util | 1 | `UtilConvertToMarkdownRequest` |
| Captcha | 2 | `CaptchaWaitRequest`, `CaptchaWaitResult` |
| Network (browser) | 7 | `ExportConsoleLogsRequest`, `ExportHarRequest`, `HeadersRequest`, `NetworkRouteRemoveRequest`, `NetworkRouteRequest`, `RouteResponseModel`, `ScopedHeadersRequest` |
| MCP protocol types | 15 | `Annotations`, `AudioContent`, `AvailableTool`, `BlobResourceContents`, `EmbeddedResource`, `Icon`, `ImageContent`, `ResourceLink`, `TextContent`, `TextResourceContents`, `Tool`, `ToolAnnotations`, `ToolCategory`, `ToolExecution`, `ToolSpec` (these mirror the MCP spec content/tool shapes) |
| Cookie | 1 | `CookieSetRequest` |
| Console | 1 | `ConsoleRecord` |
| Command | 1 | `CommandStatus` |
| Multipart bodies | 2 | `Body_register_skills`, `Body_upload_file` (the two `multipart/form-data` endpoints) |
| Session/misc shared | ~10 | `ActiveSessionsResult`, `ActiveShellSessionsResult`, `RuntimeEnv`, `SystemEnv`, `Language`, `DependencyCommandResult`, `GlobFileInfo`, `GrepMatch`, `SessionInfo`, `SessionStatus`, `Resolution`, `CreatePageRequest`, `RestartRequest`, `RecordRequest`, `RegisterHookRequest`, `StateLoadRequest`, `StateSaveRequest`, plus the page-level `*Request` schemas shared across browser endpoints (NavigateRequest, ClickRequest, FillRequest, … ~30 total) |
| FastAPI module-path duplicates | 2 | `app__models__browser_sdk__WaitRequest`, `app__schemas__file_watch__WaitRequest` (pydantic auto-names for `WaitRequest` classes that collide across modules — one for `browser/page/wait`, one for `file-watch/wait`) |

### 8. Request / input contract (supplementary)

- 76 operations declare a `requestBody`; 33 declare `parameters` (path or query); 35 declare neither (mostly DELETEs and GETs with no input).
- Request body content types: **`application/json`** (default) and **`multipart/form-data`** (only `POST /v1/file/upload` and `POST /v1/skills/register`, modelled as `Body_upload_file` / `Body_register_skills`).
- Path params: `{name}`, `{session_id}`, `{report_id}`, `{watcher_id}`, `{server_name}`, `{tool_name}`, `{index}`, `{source}`. Query params are rare (e.g. `GET /v1/sandbox/hooks?event=`, `GET /v1/file/download?path=&change_policy=`, `GET /v1/sandbox/observe/live?top_rows=`).
- Representative request shape — `BashExecRequest` (only `command` required):
  ```json
  {
    "session_id": string|null,    // reuse to continue a bash session; cd/export do NOT persist
    "command":    string,         // required
    "exec_dir":   string|null,    // absolute working dir, applied every call
    "env":        {string:string}|null,  // per-command only, does not persist
    "async_mode": bool (default false),  // if true, return running; poll via /output
    "timeout":    number|null,    // HTTP timeout (sync mode only)
    "hard_timeout": number|null,  // force-kill → status "timed_out"
    "max_output_length": int (default 50000)  // middle-truncation of stdout/stderr
  }
  ```
  Result `BashExecResult` carries `session_id`, `command_id`, `command`, `status` (`CommandStatus`: pending/running/completed/timed_out/killed), `stdout`, `stderr`, `exit_code`, `offset`, `stderr_offset` (offsets drive subsequent `POST /v1/bash/output` polling).
- `FileReadRequest` (only `file` required): `file`, `start_line` (0-based), `end_line` (not inclusive), `sudo` (default false).

## Caveats / Not Found

- **Spec is operationally self-contained for the 140 REST operations**, but the three integration transports (CDP `/cdp/json/version`, Jupyter `/jupyter`, MCP streamable `/mcp` + `/v1/mcp`) are **only named in `info.description`** — their request/response shapes are unspecified. A reimplementation must treat them as passthrough proxies and infer behavior from the backing services, not the OpenAPI.
- **`file-watch` responses are under-modelled**: all 6 file-watch operations return an empty `{}` 200 schema. The watch event payload is not in the spec; implementer must derive it from `CreateWatchRequest`/`PollRequest` and the underlying watcher semantics.
- **No examples, no `examples`, no `x-codegen`** anywhere in the spec. Sample payloads above were reconstructed from the pydantic schema `description` fields.
- **No `securitySchemes` / `servers` / `tags`** at top level — confirmed by structure inspection and a 0-count grep for `security`/`securitySchemes`. Auth is gateway-enforced (API key / JWT), not spec-enforced.
- **Two pydantic module-collision duplicates** (`app__models__browser_sdk__WaitRequest`, `app__schemas__file_watch__WaitRequest`) exist because both the browser `page/wait` and the `file-watch/wait` endpoints define a class named `WaitRequest` in different Python modules. The Fern generator handles the rename; a reimplementation should collapse these into distinct names.
- **`info.title` is literally `"FastAPI"`** (not a product name) and **`info.version` is `1.9.4`** — the version tracks the FastAPI app release, not the OpenAPI revision. There is no `info.description` beyond the 3-line CDP/Jupyter/MCP pointer.
- Fern SDK method names are pinned by `x-fern-sdk-method-name` on 137/140 ops; reimplementing SDKs should mirror these names for drop-in compatibility (3 ops lack the extension — worth a follow-up grep if exact SDK parity is required).
