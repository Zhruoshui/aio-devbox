# Research: Core Execution Resources Behavioral Deep-Dive

- **Query**: Behavioral deep-dive of `file` (+`file-watch`), `bash`, `shell`, `code`, `nodejs`, `jupyter` so a reimplementation knows exactly what to build.
- **Scope**: mixed (internal SDK raw clients + OpenAPI schema + docs)
- **Date**: 2026-07-28

## Sources Read

- Python SDK raw clients: `/tmp/aio-sandbox-ref/sdk/python/agent_sandbox/{file,bash,shell,code,nodejs,jupyter}/raw_client.py` (and `file/types/*`)
- OpenAPI: `/tmp/aio-sandbox-ref/website/docs/public/v1/openapi.json` (operations + component schemas)
- Docs: `/tmp/aio-sandbox-ref/website/docs/en/guide/basic/{file,bash,shell,code,nodejs,jupyter,code-execution}.md(x)`

## Cross-cutting conventions

### Response envelopes

- **Standard `Response[T]` wrapper**: almost every successful endpoint returns `{ success: bool, message: str, data: T }`. HTTP 200 + `success=false` is used for *expected* application failures (not just errors). The SDK parses these into `ResponseUnionXxxResult_FileOperationError` discriminated unions for file ops, and into dedicated `ResponseBashExecResult` / `ResponseShellCommandResult` etc. for execution.
- **HTTP 422** = `HttpValidationError` (pydantic validation). Raised as `UnprocessableEntityError` in the SDK.
- **409 Conflict** = used only by `GET /v1/file/download` with `change_policy=abort` (`ConflictError`).

### File-op error shape (non-standard, important)

File `read/write/replace/search/find/grep/glob/list/upload/str_replace_editor` return **HTTP 200 with `success=false`** for filesystem failures, and `data` is an error object (NOT the result union):

```json
{
  "success": false,
  "message": "Failed to read file: [Errno 2] No such file or directory: '/tmp/missing.txt'",
  "data": {
    "path": "/tmp/missing.txt",
    "operation": "read",
    "message": "...",
    "error_type": "not_found",
    "retryable": false,
    "errno": 2,
    "errno_name": "ENOENT",
    "exception_type": "FileNotFoundError"
  }
}
```

Known `error_type` values: `not_found`, `permission_denied`, `invalid_target`, `already_exists`, `invalid_path`, `read_only_filesystem`, `no_space_left`, `decode_error`, `io_error`. Reimplementation must reproduce both the success-path `data` payload AND this 200-with-error envelope. (doc: `file.mdx:29-58`)

### `OMIT` sentinel

SDK marks optional params `= OMIT` (the ellipsis `...`). The HTTP layer drops keys equal to `OMIT` from the JSON body. A reimplementation should treat "absent" vs "explicit null" distinctly: absent keys are omitted from the request, not sent as `null`.

---

## 1. `file` (excluding watch)

### HTTP routes

| Method | Path | Request schema | Response (200) |
|---|---|---|---|
| POST | `/v1/file/read` | `FileReadRequest` | `Response_Union_FileReadResult__FileOperationError_` |
| POST | `/v1/file/write` | `FileWriteRequest` | `Response_Union_FileWriteResult__FileOperationError_` |
| POST | `/v1/file/replace` | `FileReplaceRequest` | `Response_Union_FileReplaceResult__FileOperationError_` |
| POST | `/v1/file/search` | `FileSearchRequest` | `Response_Union_FileSearchResult__FileOperationError_` |
| POST | `/v1/file/find` | `FileFindRequest` | `Response_Union_FileFindResult__FileOperationError_` |
| POST | `/v1/file/grep` | `FileGrepRequest` | `Response_Union_FileGrepResult__FileOperationError_` |
| POST | `/v1/file/glob` | `FileFileGlobRequest` | `Response_Union_FileGlobResult__FileOperationError_` |
| POST | `/v1/file/list` | `FileListRequest` | `Response_Union_FileListResult__FileOperationError_` |
| POST | `/v1/file/str_replace_editor` | `StrReplaceEditorRequest` | `Response_Union_StrReplaceEditorResult__FileOperationError_` |
| POST | `/v1/file/upload` | multipart `file` + form `path` | `Response_Union_FileUploadResult__FileOperationError_` |
| GET  | `/v1/file/download?path=&change_policy=` | query params | binary stream (or 409/422) |

All paths are versioned `v1/file/...` (the SDK sends relative `"v1/file/read"`; the base URL supplies the leading slash). Source: `file/raw_client.py:84,174,252,323,385,498,603,844,965,674,741`.

### Request parameters (name / type / optional)

