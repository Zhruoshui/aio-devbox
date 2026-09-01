# pi-web 模型配置实现调研

> 源码:/tmp/pi-web-src(github.com/agegr/pi-web,Next.js 应用,SDK 进程内集成)。
> 调研日期:2026-08-26。本文件是统一模型配置页(task 08-26-unified-model-config)的参考实现依据。

## 架构要点

- pi-web 服务端**不 spawn pi CLI**,而是 import pi SDK(`@earendil-works/pi-coding-agent` 等)在 Next.js Node runtime 内跑 AgentSession。
- 配置在磁盘:`~/.pi/agent/` 下,三个文件:
  - `models.json` —— 模型可视化配置(providers/models/costs/headers/baseUrl)
  - `auth.json` —— 凭据(API key + OAuth),0o600 + proper-lockfile 锁
  - `settings.json`(SDK settings manager)—— defaultProvider/defaultModel 等

## 1. 模型可视化配置(models.json)

### schema(verbatim,`components/ModelsConfig.tsx:150-176`)

```ts
interface ModelEntry {
  id: string;
  name?: string;
  api?: string;                 // "openai-completions" | "anthropic-messages" | "google-generative-ai" | "openai-responses"
  reasoning?: boolean;
  thinkingLevelMap?: Record<string, string | null>;
  input?: string[];
  contextWindow?: number;
  maxTokens?: number;
  cost?: { input?: number; output?: number; cacheRead?: number; cacheWrite?: number; tiers?: unknown };
  headers?: Record<string, string>;
  compat?: Record<string, unknown>;
}

interface ProviderEntry {
  baseUrl?: string;
  api?: string;
  apiKey?: string;              // 仅自定义 provider 内联;托管 provider 用 auth.json
  headers?: Record<string, string>;
  compat?: Record<string, unknown>;
  models?: ModelEntry[];
  modelOverrides?: Record<string, unknown>;
}

interface ModelsJson { providers?: Record<string, ProviderEntry>; }
```

- cost 单位:USD/token。
- 存取(`lib/models-config-store.ts`):`readModelsConfig()` 缺失/损坏返回 `{providers:{}}`;`writeModelsConfig()` 先 `normalizeModelsConfigCosts`(缺 cost 键补 0)+ `sanitizeModelsConfig`(丢弃无 id 的 model),再 `writePrivateFileAtomicSync`(原子写 0600)并 `invalidateModelsCache()`。

### API 路由(`app/api/models-config/route.ts`)

- `GET /api/models-config` → 原样返回 models.json。
- `PUT /api/models-config` body = 整个 ModelsJson → 写盘 → `{success:true}` / 500。
- **无重启**:写原子 + 进程内缓存失效(`lib/models-cache.ts:40-45`,代计数 + 每 cwd 缓存 TTL 60s)。

### 前端流程(`components/ModelsConfig.tsx`)

- 挂载时 GET → `setConfig`;左树(活跃 OAuth provider / 活跃 API-key provider / 自定义 provider→model 可展开);局部状态编辑(`updateProvider`/`updateModel`/`addDiscoveredModels`/`removeModel`/`renameProvider`,行 2011-2092);`handleSave`(行 2094-2112)整体 PUT。面板副标题直接显示 `~/.pi/agent/models.json`。
- provider 图标:`@lobehub/icons` 硬编码映射(anthropic/openai/google/deepseek/groq/mistral/moonshot/qwen/zhipu/kimi/grok…,行 74-116)。

### 托管 provider 凭据(auth.json,独立于 models.json)

- `app/api/auth/api-key/[provider]/route.ts`:GET 只回状态(configured/source/models,绝不回 key);POST `{apiKey}` 走 `provider.auth.apiKey.login` 但直接 `storeProviderCredential`(避开无限网络目录刷新挂死请求),随后 `invalidateModelsCache()`;DELETE `removeStoredCredentialIfType`(OAuth 时 409 拒绝,防类型错删)。
- `lib/provider-credential-store.ts`:auth.json 形如 `{ "<providerId>": <Credential> }`,proper-lockfile(10 次重试、30s stale)与 CLI 的 AuthStorage 共用同一把锁,0600/目录 0700。
- OAuth:`app/api/auth/login/[provider]/route.ts` SSE 流(auth_url/device_code/进度),手动码经内存 `__piLoginCallbacks` token 往返。
- provider 列表按能力枚举(`lib/provider-listing.ts`),不硬编码。

## 2. 模型列表自动获取(发现)

### A. 直连 provider `/v1/models` —— `POST /api/models-config/discover`

`app/api/models-config/discover/route.ts` + `lib/model-discovery.ts` + `lib/model-discovery-auth.ts`,超时 `DISCOVERY_TIMEOUT_MS = 20_000`:

