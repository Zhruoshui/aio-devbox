# AIO 接入点与用量数据源调研(容器实测)

> 2026-08-26 实测于 aio-app-1 容器 + 本仓库。供 task 08-26-unified-model-config 设计/实现引用。
> 注意:容器内 ~/.pi/agent/models.json 含真实 API key,任何文档/artifact 不得复制 key 本体。

## 1. 容器与 agent 现状

- app 容器用户 `gem`(uid 1000),workspace 卷挂 `/home/gem`;compose 未显式设 user(aio-app 以默认 root 跑?写 /home/gem 文件时须注意属主,落地时验证并以正确 uid 写或 chown gem:gem)。
- 已装 agent:`opencode`、`pi`(均在 /usr/local/bin,scenario 烘焙)。
- **claude code、codex 未安装**(scenarios/ 无对应目录,.aio/enabled.toml 无对应项)。本任务仍预写其配置文件,装上即用。
- 现有 pi 配置:`~/.pi/agent/models.json` 已有 2 个自定义 provider,同一网关 `https://ai.aruoshui.com/v1`,api 分别为 openai-responses / openai-completions(多协议网关实例);`~/.pi/agent/settings.json` 的 defaultProvider/defaultModel 指向其中之一;auth.json 存在。
- opencode 配置:`~/.config/opencode/opencode.jsonc` 仅含 `$schema` 一行;数据目录 `~/.local/share/opencode/`。

## 2. AIO 自身接入点

### 后端(Rust axum,app/)

- 路由模块:`app/src/routes/{buttons,manifest,seam,stats,terminal,pty?}.rs`,在 `routes/mod.rs` 声明;新增模型配置路由 = 新模块 + main.rs 挂载。
- 现有依赖(Cargo.toml):axum(ws)/tokio(full)/tracing/serde/serde_json/toml/tower-http(fs)/portable-pty/futures-util/nix(fs)。**缺 HTTP 客户端与 SQLite**:
  - 探测/拉模型 → 需加 `reqwest`(建议 rustls-tls feature,避免 openssl 链接问题;容器出网走代理,reqwest 默认读 env 代理)。
  - 读 opencode.db → 需加 `rusqlite`(bundled feature 自带 SQLite,免系统依赖),只读打开。
- 状态:`app/src/state.rs` 管理会话类状态;模型配置路由多为无状态文件 IO + 共享 reqwest Client + 进程内互斥(配置写串行化)。
- services.toml(内置按钮,include_str! 编进 manifest):现有 type = `web`(iframe) | `agent`(xterm/pty)。**原生页面需第三种 pane 类型**(如 `page`),`enabled` 恒 true(由 app 自身提供)。
- entrypoint.sh 已自启 pi-web(30141);本任务不动它。

### 前端(React SPA,web/src/)

- `App.tsx` 流程:fetch `/api/manifest` → Sidebar 按钮 → golden-layout pane 实例;`PaneForService`(App.tsx:523-526)按 type 分派 `web→IframePane`、其余 `→XtermPane`;`readServiceState`/`isServiceEntry`(App.tsx:533-547)是 manifest 契约的唯一解码点(新增 type 要同步)。
- 新页面 = `web/src/panes/ModelsPane.tsx` + App.tsx 分派分支 + services.toml 条目 + `types.ts`、`Sidebar.tsx`、`icons.tsx`(serviceIcon)、`i18n.ts` 补词条。
- 既有 fetch 范式参考 `useStats.ts`(轮询 /api/stats)。
- UI 语言:zh-CN 为主(i18n.ts 有 zh/en 双语)。

## 3. 用量统计数据源(已验证字段)

### pi:`~/.pi/agent/sessions/<sanitized-cwd>--/<ts>_<uuid>.jsonl`

- 目录名 = cwd 路径 sanitize + `--` 后缀(如 `--home-gem-pi-cwd-20260825--`)。
- 每条 assistant/tool 消息行含 `"usage":{"input":N,"output":N,"cacheRead":N,"cacheWrite":N,"totalTokens":N,"cost":{"input":…,"output":…,…,"total":…}}` 与 `"model":"<modelId>"`(实测样例 model=deepseek-v4-flash / qwen3.8-max;免费中转曾出现全 0 usage)。
- 聚合 = 按 model 求和 input/output/cacheRead/cacheWrite;cost.total 可选展示。

### opencode:`~/.local/share/opencode/opencode.db`(SQLite,WAL)

- 表:workspace/project/**message**/**part**/session/…;`message.data` 为 JSON:
  - assistant 行:`{"role":"assistant","modelID":"mimo-v2.5-free","providerID":"opencode","tokens":{"input":N,"output":N,"reasoning":N,"cache":{"read":N,"write":N}},"cost":N,"time":{"created":ms,"completed":ms}}`(实测样例)。
  - user 行含 `model:{providerID,modelID}`。
- 聚合 = `SELECT data FROM message` 过滤 role=assistant,按 providerID+modelID 求和;时间过滤用 time.created(ms)。只读打开(避免锁 WAL)。

### claude code(未装;装后路径)

- `~/.claude/projects/<proj-slug>/*.jsonl`:assistant 消息行 `message.model` + `message.usage{input_tokens,output_tokens,cache_creation_input_tokens,cache_read_input_tokens}`;时间戳在行级 `timestamp`(ISO)。

### codex(未装;装后路径)

- `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`:`token_count` 事件 `info.total_token_usage{input_tokens,cached_input_tokens,output_tokens}`;模型名在 TurnContext 记录(同会话单模型)。

## 4. 网络与安全约束

- 容器出网经沙箱代理;`ai.aruoshui.com` 已在允许域(pi 正在使用)。models.dev 目录可能不可达 → 元数据补全做可选增强、失败静默。
- key 呈现:GET 类接口绝不回显完整 key(cc-switch/pi-web 均 mask);写入 0600 原子。
- 本页面与 pi-web 共同编辑 ~/.pi/agent/models.json:双方都是键级合并 + 原子写,共存安全;不做互斥跨进程锁(MVP 接受 last-writer-wins,文件级)。