**read** (`file/raw_client.py:49-57`): `file:str` (req), `start_line:int?` (0-based), `end_line:int?` (exclusive), `sudo:bool?`.
**write** (`:124-134`): `file:str` (req), `content:str` (req), `encoding:FileContentEncoding?` (`utf-8`|`base64`|`raw`), `append:bool?`, `leading_newline:bool?` (text only), `trailing_newline:bool?` (text only), `sudo:bool?`.
**replace** (`:217-224`): `file:str`, `old_str:str`, `new_str:str`, `sudo:bool?`. (Simple single replace; multi-mode replace lives in `str_replace_editor`.)
**search** (`:292-298`): `file:str`, `regex:str`, `sudo:bool?`. (Single-file regex search.)
**find** (`:362-363`): `path:str`, `glob:str`. (Simple filename glob; no extras.)
**grep** (`:423-440`): `path:str`, `pattern:str`, `include:seq[str]?`, `exclude:seq[str]?`, `case_insensitive:bool?`, `fixed_strings:bool?`, `context_before:int?` (-B), `context_after:int?` (-A), `max_results:int?`, `max_file_size:str?` (e.g. `"1M"`), `multiline:bool?` (rg `-U --multiline-dotall`), `offset:int?` (pagination), `type:str?` (ripgrep type alias), `recursive:bool?`. Backed by ripgrep.
**glob** (`:548-560`): `path:str`, `pattern:str`, `exclude:seq[str]?`, `include_hidden:bool?`, `files_only:bool?`, `include_metadata:bool?`, `max_results:int?`, `sort_by:str?` (`path`|`name`|`size`|`modified`), `sort_desc:bool?`.
**list** (`:789-801`): `path:str`, `recursive:bool?`, `show_hidden:bool?`, `file_types:seq[str]?` (e.g. `[".py",".txt"]`), `max_depth:int?`, `include_size:bool?`, `include_permissions:bool?`, `sort_by:str?` (`name`|`size`|`modified`|`type`), `sort_desc:bool?`.
**upload** (`:648-654`): multipart form `file` (streamed, `force_multipart=True`), `path:str?`. No JSON body.
**download** (`:712-748`): GET query `path:str`, `change_policy:FileDownloadChangePolicy?` (`abort` to abort if source changes). Streaming via `httpx.stream`; SDK exposes a context manager yielding `Iterator[bytes]`. `chunk_size` is read from `request_options`.
**str_replace_editor** (`:889-905`): `command:Command` (req), `path:str` (req), `file_text:str?`, `old_str:str?`, `new_str:str?`, `insert_line:int?`, `view_range:seq[int]?`, `replace_mode:StrReplaceEditorRequestReplaceMode?`, `page_range:seq[int]?`, `sheet_name:str?`, `row_range:seq[int]?`, `slide_range:seq[int]?`, `enable_metadata:bool?`.

`Command` enum = `view | create | str_replace | insert | undo_edit` (`file/types/command.py:5`). `replace_mode` = `ALL | FIRST | LAST` (`file/types/str_replace_editor_request_replace_mode.py:5`); omitted ⇒ requires unique match (original behavior). Docstring (`file/raw_client.py:906-913`): "The tool parameters are defined by Anthropic and are not editable" — this is the Anthropic str_replace_editor tool spec.

### Response payloads (schema fields)

- `FileReadResult` { `content:str`, `file:str` } (+ `line_count` per doc example).
- `FileWriteResult` { `file:str`, `bytes_written?:int` }.
- `FileGrepResult` { `path:str`, `pattern:str`, `matches:GrepMatch[]`, `match_count:int`, `files_searched?:int`, `files_matched?:int`, `truncated:bool` }. `GrepMatch` { `file:str`, `line_number:int` (1-based), `line_content:str`, `context_before?:str[]`, `context_after?:str[]` }.
- `FileGlobResult` { `path:str`, `pattern:str`, `files:GlobFileInfo[]`, `total_count:int`, `truncated:bool` }. `GlobFileInfo` { `path:str`, `name:str`, `is_directory:bool=false`, `size?:int`, `modified_time?:str` (ISO) }.
- `FileListResult` { `path:str`, `files:FileInfo[]`, `total_count:int`, `directory_count:int`, `file_count:int` }. `FileInfo` adds `permissions?:str`, `extension?:str` over `GlobFileInfo`.
- `FileUploadResult` { `file_path:str`, `file_size:int`, `success:bool` }.
- `StrReplaceEditorResult` { `output:str`, `path:str`, `prev_exist:bool`, `error?:str`, `old_content?:str`, `new_content?:str`, `metadata?:object` (only when `enable_metadata=true` for binary files) }.

### Non-obvious behavior

