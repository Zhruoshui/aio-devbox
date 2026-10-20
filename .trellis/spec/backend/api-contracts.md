# API Contracts

Executable contracts for the axum HTTP surface. There are TWO axum servers:
the sandbox app (`app/src/routes/*.rs` owns each payload; its frontend
consumer is **mgr-web** via the mgr proxy — manifest/buttons mirrors in
`mgr-web/src/pages/workspace/types.ts`, stats no longer has a frontend
consumer since web/ retired) and the mgr control plane (`mgr/src/routes.rs`
+ module routers own each payload, frontend mirror `mgr-web/src/types.ts`)
— same decode-once-at-the-boundary rule (cross-layer-thinking-guide).

## GET /api/stats — container-view resource metrics

### 1. Scope / Trigger

Added by the footer-usability task (`.trellis/tasks/08-26-footer-usability/`).
Drives the statusbar resource readout and doubles as the backend heartbeat for
the connection dot. Any field change here is a cross-layer contract change:
update both code owners and the frontend renderer together.

### 2. Signatures

- Route: `GET /api/stats`, registered in `main.rs` **before** the
  `/api/*rest` seam catch-all (static segments win, same as `/api/manifest`).
- Backend owner: `app/src/routes/stats.rs::StatsSnapshot`
  (`#[allow(non_snake_case)]` — the camelCase field names ARE the wire
  contract; do not rename casually).
- Frontend mirror: `StatsSnapshot` (`memTotalBytes?`) — retired with web/;
  no current frontend consumer (kept as the app-side contract).
- Data source: `spawn_stats_sampler` (tokio task, 2s period) keeps the
  snapshot in `AppState.stats`; the handler only clones-and-returns — no
  cgroup reads on the request path.

### 3. Contracts

Response JSON (always 200):

```json
{
  "cpuPct": 12.4,
  "memUsedBytes": 1300000000,
  "memTotalBytes": null,
  "diskUsedBytes": 8500000000,
  "diskTotalBytes": 62000000000
}
```

- `cpuPct`: f64 0–100. Container CPU vs effective quota: cgroup v2
  `cpu.stat` `usage_usec` delta / (Δt × cpus_eff), where cpus_eff =
  `cpu.max` quota/period when limited, else `available_parallelism()`.
  First sample after boot is 0 (usage_usec is cumulative — a delta needs two
  reads).
- `memUsedBytes`: `memory.current − memory.stat inactive_file` — the same
  accounting `docker stats` uses. Raw `memory.current` includes page cache
  and reads far too high.
- `memTotalBytes`: the cgroup `memory.max` limit; `null` when `max` (no
  limit — the current compose default). The frontend renders absolute-only
  then (`MEM 336.5M`, no denominator).
- `diskUsedBytes` / `diskTotalBytes`: `statvfs("/home/gem")` — the workspace
  volume, not the container overlay.

All sources are **container-view** by design (user decision: the host may be
Windows/macOS/Linux; container semantics are uniform). Verified to match
`docker stats --no-stream` (mem, within sampling drift) and `df -B1 /home/gem`
(disk, exact).

### 4. Validation & Error Matrix

- cgroup/statvfs read or parse fails → `tracing::warn!`, keep the previous
  field value (first failure: 0 / `None`); endpoint still returns 200.
  Never 5xx, never panic — the footer is advisory.
- Backend unreachable from the frontend → `useStats` sets `online: false`:
  stats seg hidden, statusbar dot red, `statusOffline` text. Recovery is
  automatic on the next 3s poll.

### 5. Good/Base/Bad Cases

- Good: `{"cpuPct": 3.1, ..., "memTotalBytes": 4294967296}` — compose sets a
  memory limit; frontend shows `MEM 1.2G / 4G`.
- Base: `memTotalBytes: null` — unlimited; frontend shows `MEM 336.5M`.
- Bad: reading `memory.current` alone as "used" (page cache inflates it);
  returning 500 when the sampler hiccups; computing cpuPct from a single
  `cpu.stat` read.

### 6. Tests Required

- `web/smoke-test.cjs`: `.statusbar .seg-stats` renders and its text contains
  CPU / MEM / DISK (assertion `statsOk`).
- Manual cross-check on change: endpoint vs `docker stats --no-stream` and
  `df -B1 /home/gem` inside the app container.

### 7. Wrong vs Correct

