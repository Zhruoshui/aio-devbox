# 统一模型配置页:四 agent 聚合配置 + 模型探测 + 用量统计

## 目标

AIO web UI 新增一个统一的模型配置页:聚合管理供应商(endpoint / API Key / 模型列表),
一次配置即可向 claude code / codex / opencode / pi 四家 agent 的原生配置文件生效;
配置时支持从 endpoint 自动拉取模型列表、对配置好的模型做可用性检测;
并在同一界面提供按模型聚合的 token 用量统计。

参考实现:pi-web(github.com/agegr/pi-web)的 ModelsConfig 面板、cc-switch(github.com/farion1231/cc-switch)的供应商管理 UX。

## 背景与现状(已确认事实)

### AIO 自身架构
- 后端:Rust axum(`app/src/routes/{buttons,manifest,seam,stats,terminal,pty}.rs`),静态服务 `web/dist`。
- 前端:React SPA(`web/src/App.tsx`、`Sidebar.tsx`、`panes/{IframePane,XtermPane}.tsx`、`useStats.ts`)。
- 按钮/面板注册:`app/services.toml`(内置,include_str! 编译进 manifest)+ `~/.aio/buttons.toml`(用户自注册)。
- 现有依赖(Cargo.toml):axum/tokio/serde/serde_json/toml/tower-http/portable-pty/nix。**无 HTTP 客户端、无 SQLite 库**——探测/拉模型需加 reqwest,读 opencode.db 需 rusqlite 或旁路。
- app 容器用户为 gem(uid 1000),workspace 卷挂 `/home/gem`;pi-web 由 entrypoint 自启(30141)。

### 容器内 agent 现状
- 已安装:opencode(`/usr/local/bin/opencode`)、pi(`/usr/local/bin/pi`)。
- **claude code、codex 未安装**(scenarios/ 无对应 scenario)。本任务仍需具备其配置生成能力:预写配置文件,agent 安装后即用。安装 scenario 由后续任务处理。

### 四家 agent 的配置文件与渲染目标(核心证据)

| agent | 配置文件 | 写入模式 | 内容形状 |
|---|---|---|---|
| claude code | `~/.claude/settings.json` | 整文件写(切换式) | `{env:{ANTHROPIC_BASE_URL, ANTHROPIC_AUTH_TOKEN, ANTHROPIC_MODEL, ANTHROPIC_DEFAULT_{HAIKU,SONNET,OPUS}_MODEL}}`(cc-switch `provider.rs:781-790`) |
| codex | `~/.codex/config.toml` + `~/.codex/auth.json` | 整文件写(切换式) | toml: `model_provider="custom"`、`model="…"`、`[model_providers.custom]{name,base_url,wire_api="responses",requires_openai_auth=true}`;auth.json: `{OPENAI_API_KEY}`(cc-switch `codexTemplates.ts:13-31`) |
| opencode | `~/.config/opencode/opencode.jsonc` | 读-改-合并(增量式) | `provider.<id> = {npm:"@ai-sdk/openai-compatible", name, options:{baseURL,apiKey}, models:{<id>:{name}}}`,保留其他用户键(cc-switch `opencode_config.rs:157-180`,互斥锁串行写) |
| pi | `~/.pi/agent/models.json` + `~/.pi/agent/settings.json` | 读-改-合并(增量式) | `providers.<key> = {baseUrl, api, apiKey, headers, compat, models:[…]}`;`settings.json` 的 `defaultProvider`/`defaultModel` 指定当前模型(pi-web schema,`components/ModelsConfig.tsx:150-176`) |

- pi 的模型 schema 最丰富(`id,name,reasoning,input,contextWindow,maxTokens,cost:{input,output,cacheRead,cacheWrite}`),pi-web 与 cc-switch 都向它对齐 → canonical 格式采用它的超集。
- 现有 pi 配置已含 2 个自定义 provider(指向 `https://ai.aruoshui.com/v1`,openai-completions / openai-responses 双协议;key 已存在文件中,本任务不得把 key 写进任何文档/artifact)。
- 协议约束:claude code 只认 anthropic-messages 端点;codex 需 wire_api(responses/chat);opencode 靠 npm SDK 包选择;pi 靠 `api` 字段。同一供应商的多协议能力(如 octopus/new-api 类网关同域名暴露 /v1/chat/completions 与 /v1/messages)需在 canonical 层显式表达。

### pi-web 三大能力的实现(待复刻)
- **模型配置 UI**:左树(provider→model)+ 右详情;`GET/PUT /api/models-config` 整体读写 models.json,原子写 0600。
- **模型列表自动获取**:`POST /api/models-config/discover` —— 由 baseUrl+api 推导列表 URL(openai → `<base>/models`;anthropic → 补 `/v1/models?limit=1000` + `x-api-key`/`anthropic-version` 头;google → `/v1beta/models?pageSize=1000` + `x-goog-api-key`;其余 Bearer),20s 超时,解析 `{data|models|results|items[]}` 多形态、剥 `models/` 前缀、去重。另有 models.dev 目录(`https://models.dev/api.json`,1h 缓存)自动补全 name/contextWindow/cost 等元数据(可选增强,离线环境可能不可达)。
- **可用性检测**:`POST /api/models-config/test` —— 真实补全请求("Reply with OK only.",maxTokens 16,无重试,20s 超时),返回 `{ok, latencyMs, status, responseText}`;UI 呈现为绿/红状态胶囊。
- cc-switch 补充:多候选 URL 重试(`/v1/models`、`/models`、剥 anthropic 后缀重拼,`model_fetch.rs:207-263`);连通性检测返回 `{status,success,message,responseTimeMs,httpStatus}`;切换前 backfill(把 live 文件当前内容存回旧 provider 快照,防止用户在 agent 侧的手改丢失);codex 双文件写入失败回滚。