- **`str_replace_editor` view** supports binary/office formats: `page_range` for PDFs (1-indexed `[start,end]`), `sheet_name`/`row_range` for Excel, `slide_range` for PPTX, `enable_metadata` returns total pages/sheets/slides. `view_range` uses 1-based indexing; `[start_line, -1]` means "to EOF" (`file/raw_client.py:935-955`).
- **`sudo`** runs the operation as root (sandbox app user by default). `file.mdx:569-583` shows it used for `/etc/nginx/nginx.conf` etc.
- **`download` change_policy=abort** returns 409 if the source file's mtime/size changes before or during streaming (`file.mdx:256-261`, `raw_client.py:758-768`).
- **Shared filesystem**: file API, shell, browser downloads, code execution, code-server all see the same paths (e.g. `/home/gem/workspace`). `file.mdx:457-477`.
- **Encoding `raw`** = "Latin-1 style string" for advanced byte handling (`file.mdx:148-153`).

---

## 2. `file-watch` (tag `file-watch`, paths under `/v1/file/watch`)

Distinct from the `file` tag. **Does NOT use the standard `{success, message, data}` wrapper** — `file.mdx:58`: "File watch endpoints also use resource-style JSON and HTTP status codes instead of the standard `success` wrapper." The SDK types these as `Optional[Any]` (`file/raw_client.py:1016,1059,1132,1187,1259,1324`).

### HTTP routes

| Method | Path | Body | opId |
|---|---|---|---|
| GET | `/v1/file/watch` | – | list_watches |
| POST | `/v1/file/watch` | `CreateWatchRequest` | create_watch |
| GET | `/v1/file/watch/{watcher_id}/events` | – (SSE if `Accept: text/event-stream`) | watch_events |
| POST | `/v1/file/watch/{watcher_id}/poll` | `PollRequest` | poll_events |
| POST | `/v1/file/watch/wait` | `FileWatchWaitRequest` | wait_for_file |
| DELETE | `/v1/file/watch/{watcher_id}` | – | stop_watch |

### State machine / lifecycle

1. **create** (`POST /v1/file/watch`) → returns `watcher_id` (doc example reads `.data.watcher_id`, `file.mdx:315`). `CreateWatchRequest` defaults (OpenAPI):
   - `recursive: bool = true`
   - `exclude: [".git","node_modules","__pycache__",".venv","*.pyc","*.pyo",".DS_Store","*.swp","*.swo"]`
   - `debounce: int = 300` ms (range **50–5000**)
   - `include_patterns: []` (empty = all events)
   - required: `path`
2. **consume events** via one of:
   - **poll** (`POST /v1/file/watch/{id}/poll`): `cursor:int?` (last consumed `seq`; only events with `seq > cursor` returned), `limit:int?`, `timeout:int?` (long-poll seconds; **0 = return immediately**). Doc: "Pass the response `cursor` directly into the next poll; do not add one yourself. If `overflow=true`, refresh your local file tree and continue from the returned cursor." (`file.mdx:328`)
   - **events** (`GET /v1/file/watch/{id}/events`): SSE stream emitting `watch_started`, `file_change`, `overflow`. Each `file_change` carries the `FileEvent` shape; SSE id is `{watcher_id}:{seq}`. (`file.mdx:332-339`)
3. **wait** (`POST /v1/file/watch/wait`): one-shot wait for a single exact-match `path`. `FileWatchWaitRequest` { `path:str` (req), `timeout:int=30` (range **1–300**), `event_types:[create,write,remove,rename,chmod]` (default all 5) }. Returns `{ event: FileEvent }`. **Edge case**: "If the file already exists and `event_types` includes `create`, `wait` may return immediately with a `create` event. If you only care about future changes, omit `create`." (`file.mdx:300`)
4. **stop** (`DELETE /v1/file/watch/{id}`): releases the watcher. Doc pattern: `create + poll + delete` for reliable cleanup (`file.mdx:304-326`).

### Event types enum

`create | write | remove | rename | chmod` (`file/types/app_schemas_file_watch_wait_request_event_types_item.py:5-7`; same set in `CreateWatchRequest` consumption).

### FileEvent shape (from doc example, `file.mdx:283-298`)

```json
{
  "event": {
    "seq": 1,
    "type": "write",
    "path": "/tmp/demo/result.json",
    "relative_path": "result.json",
    "old_path": null,
    "is_dir": false,
    "timestamp": 1776823501.334,
    "mtime": 1776823501.321,
    "size": 2048,
    "inode": 91827555
  }
}
```

`old_path` is populated on `rename`. `inode` enables change detection across polls.

### Edge cases

- Watch endpoints bypass the standard `success` envelope — a reimplementation must serve plain resource JSON + HTTP status codes here.
- `list_watches` (`GET /v1/file/watch`) takes no params and returns the watcher set.
- Poll `overflow=true` is the signal to refresh the local file tree (events may have been dropped).

---