1. 校验 providerName/baseUrl/api(默认 `openai-completions`)。
2. `buildModelsListUrl(baseUrl, api)`(`lib/model-discovery.ts:57-75`):路径不以 `/models` 结尾则追加;`anthropic-messages` 插 `/v1` 且加 `?limit=1000`;`google-generative-ai` 插 `/v1beta` 且加 `?pageSize=1000`。例:`https://api.anthropic.com` → `https://api.anthropic.com/v1/models?limit=1000`;`https://api.openai.com/v1` → `https://api.openai.com/v1/models`。
3. `resolveModelDiscoveryAuth`:mkdtemp 写**临时** models.json(只含该 provider + 占位 model),`ModelRuntime.create({modelsPath})` → `getAuth(model)` 从 auth.json 解析真实 key+headers(使托管 provider key 也生效),finally 清理。
4. `buildHeaders(api, apiKey, configuredHeaders)`:`Accept: application/json`;anthropic → `x-api-key` + `anthropic-version: 2023-06-01`;google → `x-goog-api-key`;其余 `Authorization: Bearer <key>`。
5. `fetch(endpoint, {cache:"no-store", signal: AbortSignal.timeout(20_000)})`;非 2xx → 502(上游 body 截断 500 字符);非法 JSON → 502。
6. `parseDiscoveredModels(payload)`:接受裸数组 / `{data|models|results|items:[...]}` / 对象之对象;条目可为 string 或 `{id|model|name, display_name|displayName, name}`;剥 `models/` 前缀(Gemini);去重;自然排序。
7. 响应:`{models: DiscoveredModel[], endpoint: "<最终URL>"}`。

UI:provider 详情面板 "Fetch models" 按钮(行 396-418)→ 可搜索勾选列表("Select shown"/"Add selected",行 530-605)→ `addDiscoveredModels` 并入本地状态(随后整体 PUT)。

### B. models.dev 目录 —— `GET /api/models-config/catalog`

- 抓 `https://models.dev/api.json`(15s 超时,1h 内存缓存 + in-flight 去重),`flattenModelsDevCatalog` 归一化,`recommendModelCatalogPreset` 按 provider id/baseUrl 主机名推荐预设,用于**补全单个 model 的元数据**(name/reasoning/input/contextWindow/maxTokens/cost),非拉列表。
- 离线环境可能不可达 → 只作可选增强。

## 3. 可用性检测

`app/api/models-config/test/route.ts`(121 行),`TEST_TIMEOUT_MS = 20_000`:

1. 请求安全校验(loopback/origin 白名单,`lib/request-security.ts`)。
2. mkdtemp 临时 models.json(只含该 provider+model)→ `ModelRuntime.create({modelsPath})`。
3. `modelRuntime.getAuth(model)` 解析 key;无 key → `{ok:false, error:'No API key found for "<providerName>"'}`。
4. `completeSimple(model, {messages:[{role:"user", content:"Reply with OK only.", timestamp:Date.now()}]}, {apiKey, headers, maxTokens:16, timeoutMs:20_000, maxRetries:0, cacheRetention:"none", signal, onResponse})` —— 真实补全请求(由 SDK 抽象到对应协议端点),16 token 上限、零重试。
5. AbortController + 20s setTimeout。
6. 成功判据:`message.stopReason !== "error" && !== "aborted"`;返回 `{ok:true, latencyMs, status, responseText}`(截断 300 字符);失败 `{ok:false, error, latencyMs?, status?}`。

UI(行 928-935, 1100-1148):Test 按钮旁状态胶囊 —— 绿 `Connected · {latencyMs}ms · HTTP {status} · {responseText}` / 红 `Failed · {meta} · {message}` / 灰 testing;provider/baseUrl/api/apiKey/model 变更即重置(`ModelTestState`,行 940-949)。

## 4. 用量统计(会话级,无全局面板)

- `lib/session-stats.ts`:`SessionFileStats {userMessages, assistantMessages, toolCalls, toolResults, totalMessages, tokens:{input,output,cacheRead,cacheWrite,total}, cost}`;`addUsage` 读 assistant/tool-result 消息的 `usage.*` 与 `usage.cost.total`;`computeSessionStats` 聚合**全部**条目(含 compaction/branch_summary,保证跨压缩单调)。
- 暴露:`GET /api/sessions/[id]` → `stats` + `totalActiveMs`;`mergeSessionStats` 处理 live+file 增量。
- cost 按 models.json 的 cost 字段逐模型计算;**没有**全局/账号级 dashboard。

## 5. 值得照抄的设计决策

1. canonical schema 直接采用 pi 的 models.json 形状(providers→models,含 cost/contextWindow/compat)——它与 cc-switch 的 pi 适配器一致,pi 侧渲染近恒等。
2. 发现/检测用**临时目录 + 临时 models.json**做一次性 SDK/请求上下文,不污染真实配置。
3. 原子写 0600 + 失效缓存代替重启。
4. 检测 = 真实最小补全(maxTokens 16、无重试、20s 超时),不是 ping。
5. 多形态解析 /v1/models 响应并剥前缀、去重。
