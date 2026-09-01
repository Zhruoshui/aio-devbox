# 技术设计:统一模型配置页

> task 08-26-unified-model-config。需求依据见 prd.md(D1 范式、R1-R5);参考实现细节见 research/ 三份调研。

## 1. 架构总览

```
React ModelsPane(web/src/panes/ModelsPane.tsx,新 pane 类型 "page")
   │  /api/models/*(fetch)
   ▼
axum 路由模块 app/src/routes/models/(新,mod.rs 挂载)
   ├─ store.rs     canonical 存储 ~/.aio/models.json(0600 原子写)
   ├─ discover.rs  /v1/models 拉取(reqwest,协议自适应 URL/头)
   ├─ test.rs      最小补全探测(reqwest)
   ├─ render/      四个渲染器 + apply 协调(备份→合并→原子写)
   │    ├─ pi.rs ├─ opencode.rs ├─ claude.rs └─ codex.rs
   └─ usage.rs    用量扫描(pi jsonl / opencode sqlite / claude、codex jsonl)
```

- **进程模型**:全部能力进现有 aio-app 进程(以 gem 运行,写 /home/gem 下文件天然属主正确,实测 PID 1 = aio-app as gem)。不新增容器/服务/端口。
- **新增依赖**:`reqwest`(default-features off + json + rustls-tls,规避 openssl 链接;容器出网走代理,reqwest 默认读 env 代理)、`rusqlite`(bundled,只读打开 opencode.db)。`json5`(解析 opencode.jsonc 容错)。
- **manifest 接入**:services.toml 新增 `[[service]] id="modelsConfig" type="page"`。manifest 路由对该 type `enabled` 恒 true(能力由 app 自身提供,无需探测)。前端 `PaneForService` 加 `type==="page"` 分支。

## 2. Canonical 数据模型(~/.aio/models.json)

pi 的 models.json schema 超集(见 research/pi-web §1),扩展协议声明与 per-agent 分配:

```jsonc
{
  "version": 1,
  "providers": {
    "<providerId>": {              // kebab-case,用户命名,同时用作 pi/opencode 的 provider 键
      "name": "aruoshui-octopus",  // 显示名
      "baseUrl": "https://ai.aruoshui.com/v1",
      "api": "openai-completions", // 主协议:openai-completions|openai-responses|anthropic-messages
      "apiKey": "sk-…",            // 机密;存储 0600;GET 接口打码
      "headers": {},               // 可选附加请求头
      "compat": {},                // 可选(pi 兼容开关透传)
      "anthropic": null,           // 可选 { "baseUrl": "https://…" } —— 声明 anthropic-messages 能力;
                                   //   缺省 baseUrl = baseUrl。存在才允许分配给 claude
      "models": [
        { "id": "deepseek-v4-pro", "name": "DeepSeek V4 Pro", "reasoning": true,
          "input": ["text"], "contextWindow": 1000000, "maxTokens": 384000,
          "cost": { "input": 0.435, "output": 0.87, "cacheRead": 0.003625, "cacheWrite": 0 } }
      ]
    }
  },
  "agents": {
    "pi":       { "provider": "<providerId>", "model": "<modelId>" },
    "opencode": { "provider": "<providerId>", "model": "<modelId>" },
    "claude":   { "provider": "<providerId>", "model": "<modelId>",
                  "haikuModel": null, "sonnetModel": null, "opusModel": null,   // ANTHROPIC_DEFAULT_*_MODEL 覆盖(null=同主模型)
                  "authField": "AUTH_TOKEN" },                                   // AUTH_TOKEN | API_KEY
    "codex":    { "provider": "<providerId>", "model": "<modelId>",
                  "reasoningEffort": null,   // null|low|medium|high
                  "wireApi": "responses" }   // responses|chat(默认由主协议推导)
  }
}
```

### 协议兼容矩阵(分配时的过滤规则)

| agent | 要求 | 来源 |
|---|---|---|
| pi | 任意(直接用 api) | provider.api |
| opencode | 任意(npm 包由 api 推导:anthropic-messages→`@ai-sdk/anthropic`,其余→`@ai-sdk/openai-compatible`) | provider.api |
| claude | `api=="anthropic-messages"` **或** `anthropic` 块存在 | provider.anthropic.baseUrl ?? provider.baseUrl |
| codex | 非 anthropic;wireApi = api=="openai-responses" ? "responses" : "chat" | provider.baseUrl(cc-switch 规则:origin-only 且不以 /v1 结尾才补 /v1) |

不兼容的 provider 在对应 agent 页签的下拉中禁用并标注原因。

### key 打码约定

- `GET /api/models/config` 将每个 apiKey 替换为掩码 `"sk-****<末4位>"`。
- `PUT` 时空串 = 清除;**掩码原样回传 = 保留原值**(服务端检测到回传值与掩码一致则不覆盖)。UI 用 password input + "留空保持不变"占位。
- 同规则适用于 `POST discover` / `POST test` 的请求与响应(响应不含 key)。

## 3. API 契约(全部挂在现有 gateway 之后,无新增端口)