## 3. `bash` (subprocess pipe, tag `bash`)

### HTTP routes

| Method | Path | Body | Response |
|---|---|---|---|
| POST | `/v1/bash/exec` | `BashExecRequest` | `Response_BashExecResult_` |
| POST | `/v1/bash/output` | `BashOutputRequest` | `Response_BashOutputResult_` |
| POST | `/v1/bash/write` | `BashWriteRequest` | `Response` |
| POST | `/v1/bash/kill` | `BashKillRequest` | `Response` |
| GET  | `/v1/bash/sessions` | – | `Response_list_BashSessionInfo__` |
| POST | `/v1/bash/sessions/create` | `BashSessionCreateRequest` | `Response_BashSessionInfo_` |
| POST | `/v1/bash/sessions/{session_id}/close` | – | `Response` |

Source: `bash/raw_client.py:103,203,276,342,401,451,509`.

### `exec` request (`BashExecRequest`, `bash/raw_client.py:28-39`)

`command:str` (req), `session_id:str?`, `exec_dir:str?`, `env:Dict[str,str?]?`, `async_mode:bool?`, `timeout:float?`, `hard_timeout:float?`, `max_output_length:int?`.

### Command status state machine (`CommandStatus` enum)

```
pending → running → completed     (normal exit; exit_code set, 0=success, non-zero=failure)
                 → timed_out      (hard_timeout reached; process force-killed)
                 → killed         (via /v1/bash/kill, session cleanup, or internal exec failure)
```

Quote (`bash/raw_client.py:44-61`):
> - `running`: the process is still executing. Returned immediately for `async_mode=true`, or when sync mode hits `timeout` before completion.
> - `completed`: the process exited and `exit_code` is available. This does not imply success; non-zero shell exit codes still use `completed`.
> - `timed_out`: the process exceeded `hard_timeout` and was force-killed.
> - `killed`: the process was terminated by `/v1/bash/kill`, session cleanup, or an internal execution failure before normal completion.

`pending` exists in the enum (accepted but not started yet) but the docstrings only enumerate the four above as observable.

### Timeout semantics (critical distinction)

- `async_mode=true` → returns **immediately** with `running`; poll `/output`.
- `async_mode=false` + `timeout` → HTTP waits up to `timeout` s; if not done, returns `running` and the command **keeps running in the background**. Poll `/output`.
- `async_mode=false` + no `timeout` → waits until completion.
- `hard_timeout` → the actual process kill switch; on expiry status becomes `timed_out`. `None` = no limit.
- `max_output_length` → only effective in sync mode; **middle truncation** (head + tail preserved, middle replaced with a marker). `0` disables truncation. Doc default `50000` (`bash.md:241`).

### Polling model (`/v1/bash/output`, `BashOutputRequest`)

`session_id:str` (req), `command_id:str?`, `offset:int?`, `stderr_offset:int?`, `wait:bool?`, `wait_timeout:float?`.

- `offset`/`stderr_offset` are **byte offsets**; reuse the values from the previous response.
- `command_id` targets a specific async command; if unset, session-level output.
- `wait=true` long-polls until new output arrives, command finishes, or `wait_timeout` expires. `wait=false` returns currently-available output immediately.
- Stop polling when `data.command.status` is no longer `running`; use final `exit_code` for success.
- `BashOutputResult` { `session_id:str`, `stdout:str` (new data since last offset), `stderr:str`, `offset:int`, `stderr_offset:int`, `command?:BashExecResult` (current/most-recent command status) }.

### Session reuse & caveats (key behavior)

Quote (`bash/raw_client.py:75-78`):
> `session_id`: Target session ID. If not provided or empty, a new session is created automatically. Reuse the same session_id to continue the same bash session. Only API-level session state is preserved across calls. Note: `cd` or `export` inside a command do NOT affect subsequent calls.
>
> `exec_dir`: Working directory (absolute path). Takes effect on every call - if the session already exists, its default working directory is updated for subsequent calls. Use this instead of `cd` when later commands should run in a different directory.
>
> `env`: Extra environment variables to inject for this command only. Variables exported inside the command do not persist to later calls.

