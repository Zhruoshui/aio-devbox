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

列表项字段(14, S1 起): `name/status/live/adopted/created_at/cpus/mem_mb/
env/image/entry_url/piweb_url/model_profile/services[]/installed_services`。
URL 字段在 payload 里内联生成(`http://sbx-<name>.mgr.localhost/`),sbx-
前缀与 caddy.rs render 及 composegen 网络别名三方共享同一身份——改前缀
必须三处同改。
`model_profile`: 所指派 profile id 或 `null`(未指派 = 沙箱保持本地
models.json;unified Phase 4/D8)。指派解析**每次列表调用读一次** store
(一次解析,非每行——models store 可能不小),详情逐行读。
`services[]`: compose ps 的**运行时容器**列表(字段 13,命名被占用,所以
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

## PUT /api/sandboxes/:name/model_profile — 指派语义(unified Phase 4, D8; S2 增 agents)

Body `{"profile": "<id>" | null, "agents": <subset> | null}`:
- `profile`: 指派 id;null/缺失 = 解绑。
- `agents`(S2, D4c): **整份替换**语义——缺省 = 全指派,`[]` = 零指派
  (沙箱拉取 404 → 保持本地),数组 = 精确子集(仅渲染勾选 agent)。
  **必须始终随 PUT 携带**,省略会让后端 serde default 把已有子集悄悄放大
  为全指派(frontend 编辑页/快捷指派都显式传)。
- **纯 kv 写,同步返回,绝不触发 recreate job**——沙箱的 60s 拉取自然生效;
  这是它与 `PUT /api/sandboxes/:name`(env 改动走 recreate)被刻意拆成两条
  路由的全部理由,不得合并。未知沙箱 400(`require_row` 同形);未知
  profile id 404;agent 名不在 {pi,claude,codex,opencode} 白名单内 400
  (models.rs `VALID_AGENTS`,`set_assignment`)。响应增
  `model_agents`(与 body 同形);`GET /api/sandboxes`/`:name` 的
  `sandbox_json` 增 `model_agents`(null = 全指派,旧数据兼容,AC4)。
  错误矩阵与写路径细节见 [sandbox-mgr-ops.md 契约 7](./sandbox-mgr-ops.md)。

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
锁语义(3 个)。

## /api seam 404(app 与 mgr 同构)

未知 `/api/*` 路径不得落到 SPA fallback(否则 `/api/nonexistent` 返回
200 HTML,前端 fetch 解析为文本"成功")。app 的三路由 seam 模式
(`/api`、`/api/`、`/api/*rest` any-method → 404 JSON)在 mgr
`main.rs` 同样落地;新增 API 路由注册在 seam 之前。