| 路由 | 方法 | 请求 | 响应 |
|---|---|---|---|
| `/api/models/config` | GET | — | canonical 全量(apiKey 打码) |
| | PUT | canonical 全量 | `{ok, warnings[]}`;掩码合并、校验(providerId/modelId 合法性)、原子写 |
| `/api/models/discover` | POST | `{baseUrl, api, apiKey?}`(key 掩码语义同上;或 `{providerId}` 直接用库内值) | `{models:[{id,name?}], endpoint}`;失败 502 + 上游 body 截断 500 字符 |
| `/api/models/test` | POST | `{providerId, modelId, protocol?}`(protocol 缺省 = provider.api) | `{ok, latencyMs, status?, error?, responseText?}`(截断 300) |
| `/api/models/agents` | GET | — | `{pi:{installed, liveProvider?, liveModel?}, …}` per-agent:installed=command_exists;live* 为当前配置文件回读 |
| `/api/models/apply/{agent}` | POST | —(用 canonical 中该 agent 的分配) | `{ok, written:[{path, backup}], errors[]}` |
| `/api/models/usage` | GET | `?window=today\|7d\|all` | `{rows:[{agent, provider, model, in, out, cacheRead, cacheWrite, cost?}], generatedAt}` |

实现约束:所有配置写路径经进程内 `tokio::sync::Mutex`(串行化);文件写 = 临时文件 + rename(原子),目标 0600,parents 0700。

## 4. 渲染器(apply)细节

通用流程:读 canonical → 该 agent 分配 → 备份目标文件(`.aio-bak-<ISO时间>` 滚动保留最近 3 份)→ 合并渲染 → 原子写 → 回读校验。

### pi(`render/pi.rs`)
- `~/.pi/agent/models.json`:读(缺失=`{}`)→ `providers.<providerId>` 整节点替换为 pi ProviderEntry(baseUrl/api/apiKey/headers/compat/models+cost)→ **其他 provider 节点原样保留**(pi-web 共存)。
- `~/.pi/agent/settings.json`:仅置 `defaultProvider`/`defaultModel` 两键,其余保留。
- 生效语义:pi 新会话加载 models.json,无需重启进程。

### opencode(`render/opencode.rs`)
- `~/.config/opencode/opencode.jsonc`:json5 容错读(注释在重写时丢失,备份可回溯,现文件仅 $schema 行)→ 置 `provider.<providerId> = {npm, name, options:{baseURL, apiKey}, models:{<id>:{name}}}` + 顶层 `model = "<providerId>/<modelId>"` → 其余键保留 → 写为紧凑 JSON。

### claude(`render/claude.rs`,未安装也可写)
- `~/.claude/settings.json`:读(缺失=`{}`)→ 仅置 `env.ANTHROPIC_BASE_URL`(anthropic baseUrl)、`env.ANTHROPIC_AUTH_TOKEN|ANTHROPIC_API_KEY`(authField)、`env.ANTHROPIC_MODEL`、可选 `env.ANTHROPIC_DEFAULT_{HAIKU,SONNET,OPUS}_MODEL` → **其余键(permissions/hooks 等)一律保留**(不做 cc-switch 的整文件快照/backfill——canonical 是 SSOT,键级合并更安全)。

### codex(`render/codex.rs`,未安装也可写)
- `~/.codex/auth.json`:读 → 仅置 `OPENAI_API_KEY` → 保留其余。
- `~/.codex/config.toml`:toml 解析(缺失=空)→ 置顶层 `model_provider="aio"`、`model`、可选 `model_reasoning_effort` → 置 `[model_providers.aio] {name, base_url(/v1 归一), wire_api, requires_openai_auth=true}` → 其余保留。写前 toml 序列化自校验;auth.json 先写,config.toml 失败则回滚 auth(还原备份)。

## 5. 发现与探测(复刻 pi-web + cc-switch 增强)

- **URL 推导**(按 api):openai 系 → baseUrl 不以 `/models` 结尾则追加 `/models`;anthropic → 追加 `/v1/models?limit=1000`;google 系(如后续支持)→ `/v1beta/models?pageSize=1000`。
- **多候选回退**(cc-switch):首选失败依次试 `/v1/models`、`/models`、剥 anthropic 系后缀(`/anthropic`、`/claude`、`/api/coding`)重拼。首个 2xx 即用。
- **头**:openai → `Authorization: Bearer`;anthropic → `x-api-key` + `anthropic-version: 2023-06-01`;均带 `Accept: application/json` + provider.headers。
- **响应解析**:裸数组 / `{data|models|results|items}` / 对象之对象;条目 string 或 `{id|model|name, display_name|displayName}`;剥 `models/` 前缀;去重自然排序。
- **test**:对目标协议端点发最小补全(`max_tokens:16`,消息 "Reply with OK only.",无重试,20s 超时)。anthropic 协议用 `/v1/messages` + anthropic 头;openai 用 `/chat/completions`(responses 协议用 `/responses`)。成功 = 2xx 且非 error stop。
- 超时统一 20s(discover/test),reqwest Client 进程内复用。
- models.dev 目录元数据补全:**本期不做**(离线可达性不确定,PRD 列为可选增强)。