`bash.md:200-226` reinforces: a **new process is spawned per `exec`**. `cd`/`export` inside one command do not leak. To continue in a directory, pass `exec_dir` again. Each command has its own `command_id` (contrast with shell's session-level identity).

### Session lifecycle

- `SessionStatus` enum = `ready | closed` (`bash/raw_client.py:388-389`).
- `create_session` (`POST /v1/bash/sessions/create`): `session_id?`, `exec_dir?`, `snapshot_path?` (shell snapshot script sourced on init only; command-side env changes are NOT written back). (`bash/raw_client.py:421-441`)
- `close_session` (`POST /v1/bash/sessions/{session_id}/close`).
- `BashSessionInfo` { `session_id`, `status`, `working_dir`, `created_at`, `last_used_at`, `current_command?`, `command_count` }.

### stdin / kill

- `POST /v1/bash/write` { `session_id`, `input`, `command_id?` } → writes to the running process stdin pipe. For line-buffered programs include `\n`. Some REPLs write prompts to **stderr** — consumers must inspect both streams (`bash.md:198`).
- `POST /v1/bash/kill` { `session_id`, `signal?` } where signal ∈ `SIGTERM | SIGKILL | SIGINT`.

### Error-handling order (doc, `bash.md:254-281`)

HTTP success means "request accepted", not "command succeeded". Order: (1) HTTP status, (2) `success`, (3) `data.status` for lifecycle, (4) when `status=completed`, `exit_code` for success. Empty `stdout`/`stderr` does not mean the command did not run.

---

## 4. `shell` (PTY terminal, tag `shell`)

### HTTP routes

| Method | Path | Body | Response |
|---|---|---|---|
| POST | `/v1/shell/exec` | `ShellExecRequest` | `Response_ShellCommandResult_` |
| POST | `/v1/shell/view` | `ShellViewRequest` | `Response_ShellViewResult_` |
| POST | `/v1/shell/wait` | `ShellWaitRequest` | `Response_ShellWaitResult_` |
| POST | `/v1/shell/write` | `ShellWriteToProcessRequest` | `Response_ShellWriteResult_` |
| POST | `/v1/shell/kill` | `ShellKillProcessRequest` | `Response_ShellKillResult_` |
| POST | `/v1/shell/sessions/create` | `ShellCreateSessionRequest` | `Response_ShellCreateSessionResponse_` |
| POST | `/v1/shell/sessions/update` | `ShellUpdateSessionRequest` | `Response` |
| GET  | `/v1/shell/terminal-url` | – | `Response_str_` |
| GET  | `/v1/shell/sessions/stats` | – | `Response_ShellSessionStats_` |
| GET  | `/v1/shell/sessions` | – | `Response_ActiveShellSessionsResult_` |
| DELETE | `/v1/shell/sessions` | – | `Response` (cleanup all) |
| DELETE | `/v1/shell/sessions/{session_id}` | – | `Response` (cleanup one) |
| WS   | `/v1/shell/ws` | – | WebSocket terminal |

Source: `shell/raw_client.py:92,159,227,292,351,424,491,544,580,616,652,690`; WS at `shell.md:98`.

### `exec` request (`ShellExecRequest`, `shell/raw_client.py:33-46`)

`command:str` (req), `id:str?` (session id; auto-created if absent), `exec_dir:str?` (absolute), `async_mode:bool?`, `timeout:float?`, `strict:bool?`, `no_change_timeout:int?`, `hard_timeout:float?`, `preserve_symlinks:bool?`, `truncate:bool?`.

### Status enum (`BashCommandStatus`) — different from bash

```
running | completed | no_change_timeout | hard_timeout | terminated
```

Note: named `BashCommandStatus` in OpenAPI ("compatible with OpenHands"). `terminated` ≠ bash's `killed`; `no_change_timeout`/`hard_timeout` are explicit status values (bash folds these into `timed_out`).

### Key parameters (docstring quotes, `shell/raw_client.py:69-85`)

- `strict`: "If True, returns error when working directory does not exist. If False or None, silently falls back to session working directory."
- `no_change_timeout`: "If no output change is detected within this time, command returns with `NO_CHANGE_TIMEOUT` status. Overrides session-level setting for this command only." Default **120s** on session create.
- `hard_timeout`: "When reached, the command is forcefully stopped and current console output is returned with `HARD_TIMEOUT` status. Unlike `timeout` (which only affects HTTP response timing), this actually terminates the command."
- `preserve_symlinks`: "If True, `pwd` shows symlink path. If False, symlinks resolved to physical paths. Default False."
- `truncate`: "If True, truncate output when it exceeds 30000 characters (default: True)."

### SSE streaming

`exec` and `view` "Support SSE streaming if Accept header contains `text/event-stream'" (`shell/raw_client.py:48,143`). A reimplementation must accept both JSON and SSE on these two routes.

### Response payloads

- `ShellCommandResult` { `session_id:str`, `command:str`, `status:BashCommandStatus`, `output?` (only when completed), `console?` (command records), `exit_code?` (only when completed) }.
- `ShellViewResult` { `output:str`, `session_id:str`, `console?`, `status:BashCommandStatus`, `command?`, `exit_code?` } — terminal snapshot.
- `ShellWaitResult` { `status:BashCommandStatus` } — checks whether the current command is still running.
- `ShellWriteResult` { `status:BashCommandStatus` }.
- `ShellKillResult` { `status:BashCommandStatus`, `exit_code?`, `returncode?` (deprecated, use `exit_code`) }.
- `ShellCreateSessionResponse` { `session_id:str`, `working_dir:str` }.

### Session model vs bash

- `create_session`: `id?`, `exec_dir?`, `no_change_timeout?` (default 120), `preserve_symlinks?`. **"If id already exists, return the existing session"** (`shell/raw_client.py:401-402`) — idempotent create.
- `update_session` (`POST /v1/shell/sessions/update`): `id:str`, `no_change_timeout?:int` — only this field is updatable.
- `get_terminal_url` (`GET /v1/shell/terminal-url`): "Create a new shell session and return the terminal URL" (`shell/raw_client.py:531-533`) — returns `Response_str_` (the URL). Note: it creates a session as a side effect.
- `get_session_stats`: `ShellSessionStats` { `total_sessions`, `active_sessions` (used within last 5 min), `idle_sessions`, `max_sessions`, `session_timeout`, `usage_ratio` }.
- `list_sessions`: `ActiveShellSessionsResult` { `sessions`: map<id, info> }.

### Reuse semantics (different from bash)

Shell is PTY-based: within the same `id`, **working directory and environment variables ARE preserved** across execs (`shell.md:48-62`). This is the inverse of bash where each exec is a fresh process. Contrast table (`bash.md:7-14`, `shell.md:9-18`):

| | shell | bash |
|---|---|---|
| Backend | PTY | subprocess pipe |
| Output | one `output` field | separate `stdout`/`stderr` |
| Read model | `wait`+`view` snapshots | `/output` offset reads |
| Input | terminal input events | stdin pipe |
| Command identity | session-level | per-command `command_id` |

### WebSocket terminal (`/v1/shell/ws`)

Client→server messages: `{type:"input", data:"ls -la\n"}`, `{type:"resize", data:{cols,rows}}`, `{type:"pong", data:{timestamp}}`. Server→client: `{type:"output", data}`, `{type:"ping", timestamp|data}`. After connect the server returns a `session_id` for the active terminal. **"After connection failures, create a new terminal session instead of relying on reconnects for complete output history."** (`shell.md:121`). Built-in terminal UI at `/terminal` (`shell.md:91`).

---

## 5. `code` (unified entry, tag `code`)

### HTTP routes

| Method | Path | Body | Response |
|---|---|---|---|
| POST | `/v1/code/execute` | `CodeExecuteRequest` | `Response_CodeExecuteResponse_` |
| GET  | `/v1/code/info` | – | `Response_CodeInfoResponse_` |

### `execute` request (`code/raw_client.py:25-34`)

`language:Language` (req), `code:str` (req), `timeout:int?`, `cwd:str?`, `stateful:bool?`, `session_id:str?`.

`Language` enum = `python | javascript` (OpenAPI `Language` schema).

### Behavior

- "Run code through the unified runtime, dispatching to Python, Node.js, or future language executors" (`code/raw_client.py:37`).
- `stateful=True` → uses **Jupyter kernel** (for both python and javascript per docs); "variables and state persist across requests with the same `session_id`" (`code/raw_client.py:53-57`). `session_id` is "Required when stateful=True to maintain state across requests. Auto-generated if not provided."
- Python → Jupyter kernel; JavaScript → Node.js (`code.md:7-12`).

### Response (`CodeExecuteResponse`)

{ `language:Language`, `status:str` (execution status indicator), `code:str` (echo), `outputs?:array`, `stdout?`, `stderr?`, `exit_code?`, `traceback?`, `session_id?` (only when `stateful=True`) }.

### `info` (`CodeInfoResponse`)

{ `languages:CodeLanguageInfo[]` }. `CodeLanguageInfo` { `language`, `description`, `runtime_version?`, `default_timeout:int=30` (1–300), `max_timeout:int=300` (1–300), `details?` }. **"Version info is cached at service level (first call only runs subprocess)."** (`code/raw_client.py:113-117`).

---

## 6. `nodejs` (tag `nodejs`)

### HTTP routes

| Method | Path | Body / param | Response |
|---|---|---|---|
| POST | `/v1/nodejs/execute` | `NodeJSExecuteRequest` | `Response_NodeJSExecuteResponse_` |
| GET  | `/v1/nodejs/info` | – | `Response_NodeJSRuntimeInfo_` |
| POST | `/v1/nodejs/sessions` | `NodeJSCreateSessionRequest` + query `version` | `Response_NodeJSCreateSessionResponse_` |
| GET  | `/v1/nodejs/sessions` | query `version` | `Response_NodeJSSessionListResponse_` |
| GET  | `/v1/nodejs/sessions/{session_id}` | query `version` | `Response_NodeJSSessionResponse_` |
| PATCH | `/v1/nodejs/sessions/{session_id}` | `NodeJSUpdateSessionRequest` + query `version` | `Response_NodeJSUpdateSessionResponse_` |
| DELETE | `/v1/nodejs/sessions/{session_id}` | query `version` | `Response_NodeJSDeleteSessionResponse_` |

**Important**: `version` is a **query parameter** (not body) on all session routes and on execute. Values: `"node20" | "node22" | "node24"` or aliases `"20" | "22" | "24"` (`nodejs/raw_client.py:82-83`). Source: `nodejs/raw_client.py:92,156,198,269,340,401,470`.

### `execute` request (`NodeJSExecuteRequest`, `nodejs/raw_client.py:30-41`)

`code:str` (req), `timeout:int?`, `stdin:str?`, `files:Dict[str,str?]?` (additional files created in exec dir), `stateful:bool?`, `session_id:str?`, `cwd:str?`, `version:str?`.

### Stateful vs stateless (quote, `nodejs/raw_client.py:46-57`)

> For stateless execution (default):
> - Each request creates a fresh execution environment
> - Environment is cleaned up automatically after execution
>
> For stateful execution (stateful=True):
> - Uses persistent REPL session that maintains state between requests
> - Variables, functions, and imports persist across calls
> - Returns session_id to continue the session in subsequent requests
> - Supports async/await at top level

### Response (`NodeJSExecuteResponse`)

{ `language:str` (always `'javascript'`), `status:str` (`ok | error | timeout`), `execution_count?`, `outputs:NodeJSOutput[]`, `code:str`, `stdout:str`, `stderr:str`, `exit_code:int`, `session_id?` (use to continue stateful session) }.

`NodeJSOutput` { `output_type:str` (`stream | error | execute_result`), `name?` (stdout/stderr), `text?`, `ename?`, `evalue?`, `traceback?` } — Jupyter-style output records.

### Session lifecycle

- `create_session` (`POST /v1/nodejs/sessions`): `version?` (query), `session_id?`, `cwd?`, `max_idle_time?:int` (**default 24 hours**, in seconds per request schema; note `NodeJSSessionInfo.max_idle_time` is reported in **milliseconds**). Returns `NodeJSCreateSessionResponse` { `session_id`, `created:bool`, `message?`, `session?` }.
- `list_sessions`: `NodeJSSessionListResponse` { `sessions`: map<id, `NodeJSSessionInfo`> }. `NodeJSSessionInfo` { `session_id`, `cwd`, `created_at` (ms epoch), `last_used` (ms epoch), `max_idle_time` (ms), `age_seconds`, `state: "IDLE"|"EXECUTING"` }.
- `get_session` / `delete_session` / `update_session` (PATCH, body `max_idle_time?`, `cwd?`).
- `info` (`NodeJSRuntimeInfo`): `node_version`, `npm_version`, `supported_languages`, `description`, `runtime_directory?`, `global_npm_directory?`, `runtime_packages:NodeJSPackageInfo[]`, `global_packages:NodeJSPackageInfo[]`, `error?`, `available_versions[]` (e.g. node20/node22/node24), `current_version?`.

---

## 7. `jupyter` (tag `jupyter`)

### HTTP routes

| Method | Path | Body | Response |
|---|---|---|---|
| POST | `/v1/jupyter/execute` | `JupyterExecuteRequest` | `Response_JupyterExecuteResponse_` |
| GET  | `/v1/jupyter/info` | – | `Response_JupyterInfoResponse_` |
| GET  | `/v1/jupyter/sessions` | – | `Response_ActiveSessionsResult_` |
| DELETE | `/v1/jupyter/sessions` | – | `Response` (cleanup all) |
| DELETE | `/v1/jupyter/sessions/{session_id}` | – | `Response` (cleanup one) |
| POST | `/v1/jupyter/sessions/create` | `JupyterCreateSessionRequest` | `Response_JupyterCreateSessionResponse_` |

Source: `jupyter/raw_client.py:72,128,166,200,238,299`.

### `execute` request (`JupyterExecuteRequest`, `jupyter/raw_client.py:28-36`)

`code:str` (req), `timeout:int?`, `kernel_name:str?`, `session_id:str?`, `cwd:str?`.

- `kernel_name`: `'python3' | 'python3.10' | 'python3.11' | 'python3.12'`. **Defaults to the runtime Python version resolved from `PYTHON_VERSION`** (`jupyter/raw_client.py:43-56`).
- `session_id` maintains kernel state across requests.

### Session persistence & expiry (quote, `jupyter/raw_client.py:39-44`)

> Execute Python code using Jupyter kernel with session persistence … Use session_id to maintain variable state across multiple requests. **Sessions automatically expire after 30 minutes of inactivity.**

This 30-min auto-expiry is jupyter-specific (nodejs uses `max_idle_time`, default 24h; bash sessions are explicit `ready/closed`).

### Response (`JupyterExecuteResponse`)

{ `kernel_name:str`, `session_id?`, `status:str` (`ok | error | timeout`), `execution_count?`, `outputs:JupyterOutput[]`, `code:str`, `msg_id?` (Jupyter kernel message id) }.

`JupyterOutput` { `output_type:str` (`stream | execute_result | display_data | error`), `name?` (stdout/stderr), `text?`, `data?:object` (for execute_result/display_data, e.g. rich output/mime bundles), `metadata?:object` }. `execute_result`/`display_data`/`error` make this richer than nodejs's output set.

### Session management

- `create_session` (`POST /v1/jupyter/sessions/create`): `session_id?`, `kernel_name?`, `cwd?` → `JupyterCreateSessionResponse` { `session_id`, `kernel_name`, `message` }.
- `list_sessions`: `ActiveSessionsResult` { `sessions`: map<id, info> }.
- `delete_session` / `delete_sessions` (all).
- `info` (`JupyterInfoResponse`): { `default_kernel`, `available_kernels[]`, `active_sessions:int`, `session_timeout_seconds:int`, `max_sessions:int`, `description`, `kernel_detection:str` }.

### Non-obvious

- **No explicit create needed** to use stateful execution — calling `/execute` with a `session_id` auto-creates the session/kernel (`jupyter.md:16-33`). `create_session` is for pre-configuring cwd/kernel.
- Jupyter is the backend for `/v1/code/execute` with `stateful=True` for **both** python and javascript (per `code.md` routing table), so a reimplementation may share kernel plumbing between `code` and `jupyter`.

---

## Reimplementation checklist (critical, easy-to-miss items)

1. **Two error envelopes**: standard `{success,message,data}` for most APIs vs file-watch's plain resource JSON + HTTP codes vs file-download's binary stream + 409. File ops return **HTTP 200 with `success=false`** for fs errors (do not map these to 4xx/5xx).
2. **`OMIT` semantics**: optional params are *omitted* from the JSON body, not serialized as `null`. The SDK uses `...` as the OMIT sentinel.
3. **bash status enum** (`pending|running|completed|timed_out|killed`) ≠ **shell status enum** (`running|completed|no_change_timeout|hard_timeout|terminated`).
4. **bash**: new process per `exec`; `cd`/`export` do NOT persist; use `exec_dir` (updates session default) and `env` (per-command only). Poll with byte `offset`/`stderr_offset`.
5. **shell**: PTY; same `id` preserves cwd/env across execs. SSE on `exec`/`view` via `Accept: text/event-stream`. `terminal-url` GET creates a session as a side effect.
6. **shell WebSocket** `/v1/shell/ws` with input/resize/pong + output/ping messages; no reconnect history guarantee.
7. **nodejs `version`** is a **query param** on every session route and on execute (values `node20/22/24` or `20/22/24`). `max_idle_time` default 24h; reported in ms in `NodeJSSessionInfo`.
8. **jupyter** auto-expires sessions after **30 min** inactivity; `/execute` auto-creates a session from `session_id` (no need to call `create_session`).
9. **file-watch** has its own lifecycle (create→poll/events→stop) with `cursor`-based paging, `overflow` signal, and a one-shot `wait` that may return immediately if the file already exists and `create` is in `event_types`.
10. **`str_replace_editor`** is the Anthropic tool spec (`view|create|str_replace|insert|undo_edit`, `replace_mode` `ALL|FIRST|LAST`, plus PDF/Excel/PPTX range params).
11. **`/v1/code/info`** caches version info at service level (first call only runs subprocess).
12. All execution endpoints treat **HTTP 200 as "request accepted"**, not "command succeeded" — consumers must inspect `data.status` then `exit_code`.

## Caveats / Not Found

- The SDK `raw_client.py` files are Fern-generated and contain both sync and async duplicated classes; only the sync class was quoted (async is identical with `await`).
- The OpenAPI `Response_*` envelope schemas (`Response_BashExecResult_` etc.) were not fully dumped, but the inner result schemas (e.g. `BashExecResult`) were, and the docs confirm the `{success,message,data}` wrapper.
- `FileSearchResult`, `FileFindResult`, `FileReplaceResult` inner schemas were not individually dumped (only listed in the union type names); their shape follows the same `{success,message,data:<result|error>}` pattern. A reimplementation should confirm exact fields for `search`/`find`/`replace` from `openapi.json` if precise fields are needed.
- WebSocket sub-protocol details (auth, heartbeat interval) beyond the message shapes in `shell.md:97-119` were not found in the SDK raw clients; check the server implementation or `guide/advanced/web-terminal.md` for the full xterm.js integration if needed.