### token 用量统计的数据源(全部本地、已验证)
| agent | 位置 | 字段 |
|---|---|---|
| pi | `~/.pi/agent/sessions/<sanitized-cwd>--/*.jsonl` | 每条 assistant/tool 消息 `usage:{input,output,cacheRead,cacheWrite,cost:{…}}` + `model` |
| opencode | `~/.local/share/opencode/opencode.db`(SQLite)`message` 表 | assistant 行 `data` JSON:`tokens:{input,output,reasoning,cache:{read,write}}`、`modelID`、`providerID`、`cost`、`time.created`(ms) |
| claude code(装后) | `~/.claude/projects/**/*.jsonl` | assistant 消息 `message.usage{input_tokens,output_tokens,cache_creation_input_tokens,cache_read_input_tokens}` + `message.model` |
| codex(装后) | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` | `token_count` 事件 `total_token_usage{input_tokens,cached_input_tokens,output_tokens}`;模型名在 TurnContext 记录 |

## 已拍板决策

- **D1 交互范式(2026-08-26 用户确认)**:统一供应商库 + 每 agent 分配生效。一份全局供应商库(endpoint/key/协议/模型列表,可视化增删改查+检测);每 agent 一个页签,选择当前 provider+model 及少量 agent 特有覆盖项(claude 三档小模型映射、codex reasoning effort 等);"生效"= 渲染写入该 agent 原生配置。改 key 一处改、处处生效。不采用 cc-switch 式每 agent 独立预设库,也不做快照漂移模型。

## 需求

- **R1 聚合配置管理**:统一供应商库(endpoint、协议、API Key、模型列表、可选 headers/成本),可视化增删改查;API Key 等敏感字段以 0600 权限原子写。
- **R2 四 agent 生效**:按各家写入模式渲染(claude/codex 切换式整写,opencode/pi 增量合并保留用户其他键);每 agent 可独立选择当前 provider+model(含 agent 特有覆盖项);claude/codex 未安装时仍预写配置并明确标注状态。
- **R3 模型列表自动获取**:编辑供应商时从 endpoint 拉取 `/v1/models`(协议自适应头与 URL 推导、多候选回退、超时控制),勾选并入模型列表。
- **R4 可用性检测**:对选定模型发起最小补全探测,呈现 ok/延迟/HTTP 状态;支持批量检测。
- **R5 token 用量统计**:聚合本地会话日志,按 agent×model 展示 input/output/cache token,顶部时间窗口切换(今日/7天/全部);tokens 为主口径,成本为可选列——仅当数据源自带(pi 日志内嵌 cost、opencode 的 cost 字段)时显示,claude/codex 暂留空(2026-08-26 用户确认)。

## 验收标准(草稿,随问题收敛细化)

- AC1:在统一页面创建一个供应商(真实 endpoint+key),拉取模型列表成功并勾选入库。
- AC2:对入库模型执行可用性检测,能看到延迟与 HTTP 状态;不可用时呈现明确错误。
- AC3:选择 pi 生效后,`~/.pi/agent/models.json` 出现该 provider 节点且 `settings.json` 的 defaultProvider/defaultModel 更新;pi 新会话可直接使用;已有 pi-web 手工配置的其他 provider 不被破坏。
- AC4:opencode 生效后,`opencode.jsonc` 的 `provider.<id>` 片段合并写入,原有其他键保留;opencode 新会话可选用该模型。
- AC5:claude/codex 生效后,`~/.claude/settings.json` / `~/.codex/{config.toml,auth.json}` 按上述形状生成;UI 标注"未安装,已预置"。
- AC6:用量统计页能看到 pi 与 opencode 的按模型 token 聚合,与手工抽验的会话日志数值一致。
- AC7:全程不因渲染/合并破坏用户在 agent 配置文件里的既有无关内容(原子写 + 合并 + 备份)。

## 暂缓范围(Out of Scope)

- claude code / codex 的安装 scenario(后续任务)。
- 模型成本(费用)统计——canonical 已含 cost 字段,展示与否见开放问题。
- OTEL/实时上报类方案;多用户/权限体系。
- 替换或魔改 pi-web 本身(它与本页面共存,共同编辑同一 models.json)。

## 开放问题(阻塞规划)

无——产品口径已全部拍板(D1 交互范式、R5 统计口径),剩余为 design 阶段的技术表达(canonical 多协议字段形状、Rust 端 HTTP/SQLite 依赖引入方式等),不阻塞规划。
