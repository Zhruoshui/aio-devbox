# API Surface - AIO Sandbox (server contract)

Source: `sdk/js/src/api/resources/*` (Fern-generated, mirrors `sdk/python/agent_sandbox/*`).
This is the **contract** any replica server must implement. 20 resource groups, ~120 methods.

All routes are under the sandbox server (`SANDBOX_SRV_PORT` 8091, proxied via gateway 8080).
Route prefix per resource inferred from docs: `/v1/<resource>/*` (e.g. `/v1/bash/*`).

## Resources & methods

| Resource | Methods | What it is |
|----------|---------|------------|
| **auth** | `createTicket`, `authenticate` | JWT ticket-based auth |
| **sandbox** | `getContext`, `getPythonPackages`, `getNodejsPackages`, `listHooks`, `registerHook`, `removeHook`, `observeStart`, `observeStop`, `observeStatus`, `observeLive`, `observeExport`, `observeReports`, `observeReportDownload`, `observeReportDelete` | Sandbox metadata + lifecycle hooks + screen/session observation (recording) |
| **file** | `readFile`, `writeFile`, `listPath`, `findFiles`, `globFiles`, `grepFiles`, `searchInFile`, `replaceInFile`, `strReplaceEditor`, `downloadFile`, `uploadFile`, `watchCreate`, `watchList`, `watchEvents`, `watchPoll`, `watchStop`, `watchWait` | Full file ops incl. Aider/Cline-style `str_replace_editor` + file watching |
| **bash** | `createSession`, `exec`, `output`, `write`, `kill`, `closeSession`, `sessions` | Pipe-based non-interactive bash exec with sessions |
| **shell** | `createSession`, `execCommand`, `writeToProcess`, `killProcess`, `waitForProcess`, `getTerminalUrl`, `view`, `listSessions`, `updateSession`, `cleanupSession`, `cleanupAllSessions`, `getSessionStats` | Interactive PTY terminal (WebSocket at `/v1/shell/ws`) |
| **code** | `executeCode`, `getInfo` | Generic code execution |
| **nodejs** | `createSession`, `executeCode`, `deleteSession`, `getSession`, `listSessions`, `updateSession` | Node.js execution sessions |
| **jupyter** | `createSession`, `executeCode`, `deleteSession`, `deleteSessions`, `listSessions`, `getInfo` | Jupyter kernel sessions |
| **browser** | `executeAction`, `getInfo`, `screenshot`, `restart`, `setConfig`, `getProxyPac` | High-level browser control |
| **browserPage** | `navigate`, `back`, `forward`, `reload`, `click`, `hover`, `fill`, `fillForm`, `typeText`, `pressKey`, `hotKey`, `scroll`, `scrollTo`, `scrollToElement`, `selectOption`, `check`, `uncheck`, `uploadFile`, `getText`, `getHtml`, `getMarkdown`, `getElements`, `findText`, `getConsole`, `exportConsole`, `evaluate`, `screenshot`, `wait`, `record` | ~30 Playwright-like page operations |
| **browserTabs** | `create`, `list`, `activate`, `close` | Tab management |
| **browserCookies** | `getCookies`, `setCookies`, `clearCookies` | Cookie management |
| **browserNetwork** | `addRoute`, `removeRoute`, `getRequests`, `exportHar`, `setHeaders`, `setScopedHeaders` | Network interception / HAR export |
| **browserState** | `save`, `load` | Browser state persistence |
| **browserCaptcha** | `detect`, `wait` | Captcha detection |
| **mcp** | `listMcpServers`, `listMcpTools`, `executeMcpTool` | MCP hub client (aggregates browser/markitdown/chrome-devtools MCP servers) |
| **skills** | `registerSkills`, `listMetadata`, `getContent`, `deleteSkill`, `clearSkills` | Claude-Skills-style skill registry |
| **proxy** | `setUpstream`, `getUpstream`, `removeUpstream`, `addMapping`, `removeMapping`, `listMappings`, `addExclude`, `removeExclude`, `listExcludes`, `diagnose`, `health` | tinyproxy control (upstream/mapping/excludes) |
| **display** | `record` | Display/screen recording |
| **util** | `convertToMarkdown` | markitdown conversion |

## Notes for a reimplementation

- **SDK-first strategy**: the open SDKs are the spec. Rebuild the server to satisfy
  `agent_sandbox` (Python) / `@agent-infra/sandbox` (JS) unchanged, and the whole
  examples/eval ecosystem works against your replica for free.
- **Fern definition** (`sdk/fern/`) is the single source that generates both SDKs.
  Extracting its OpenAPI/IR would give a machine-readable contract (recommended next
  research step if proceeding).
- **Heaviest areas**: `browserPage` (~30 methods over CDP) and `file` (watching +
  `str_replace_editor`). These dominate implementation effort.
- **Request/response shapes**: live in `sdk/js/src/api/resources/*/client/requests/*.ts`
  and `sdk/python/agent_sandbox/*/types/`. These are the exact types to match.