## 6. 用量统计(usage.rs)

- **pi**:遍历 `~/.pi/agent/sessions/*/*.jsonl`,流式逐行;行 JSON 含顶层 `usage` + `model` 即累计(input/output/cacheRead/cacheWrite,cost.total 可选)。时间:行内 timestamp,缺失回退文件 mtime。
- **opencode**:rusqlite 只读打开(`file:…?mode=ro`,WAL 安全),`SELECT data FROM message`,过滤 `role=="assistant"`,按 `providerID`+`modelID` 聚合 `tokens`(input/output/cache.read/cache.write),cost 取行内 `cost`,`time.created` 做窗口过滤。
- **claude**(装后自动生效):`~/.claude/projects/**/*.jsonl`,`message.usage{input_tokens,output_tokens,cache_creation_input_tokens,cache_read_input_tokens}` + `message.model`,行级 `timestamp`。
- **codex**(装后自动生效):`~/.codex/sessions/**/*.jsonl` `token_count` 事件 `total_token_usage`,模型名取同文件最近 TurnContext。
- 窗口:today(本地零点)/7d/all。缓存:进程内 30s TTL(按 window 键);每次全量扫描(个人规模数据量,实现从简)。
- 未安装/无数据源 → 该 agent 不出现在 rows,页面显示提示。

## 7. 前端(ModelsPane.tsx)

- 页签:`供应商库` | `pi` | `opencode` | `Claude` | `Codex` | `用量统计`;agent 页签带 installed 徽标(未安装黄色横幅"配置将预写,安装后生效")。
- 供应商库:左 provider 列表(新增/删除);右详情:名称/baseUrl/api 下拉/anthropic 块(开关+可选 baseUrl)/apiKey(密码框,占位"留空保持不变")/headers+compat 折叠高级;模型表(行:模型测试按钮+状态胶囊绿✓ms/红✗/灰 testing、删除);「从端点拉取模型」打开搜索勾选弹层(discover 结果,"全选本页/加入所选")。
- agent 页签:provider 下拉(不兼容项禁用+原因 tooltip)、model 下拉、覆盖项(claude 三档+authField;codex effort+wireApi)、「生效」按钮 → apply → 展示 written/backup 清单 + live 回读对比(`/api/models/agents`)。
- 用量统计:窗口切换 + 表格 + 合计行 + 手动刷新(进入页签自动拉一次)。
- 接入点改动:`types.ts`(ServiceEntry.type 加 "page")、`App.tsx`(isServiceEntry + PaneForService 分支)、`services.toml` 条目、`icons.tsx`(serviceIcon)、`i18n.ts`(zh/en 词条)、`styles.css`(Kumo 风格)。
- 数据流遵循 cross-layer 约定:manifest 契约单一解码点 `readServiceState`;ModelsPane 的 API 响应在 pane 内一处解码为 TS 类型。

## 8. 兼容与迁移

- **首启导入**:canonical 缺失且 pi models.json 存在时,供应商库页显示「从 pi 导入」按钮(provider 1:1 映射,api=其 api,anthropic 块在 api=anthropic-messages 时置空对象);不自动写,用户确认后入库。
- **pi-web 共存**:双方都是"读-合并-原子写";无跨进程锁,接受极小 last-writer-wins 窗口(个人使用);apply 后的回读校验可提示外部变化。
- **回滚**:每目标文件滚动 `.aio-bak-*`×3;UI 展示备份路径;canonical 自身首次写入前若存在旧版先备份。整特性是纯增量(新 pane + 新路由),revert 提交即移除,无数据迁移负担。

## 9. 主要取舍记录

1. canonical JSON 单文件 vs SQLite:选 JSON(单用户、体量小、可 diff、人工可救)。
2. 键级合并 vs cc-switch 整文件快照+backfill:选键级合并(canonical 是 SSOT,少一套快照漂移机制;用户键永不丢失)。
3. reqwest+rustls vs 手写 hyper:选 reqwest(代理 env、超时、JSON 一等支持),代价是依赖体积。
4. rusqlite bundled vs 旁路 sqlite3/python:选 bundled(构建期自带,无运行时外部依赖)。
5. GET 打码+掩码回传 vs 明文返回:选打码(AIO 网关无鉴权、LAN 可达;终端面板本已等同 root,打码属低成本纵深)。
6. opencode.jsonc 注释丢失:接受(备份保留;现文件无实质注释)。

## 10. 测试策略

- Rust 单元测试:URL 推导与多候选表驱动;models 响应解析多形态;canonical 掩码往返;四个渲染器 golden-file(fixture 假 HOME:既有用户键保留、合并正确、备份产生);usage 聚合(fixture jsonl + 内存 sqlite)。
- 前端:`npm run build` + 既有 smoke-test.cjs 模式扩一条(页面元素存在性)。
- 集成验收:按 prd.md AC1-AC7 在重建容器内走查(注意 [[compose-up-build-skips-running]] 教训:改 Dockerfile/app 后须显式 build + --force-recreate)。