Wrong: frontend derives "offline" from the manifest fetch only (refresh
failures are deliberately swallowed there).

Correct: the 3s `/api/stats` poll is the heartbeat — any failure flips
`online` false within one period; the manifest channel stays responsible for
button visibility only. Poll (3s) and sample (2s) periods are coprime so the
readout does not beat against the sampler.

## POST/DELETE /api/buttons + GET /preview/:port — user web-type buttons & dev-server preview

### 1. Scope / Trigger

Added by the web-button-preview task (`.trellis/tasks/09-01-web-button-preview/`,
issue #1). `POST /api/buttons` previously created agent buttons only; it now
accepts `type: "web"` + `port`, and a new dynamic reverse proxy serves
`/preview/<port>/*` from the app (axum) — NOT the gateway (a Caddy route cannot
reach a loopback-bound dev server; the gateway's catch-all already hands
`/preview/*` to axum, so Caddyfile/compose stay untouched).

### 2. Signatures

- `POST /api/buttons` — owner: `app/src/routes/buttons.rs::ButtonInput`
  (`{label, cmd?, type?, port?}`; `type` defaults to `"agent"`). Web buttons
  persist `cmd: ""` + `port: u16` in `buttons.toml`
  (`ButtonDef.port: Option<u16>`, `skip_serializing_if` — old files deserialize
  unchanged; agent rows omit the key).
- `GET /api/manifest` — unchanged shape; web user buttons emit
  `url: "/preview/<port>/"`, `target: "127.0.0.1:<port>"` (TCP probe, same
  semantics as built-in web buttons). Owner: `config.rs::load_buttons`
  (web-without-port rows are dropped with a warn, never a dead pane).
- `GET /preview/:port(/:path)` — owner: `app/src/routes/preview.rs`
  (`preview_proxy`). Routes: `/preview/:port`, `/preview/:port/`,
  `/preview/:port/*path` (matchit 0.7.3 catch-all needs a non-empty tail).
- `GET /api/buttons/probe?port=<N>` — added by 09-02-web-button-ux-fix.
  Owner: `app/src/routes/buttons.rs::probe_port`. TCP-dials `127.0.0.1:<port>`
  (same 400ms timeout as the manifest liveness probe in `config.rs`), returns
  `200 {"listening": bool}`. Port rules mirror POST validation (integer
  1-65535, 0/8088/non-numeric → 400). Non-blocking UX hint for the register
  dialog: `listening:false` is a warning, never an error — registration of a
  dead port stays allowed.
- Frontend mirror: `mgr-web/src/pages/workspace/types.ts::RegisterButtonInput`
  (reached through the mgr proxy `/api/sbx/:name/api/buttons*`, 契约 10).

### 3. Contracts

Validation matrix (POST, 400 on violation): web without port; port 0;
port 8088 (axum itself — proxying would recurse); unknown `type`; agent with
empty/oversized cmd. Web rows normalize `cmd` to `""`.

Proxy behavior: HTTP all-methods forwarded (hop-by-hop headers stripped, Host
rewritten to `127.0.0.1:<port>`), response bodies streamed unbuffered
(reqwest `bytes_stream` → `Body::from_stream`; SSE survives). WS upgrades are
detected via `Option<WebSocketUpgrade>` + the Upgrade header, connected with
tokio-tungstenite (plaintext, `WS_CONNECT_TIMEOUT` 2s), pumped message-level
both ways until either side closes; the upstream-negotiated
`Sec-WebSocket-Protocol` is echoed back to the browser (vite HMR negotiates
`vite-hmr` and aborts without it). Errors: upstream unreachable → 502;
non-numeric port / port 0 / 8088 → 404 (fast-fail, no proxy attempt).

### 4. Tests Required

- Unit: `config.rs` (web parsing, legacy no-port file, web-without-port drop),
  `buttons.rs::validate_shape` matrix, `preview.rs` pure fns
  (`port_allowed`, `upstream_path`, hop-by-hop filter).
- Focused integration (issue #1): register → manifest → proxy HTTP/SSE/WS →
  delete, 21 assertions (`/tmp/preview-itest/setup.sh` harness).

### 5. Wrong vs Correct

Wrong: proxying 8088 (self-recursion `/preview/8088/preview/...`), rewriting
HTML to fix root-absolute asset URLs (vite/Next need upstream `base` config —
documented in README), stripping the WS subprotocol on the way back.

## GET /api/manifest — `{env:VAR:default}` placeholder in built-in urls

### 1. Scope / Trigger

Added by the piweb-port-env task (`.trellis/tasks/09-02-piweb-port-env/`,
issue #3). Built-in `services.toml` urls may carry `{env:VAR:default}`
placeholders; they are expanded ONCE at app startup (`main.rs` maps
`config::expand_placeholders` over the loaded list) — env doesn't change
during the process lifetime, so the manifest handler never re-expands.

### 2. Signatures

- Owner: `app/src/config.rs::expand_placeholders` (+ private `expand_one`).
- Today's only consumer: piWeb's
  `url = "http://{host}:{env:PI_WEB_HOST_PORT:30141}/"` — the HOST-side
  publish port (compose passes `PI_WEB_HOST_PORT` through, defaulting 30141;
  `target` stays `app:30141`, the sandbox-net-internal probe, unaffected).
- Frontend is unaware: it still only substitutes `{host}` client-side.

### 3. Contracts

- Semantics: set VAR (non-empty) => its value; unset OR EMPTY var => the
  default after the second colon. An empty env var is treated as unset on
  purpose — compose projects interpolate `PI_WEB_HOST_PORT=` from a
  half-edited `.env`, and an empty port would render `http://host:/`.
- Malformed specs (`{env:VAR}` with no default, `{env:}`, unbalanced braces,
  non-env braced groups like `{host}`) pass through verbatim — a typo in
  services.toml stays visible in the manifest instead of being silently
  swallowed.
- User buttons (`buttons.toml`) are NOT expanded — their urls are generated
  (`/preview/<port>/`), not authored, so they can't carry placeholders.

### 4. Tests Required

- Unit (`config.rs` tests): env-override, missing-env default, empty-env
  default, multiple/adjacent placeholders, malformed pass-through, no
  closing brace, plain string. Verified live: `PI_WEB_HOST_PORT=30142` makes
  the manifest piWeb url `http://{host}:30142/`; unset keeps `:30141`.

---

## 对外端口可配契约(gateway-port-env,issue #16)

单容器栈(`docker-compose.yml`)的对外端口约定:

- **gateway 入口**:`AIO_GATEWAY_PORT`(默认 8080)。同一变量驱动两处,两侧
  **必须相等**:compose 发布映射 `${AIO_GATEWAY_PORT:-8080}:${AIO_GATEWAY_PORT:-8080}`
  与 `gateway/Caddyfile` 站点地址 `:{$AIO_GATEWAY_PORT:8080}`(caddy 原生 env
  占位符,容器内加载时展开)。
- **关键陷阱**:`environment:` 条目不可省——它走 compose 插值且优先级高于
  `env_file: .env`。`AIO_GATEWAY_PORT=8082 make up` 这类 shell 覆盖只进得了
  environment(发布映射随插值同步),env_file 读不到 shell 值,caddy 会展开
  回默认 8080,两侧漂移。
- 与 `PI_WEB_HOST_PORT`(30141,见上文 manifest 占位符一节)同构:直发端口的
  配对规则 = 宿主侧与沙箱侧绑定同一变量值。
- sbx 沙箱场景:宿主对外端口由 `sbx ports --publish` 决定,与上述变量正交;
  这些变量主要服务裸 `docker compose` 部署。

---

# mgr 控制面 API(`mgr/src/routes.rs` + 模块子路由)

sandbox-mgr(09-08-sandbox-mgr-tui Phase 1/3 + 09-09-sandbox-mgr-unified)。
base path:容器形态经总网关 `http://mgr.localhost` → `mgr-api:8089`;裸跑
形态直连 `MGR_BIND`(默认 `:8089`)。mgr-web SPA 与静态 `/api` seam 同
app 模式(见 GET /api/stats 的路由顺序规则)。所有错误统一
`{"error": "<message>"}` JSON;校验类 400,内部失败 500(anyhow 链尾)。

子路由模块各自持有自己的 router,经 `merge` 注册在 routes.rs 总 router、
**seam 之前**(merge 是"静态段先赢"规则的注册序形式): `models.rs`/
`usage.rs`/`proxy.rs`。新增模块照此并入。

## GET /api/sandboxes — 列表(含实时状态合并)

### 1. Scope / Trigger

mgr-web 列表页数据源;DB status 是"意图"(creating/running/error),`live`
是 compose ps 的实时事实。两个维度刻意分离——DB 说"应该 running"而 live
说"gone"时,说明有人手工 `docker compose down` 过,UI 必须如实展示而非隐藏。

### 2. Signatures

- Owner: `mgr/src/routes.rs::sandbox_json`(单行 payload 构造,列表与详情
  共用;改字段 = 两处同步 `mgr-web/src/types.ts::Sandbox`)。
- `live` 枚举: `"running" | "stopped" | "gone" | "unknown"`——**unknown 是
  compose ps 本身失败**(docker daemon 挂/文件丢失),与"没有容器"(gone)
  语义不同;Err 分支必须显式报 unknown,不能映射成空列表(否则 daemon 故障
  会显示所有沙箱"已消失")。

### 3. Contracts

列表项字段(12, S1 起;cc-switch Phase 1.2b 起移除 model_profile/
model_agents): `name/status/live/adopted/created_at/cpus/mem_mb/
env/image/entry_url/piweb_url/services[]/installed_services`。
URL 字段在 payload 里内联生成(`http://sbx-<name>.mgr.localhost/`),sbx-
前缀与 caddy.rs render 及 composegen 网络别名三方共享同一身份——改前缀
必须三处同改。
`services[]`: compose ps 的**运行时容器**列表(字段 11,命名被占用,所以
服务开关字段不得叫 `services`——见下)。
`installed_services`: **装了什么服务**的只读四开关
`{code_server, vnc, pi, pi_web}`(S1,任务 09-10-mgr-create-services),
由 `mgr/src/routes.rs::installed_services_of` 折叠:code_server/vnc 读
`services_json` 列;pi/pi_web 由 `env.scenarios` 推导。**S1 之前的行
(`services_json` NULL)→ 四开关全 true**——pre-S1 原生沙箱按"服务无条件"
构建(pi/pi-web 当时是 always_on 场景,从不进 scenarios),推导自空集合
会错误报告"未装";adopt 行同样保持 NULL→全开(adopt 流程的既定选择)。

### 4. Validation & Error Matrix

- name slug: `[a-z0-9-]+`,≤32 字符,字母数字开头;保留 `mgr` 与 `sbx-`
  前缀 → 400。前端 `mgr-web/src/pages/CreatePage.tsx::NAME_RE` 必须与
  `validate_name` 等价(提交前先本地拒)。

### 5. Tests Required

- `caddy.rs` render 单测(站点块存在/删除消失/mgr 静态站优先)锁 URL 形状。

## POST/PUT sandboxes — services 四开关(S1,09-10-mgr-create-services)

create 请求体与 PUT 请求体均可选带 `services: {code_server, vnc, pi,
pi_web}`,**四键缺省全 true**(`ServicesBody` 手写 `Default`,防 derive 全
false 把旧客户端无 services 字段的请求译成"全关")。归一化
(`routes.rs::normalize_services`,create 与 PUT 同一 helper):

- pi/pi_web 是**场景**(单源真值在 `env.scenarios`):开关先剔除再按结果
  写回——`pi=true` 推 `"pi"`,`pi_web=true` 推 `"pi"` + `"pi-web"`;关掉
  则从 scenarios 剔除。
- **pi_web=true 时 `pi` 与 `vnc` 都不得为 false,否则 400**(错误
  "pi-web 依赖 pi 与 vnc…")——pi-web 的配置挂在 pi 安装下、其 Chromium
  由 vnc 侧车承载,两者缺一即矛盾(R1/AC3;前端联动是客户端一半,后端
  校验兜底)。
- code_server/vnc 原样写 `services_json` 列(只存这两个布尔;pi/pi_web
  存两份即双源真值)。
- 归一化**先于** `to_manifest_checked`:场景集进 manifest 校验。

**PUT 的 services 字段刻意忽略**(声明并注释,不是 serde 静默跳过):安装
集由镜像内容在 create 时定死,不可改。但 PUT 的 env 换血不能把隐藏在
env.scenarios 里的 pi/pi-web 弄丢——PUT 侧用**当前行的四开关形状**
(`installed_services_of`)重归一化:pre-S1 行读成全开 → 首次 PUT-recreate
把 pi/pi-web 重新烘焙进场景 → 装配字节与 always_on 时代逐字节一致 →
同 hash、复用镜像,不会静默剥掉已装服务(AC4 回归门)。

## PUT /api/sandboxes/:name — limits 三态语义

**缺失 = 保留当前值;`>0` = 设置;`0` = 清除(无限制)。** `null` 与缺失
同义(serde Option),所以 UI 的"清除"必须显式发 `0`,不能发 `null`——纯
null 方案下 UI 无法区分"未更改"与"清除"。创建侧对偶:`0`/null 归一化为
无限制。负数 400(`check_limits`)。改 env 走 recreate job(202 + `{job}`),
卷保留。

## POST /api/sandboxes/:name/service/:service/start — 按需单服务拉起(unified Phase 3, D4)

Synchronous(compose `up -d <svc>`,秒级),无 job。`service` 白名单当前仅
`code-server`;沙箱 not running → 400(停着的栈不得半启动回来);
adopted 行走 `compose_service_up_file` 变体(无 `-p`,契约 8)。前端 pane
打开时探测可达(`probe?port=8200`)直接 iframe,否则 start → probe 轮询
→ iframe。profile 分裂规则见 [sandbox-mgr-ops.md 契约 4](./sandbox-mgr-ops.md)。

## /api/sbx/:name/* — 沙箱代理路由组(unified Phase 1)

**Owner**: `mgr/src/proxy.rs`(any-method catch-all)。mgr-web 的一切沙箱面
(终端 WS `/api/sbx/<n>/api/term/ws?cmd=...`、buttons CRUD +
probe、manifest、`/preview/<port>/*`)经此代理到 `http://sbx-<name>-
piweb:8088/<path>`——上游宿主由 name 封闭派生(无 SSRF 面),HTTP+WS
双透传。完整契约(宿主派生封闭性/别名/路由形状/错误语义)见
[sandbox-mgr-ops.md 契约 10](./sandbox-mgr-ops.md);暴露面并入契约 9
的认证四处清单。

## images 表 build_log 的 A5 保留语义

同 env 第二个沙箱复用镜像(A5)时,`upsert_image` 以**空 log**命中
`ON CONFLICT DO UPDATE`——必须 CASE WHEN 保留旧 build_log/built_at,否则
原始构建日志被空值覆盖,镜像页日志按钮永久禁用。`mgr/src/db.rs` 单测
锁语义(3 个)。S3 起 `upsert_image` 增 `combo` 参数(5 参):组合描述随
每写携带,ON CONFLICT 时 `COALESCE(?5, images.combo)`——NULL combo 不覆盖
已有描述(旧行 A5 复用场景)。

R3(picker UX)起 `upsert_image` 为 7 参:追加 `env_json` / `services_json`
(结构化组合详情源,create 时写 `SandboxEnv::canonical_json()` 与
`Services::canonical_json()`)。两者与 build_log 同款 `CASE WHEN ?n = ''`
保留语义(空串 = 不覆盖旧值);与 combo 一样**不**是空 log 才写——同 env
重建每次都带真值。`list_images` / `ImageRow` 相应扩为 7 元组。

**失败构建也留行(R3)**:base 构建失败时 create job 仍 upsert 一行
(build_log = 失败输出 tail、combo/env_json/services_json 照常),
使镜像页保留失败现场;该行 tag 对应的镜像并不存在,故
`size_bytes` 为 null、refcount 为 0(cleanup/delete 走「缺失跳过」路径),
前端复用提示类逻辑须以 `size_bytes !== null` 作为「镜像真实存在」判据。

**build_log 双 cap(R3)**:job 运行期日志尾 8KB(`LOG_TAIL`),images 表
留存量 256KB(`BUILD_LOG_TAIL`,mgr/src/jobs.rs)——8KB 常把真正的
错误行裁掉。注意失败路径写入的是 `docker::build` 错误里的 merged tail
(2KB,`docker.rs` 的 `tail_str`),因为同一字符串还要进 job.error
(1.5s 轮询 + jobs 表落库),不能放大。

## /api/models/providers + /api/models/templates — cc-switch 供应商库(10-04-model-config-cc-switch Phase 1.2/3)

cc-switch 重构后 mgr 只保留**供应商库**(共享模板源)。旧 profile 系统、
mgr 侧 canonical config、`import/pi`、`catalog`、`PUT /api/sandboxes/:name/
model_profile` 全部移除(prd D1/D6)。

- **存储**: kv 键 `models_library`,值 = `aio_models::template::ProviderLibrary`
  `{version: u32, providers: BTreeMap<id, ProviderTemplate>}`。`ProviderTemplate`
  形状见 `aio-models/src/template.rs`(name/baseUrl/api/apiKey/headers/models/
  websiteUrl/category/notes,全 camelCase 序列化)。
- **迁移**: 读路径在 `models_library` 缺失时自动从旧 `models_profiles`(multi-profile)
  或更老的 `models_config`(Phase 4a)提取 providers 写入新键并删旧键
  (models.rs `read_library`)。
- **masked-echo 契约**: GET `/api/models/providers` 返回 mask 后 key
  (`<first4>****<last4>`,短 key 全 `****`);PUT/POST 回传 mask = 保留原值,
  `""` = 清除,其它 = 替换,**缺失 = 保留 stored plaintext**(merge_api_keys
  语义,与 canonical 时代一致)。
- **路由**:
  - `GET /api/models/providers` — masked 全库
  - `PUT /api/models/providers` — 整体替换(merge + validate + 版本 +1)
  - `POST /api/models/providers` — `{id, ...template}` 新增(id 由前端 slug,
    重复 id 400)
  - `PUT /api/models/providers/:id` — 单条更新(同样 masked-echo;404 未知 id)
  - `DELETE /api/models/providers/:id` — 单条删除(404 未知 id)
  - `GET /api/models/templates` — 模板列表 **携带真实 apiKey**(10-13
    models-config-ux R2,推翻 10-04 的"不传 key"决定:唯一消费方是 mgr-web
    自身,与供应商编辑器同信任域;前端面板仅在 key 字段为空时预填,不覆盖
    用户输入);响应 `{version, templates: [{id, name, baseUrl, api,
    apiKey?, models, websiteUrl?, category?, notes?}]}`
  - `POST /api/models/discover` + `POST /api/models/test` — 纯 HTTP 探测,与
    profile 无关,保留不变(ById/Literal untagged 两形态,详见 Model Config
    Guide 的 discover/test 节)

## GET /api/models/usage (app) — byDay 序列(S4,D5)

响应在既有 `rows` + `generatedAt` 之上增 `byDay`:近 14 个自然日
(UTC)`(date, agent, model)` 的逐日聚合。

```json
{ "rows": [/* 不变 */], "byDay": [{ "date": "2026-09-10", "agent": "pi",
  "model": "claude-sonnet-4-6", "in": 100, "out": 50, "cacheRead": 10,
  "cacheWrite": 5, "cost": 0.01 }], "generatedAt": "…Z" }
```

- **与 window 参数解耦**(S4 关键语义):byDay 恒定最近 14 天
  `[today-13 … today]`(扫描器先累加日桶、后做窗口 cutoff,再按
  `now_day` 裁剪)。today/7d 窗口的 byDay 不因此被截断;all 窗口也不吐
  出全部历史。
- 每项 `cost` 仅当来源记账了成本(pi/opencode);claude/codex 日项
  `cost` 缺省 → 前端隐藏成本系列(AC4)。
- 无数据的日子**不发合成行**——前端按 14 日跨度 gap-fill 到 0。
- 与 `rows` 的时间窗口互相独立:选某个 window 不影响趋势图。
- 向后兼容:旧前端(无 byDay 消费)忽略该字段;旧 app(无 byDay)不返回
  → 新前端隐藏趋势图。

## GET /api/usage (mgr) — totals 派生(S4,D5)

响应在既有 `sandboxes` 之上增 `totals`:

```json
{ "sandboxes": [/* 不变 */], "totals": [{ "name": "sbx-a", "in": 123,
  "out": 45, "cost": 0.015 }] }
```

- `totals` 是 **mgr 对已缓存 entries 的纯 sum**(design §2),不含重新
  扫描:对每个非 error entry 的 `usage.rows` 累加 `in`/`out`/`cost`
  (`cost` 缺失按 0);error entry 的项三个字段全 0。
- 用于合计视图的「分沙箱条形图」;若无 totals(旧 mgr)前端从 entries
  推算。
- 不入缓存——由 30s 缓存的 entries 派生,命中缓存即算。

## images 管理端点(S3,D3)

- `GET /api/images` — 每行 `{env_hash, tag, built_at, refcount, build_log,
  combo, env, services, size_bytes}`。`combo` 为可读组合描述(create 时 envhash:
  `describe_combo(env, services)` 写入;旧行 NULL,前端回退 env_hash[:12]。
  pi/pi-web 服务开关从 env.scenarios 推导,S1 折叠后 Services 仅含
  cs/vnc)。`size_bytes` = 实时 `docker image inspect {{.Size}}`(base tag),
  查询失败/镜像不存在 → null(前端显示 —),**永不因此报错**。
- `env` = 构建时 `SandboxEnv` 的规范化 JSON(`{scenarios, versions}`),
  `services` = 构建时 `{code_server, vnc}` 布尔组(R3:镜像页组合详情面板的
  结构化数据源,不再解析 combo 字符串)。两者来自 images 表 R3 新增的
  `env_json`/`services_json` 列;列缺失(NULL,pre-R3 行)或 JSON 解析失败
  → **null**(never break on the unknown),前端回退为「仅可看构建日志」提示。
  不入库历史回填(D-b)。
- `POST /api/images/:env_hash/delete` → **202 {job}**(kind `image-delete`)。
  预检同步:env_hash 必须 64 hex(否则 400);行不存在 404;`refcount > 0`
  → **409**(R3,含构建中引用——create/recreate 沙箱在 run_create 写
  env_hash 后 refcount 即含之)。job 内**重查 refcount**(竞争窗口兜底),
  `image_tags(hash)` 三 tag 依次 `image_rmi -f`(缺失跳过),全成功才
  `delete_image_row`;任一 rmi 失败 → 中止报错、**DB 行保留**(下次构建
  upsert 重建,R5 不半删)。**不碰 vnc**(全局共享无行可查)。
- `POST /api/images/cleanup` → **202 {job}**(kind `image-cleanup`)。job
  遍历 refcount=0 行逐行按上法删组(单行失败报告后继续,R4),累计回收
  bytes + `docker builder prune`,log 汇总 `cleanup done: N image(s)
  removed, M failed, reclaimed X bytes of images`。
- rmi 安全:只允许 `image_tags(hash)` 产出的 `sandbox-` 前缀 tag
  (`docker::is_owned_image_tag`),任意 tag 注入被拒。
- R3 起 `GET /api/images` 行增 `job_id`(该行构建 job;pre-R3 行 null)。
  `job_id → jobs.log_path` 是完整构建日志的寻址路径,见下节。

## 完整构建日志端点(R3,09-23-image-detail-tools-log)

create/recreate job 的**完整**输出落盘在 `<data>/jobs/<id>.log`(短周期
job —— delete / image-delete / image-cleanup —— 不落盘,内存尾即全文),
jobs 表 `log_path` 列在构建**开始前**写入(`set_job_log_path`;`persist_job`
用 `COALESCE` 保留,不被 job 结束时的空值抹掉),因此 images 行能在 mgr
重启后仍找到自己的构建日志。

- `GET /api/jobs/:id/log?offset=&limit=&tail=`(owner `routes.rs::job_log`)
  与 `GET /api/images/:env_hash/log?offset=&limit=&tail=`(owner
  `routes.rs::image_log`)返回同一形态:
  `{data, total_size, next_offset, log_path, complete}`。
- **offset/limit/tail 全是 BYTE**,不是 JS 字符数。`data` 两端都被走到
  char 边界(`jobs::log_page`),分页必须按 `next_offset` 前进——
  用 `data.length` 计数会漏/重(UTF-16 != 字节)。`complete:false`
  = 来源是内存尾/`build_log` 列回退,不是完整文件(前端据此标注"尾部")。
- 取值优先级:job 侧 = 内存单元的 `log_path` → jobs 表 `log_path` → 内存
  log 尾;image 侧 = `images.job_id` → jobs 表 `log_path` → `build_log`
  列。文件缺失/不可读与"没有文件"同路(debug/warn 后回退),**不 5xx**。
- 上限:`limit` 缺省 256KB,clamp 到 [1, 4MB](`LOG_PAGE_MAX`);文件读在
  `spawn_blocking` 上(4MB+ 读不占 reactor)。job 不存在 → 400
  (lifecycle 的 400-shaped not-found,与 `GET /api/jobs/:id` 一致);
  image 侧 env_hash 非 64 hex 或行不存在 → 400。
- 内存尾 cap `LOG_TAIL` = 64KB,images 表 `build_log` cap
  `BUILD_LOG_TAIL` = 4MB;两者截断都走 `keep_tail`(向前对齐 char 边界,
  多字节输出不再 panic)。
- create job 的**所有**行(含 docker build 逐行输出与 job 自己的行)都经
  **同一个有序 drain 队列**写入内存尾与磁盘,R3 的 `flush_chunks` 是屏障:
  写 images 行(job_id 指向该日志)与返回 ok/error 之前必须已落盘,
  否则镜像页会看到缺尾的日志。
- 测试:`jobs.rs`(keep_tail/log_page char 边界与 panic 回归、落盘全文 vs
  内存尾、flush 屏障)、`routes.rs`(分页跨多字节重建、>256KB、回退矩阵、
  seam 不吞这两个路由)、`db.rs`(job_id 保留语义 / 迁移幂等)。

## user-tools fragment 编辑器(R2,09-23-image-detail-tools-log)

mgr 对 `state.repo` **唯一**的写路径(其余全部只读,契约 1):WebUI 编辑
`scenarios/user-tools/fragment.Dockerfile`(L2 用户自定义工具层)。三个
静态路由,owner `routes.rs` + `mgr/src/usertools.rs`(后者独占文件**格式**):

- `GET /api/user-tools` → `{content, entries, parse_note, path}`。
  `content` = 文件原文;`entries` = 表单解析结果,**null = 文件含表单无法
  表达的指令**(页面据此禁用表单、切直接编辑),`[]` = 空表单态;
  `parse_note` 为 `null` 时表示解析成功。字段名沿用 mgr 的 snake_case
  (设计稿里的 `parseNote` 只是 JS 侧草写)。**文件缺失不是错误** → `""`
  + `entries: []`(首次保存时创建)。
- `PUT /api/user-tools` body `{content}` → `{ok:true}`;**原文入库**
  (注释/空行逐字节保留,AC5)。只做静态校验,不做规范化。
- `PUT /api/user-tools/entries` body `{entries: ToolEntry[]}` → 由后端
  `render` 生成完整 fragment 后原子写入。**空数组 = 写回"无自定义工具"
  占位 RUN**(不是空文件)。

`ToolEntry`: `{name, install: "apt"|"tarball"|"script", version?, url?,
install_path, test_cmd?}`。表单与直接编辑**读写同一份文件**,单向生成 +
回落——解析不出条目就禁用表单,不做双向同步。

校验矩阵(违反 → 400 `{"error": …}`,两条写路由同一套硬规则的执行点):

- 空内容 400。
- **/root 写入 400**:`WORKDIR`/`COPY`/`ADD` 目标、`RUN` 的任意 token 指向
  `/root` 或 `~`。检查作用于**逻辑指令**(`\` 续行合并、续行里的注释行
  跳过)——安装步骤的常见形态就是 `RUN … \` + 后续物理行,按物理行扫描
  会正好漏掉写入点。`COPY` 的 `/root` **源**允许(构建期该层自有)。
  注释行始终豁免(文件自身的硬规则说明就引用了 `/root`)。
- 表单字段级:`name` `^[A-Za-z0-9][A-Za-z0-9._-]*$`(与页面 `rowError`
  等价,前端先拒、后端兜底);`install_path` 闭集
  `/usr/local/bin`|`/opt`;tarball/script 必填 `https://` URL;
  `version` 不含空白。渲染结果再过一遍原文守卫(纵深防御)。
- 写盘失败(容器里 repo 是 ro)→ **500**,anyhow 链尾带 OS 错误与
  "repo must be mounted read-write" 提示;绝不做"静默接受"。

容器形态的挂载:`mgr/compose.yml` 保持 `${PWD}:${PWD}:ro`,**另加**
`${PWD}/scenarios/user-tools` 的嵌套 rw bind(同宿主路径,路径一致性)。
挂**目录**而非单文件:保存是 temp + rename 原子替换(`write_atomic`),
换 inode 会让单文件 bind 失效。

测试:`usertools.rs`(render↔parse 全 kind 往返、续行 /root 守卫、原子写
无残留、名称与 URL 矩阵)、`routes.rs`(三个端点 + 缺失文件 + 原文入库 +
拒 /root + seam 不吞)。

## /api seam 404(app 与 mgr 同构)

未知 `/api/*` 路径不得落到 SPA fallback(否则 `/api/nonexistent` 返回
200 HTML,前端 fetch 解析为文本"成功")。app 的三路由 seam 模式
(`/api`、`/api/`、`/api/*rest` any-method → 404 JSON)在 mgr
`main.rs` 同样落地;新增 API 路由注册在 seam 之前。
