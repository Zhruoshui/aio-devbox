# Research: verbatim spec quotes — 契约 3, 4, 7, 9 + api-contracts mgr section + frontend conventions

- **Query**: faithful capture of current spec text that this task will edit: .trellis/spec/backend/sandbox-mgr-ops.md 契约 3/4/7/9 (full text), api-contracts.md mgr section, frontend/directory-structure.md mgr-web conventions, frontend/xterm-pane.md.
- **Scope**: internal (spec quotes)
- **Date**: 2026-09-09

All quotes below are VERBATIM from the current files (commit f92fd84 working tree). Line numbers refer to the current files.

---

## 1. .trellis/spec/backend/sandbox-mgr-ops.md — 契约 3 (lines 71-95)

```markdown
## 契约 3: 总网关 Host 重写必须用公共子域名

**Trigger**: 任何反向代理到 pi-web(`sbx-<name>-piweb:30141`)的网关配置。

pi-web 的 request-security 中间件(middleware.js)对 Host 的实际匹配规则:

1. 剥端口(`new URL('http://'+host).hostname`),再校验 hostname;
2. 放行: `localhost` / **`*.localhost` 后缀** / IP 字面量 /
   `PI_WEB_ALLOWED_HOSTS` 条目(逗号分隔,同样剥端口比较);
3. 其余一律 403 `Untrusted request`(页面路径)或 403
   `Untrusted API request`(/api)。

**规则**: 总网关重写 Host 时写**公共子域名**,不是上游 alias:

```caddyfile
# 正确 (mgr/src/caddy.rs render): *.localhost 后缀命中规则 2
header_up Host sbx-<name>-piweb.mgr.localhost

# 错误: 裸 alias 不以 .localhost 结尾、不在 ALLOWED_HOSTS → 403
# header_up Host sbx-<name>-piweb:30141
```

注意与 `PI_WEB_ALLOWED_HOSTS`(compose 里 `app,sbx-<name>-piweb.mgr.localhost`)
是双保险关系:即使 env 丢失,`*.localhost` 后缀仍放行。design.md §2 早期版本
写的是 alias 形式,已于 09-08 更正——以本文为准。
```

## 2. sandbox-mgr-ops.md — 契约 4 (lines 99-115)

```markdown
## 契约 4: mgr 生命周期命令必须带全量 profile

mgr 沙箱的 code-server/vnc 在生成的 compose 里是 profile 门控服务(与 repo
compose 同构,design §3.5),但 mgr 把沙箱当整体产品管理:镜像全部构建、
up 必须全起(否则工作台面板缺一半)、down 必须看到它们才能拆干净。

```rust
// mgr/src/docker.rs
const SANDBOX_PROFILES: [&str; 4] = ["--profile", "code-server", "--profile", "vnc"];
```

`compose_ps` 例外: compose 5.x 的 `ps --all` 列出 profile 门控容器,
不需要(也不应该)加 profile 标志。

**验证点**: mgr 创建的沙箱应有 4 个容器(app/gateway/code-server/vnc);
`GET /api/manifest` 中 codeServer/vnc/piWeb 全部 `enabled: true`。
```

(D4 — code-server lazy start — directly amends this contract: up no longer starts code-server unconditionally; the "up 必须全起" rationale and the 4-container verification point both change. vnc stays always-on per D4.)

## 3. sandbox-mgr-ops.md — 契约 7 (lines 167-210)

```markdown
## 契约 7: 模型配置上收——同步链与写降级(Phase 4)

**Trigger**: 任何动 `mgr/src/models.rs`、`app/src/mgr_sync.rs`、
`app/src/routes/models/mod.rs` 写接口、或 composegen MGR_URL 注入的人。

mgr 是模型配置唯一真相源(D6),三段式同步链,任何一段的字段名/语义
漂移都会让拉取静默失效(拉不到≠报错,是 60s 空转):

1. **真相源**: kv 表 `models_config` 键,值 `{"version": <u64>, "config":
   <CanonicalConfig>}`,PUT 成功才 bump version。kv 只可能写入通过
   validate 的 JSON,因此 mgr 侧无 app 的 corrupt-move-aside 分支
   (app models.json 是文件、mgr 是 kv——损坏语义不同是**有意的**)。
2. **拉取端点**: `GET /api/models/sync` 返回**未 mask** canonical(明文
   key)。这是 D6/D9 已接受的边界: mgr-api 不发布宿主端口、aio-mgr-net
   不出宿主、总网关站点块只按 Host 路由沙箱域名。**不要**在 mgr-web 里
   调它(要明文没意义),浏览器走 masked 的 `GET /api/models/config`。
   消费端 `app/src/mgr_sync.rs` 的解析结构体与 mgr 的响应形状有处理器
   级测试双向锁定(`sync_handler_shape_*` / `sync_payload_decodes_*`)。
3. **沙箱侧**: composegen 给 app 注入 `MGR_URL=http://mgr-api:8089`;
   启动拉一次 + 60s 周期;深比较(serde_json 全量等值)不同才
   `write_config` + `apply_all_agents`(与 apply/:agent handler 共用
   `render_agent`,单一渲染路径);拉取失败 warn 一次静默用本地缓存。
   `MGR_URL` 未设置 = 存量栈,零行为变化(guard 恒通、不 spawn)。

**写降级矩阵**: MGR_URL 设置时 app 侧 6 个写接口统一 403 body
`managed-by-mgr`(PUT config、import/pi、apply/:agent、provider PUT/
DELETE、sync);前端 `GET /api/models/managed` 探测只读态 + 403 兜底。
GET 类(config/agents/usage/catalog/managed)与 discover/test 探测**不降级**。

**usage 扇出**: `GET /api/usage?window=` 按需并发拉各 running 沙箱
`http://sbx-<name>-piweb:8088/api/models/usage`(注意别名是
**sbx-<name>-piweb**——composegen 给 app 的 aio-mgr-net 别名;design §3.7
原文的 `sbx-<name>:8088` 是笔误,`sbx-<name>` 是 gateway 的 :80 别名)。
单沙箱 5s 超时、错误隔离进 `error` 字段、30s TTL 缓存(含错误条目,
避免错误沙箱被高频重试)。design 原文的"60s 常驻轮询"实现为按需扇出
+TTL——等价满足汇总语义,少一份常驻状态。

**验证点**: 改 mgr 配置后沙箱 canonical(明文)与 pi native render 在
≤60s 内更新;沙箱 PUT 返回 403 `managed-by-mgr`;`/api/usage` 含各
running 沙箱条目且单沙箱挂掉不整体失败。

**mgr 不提供的端点**(沙箱本地文件操作,mgr 语义不成立,mgr-web 移植
时裁掉): `/api/models/agents`、`apply/:agent`、`agents/:agent/provider/
:id`、`agents/:agent/sync`、单沙箱 `/api/models/usage`。
```

(D8 — multi-profile — rewrites clauses 1 (kv shape) and the sync chain identity, plus "mgr 是模型配置唯一真相源" gains a per-sandbox dimension.)

## 4. sandbox-mgr-ops.md — 契约 9 (lines 255-277)

```markdown
## 契约 9: 全栈无认证——安全边界与残留清理(Phase 5, D9)

**Trigger**: 任何想给网关/mgr 加回认证、或在不受信网络部署的人。

D9 决策: 信任边界 = 宿主机/本机。**全面无认证**——存量栈 gateway
(去 basicauth 后的 repo gateway/Caddyfile)、mgr 总网关(caddy.rs
render,有单测 `render_never_contains_basicauth` 锚定)、每沙箱生成的
gateway(composegen,同锚定)。

- 重新引入认证必须三处同步: repo Caddyfile + caddy.rs render +
  composegen render_caddyfile(任一遗漏 = 部分路由裸奔)。
- 历史机制已删,不可"顺手恢复": `make hash`/`ensure-hash`、
  gateway/secrets/、gateway/entrypoint.sh(hash 投递)、Makefile
  save/load 的 hash 打包、CI 冒烟的 `-u admin:admin`。
- `docker-compose.yml` gateway 的 `env_file: .env` **保留**(PI_WEB_HOST_PORT
  等仍需要;SANDBOX_USER 残留在旧 .env 里是无害未引用 env)。
- 部署边界: mgr 总网关 `ports: 80:80` 是**宿主入口**,任何 LAN 暴露
  = 无认证暴露全部沙箱 + mgr 控制面。远程使用自行加 VPN/带认证反代。

**验证点**: `grep -rn "basicauth\|SANDBOX_USER\|ensure-hash\|secrets/hash"
Makefile docker-compose.yml gateway/ .env.example .github/ README* docs/`
清零;`make -n up NOBUILD=1` 无 ensure-hash 报错;网关直连 200 无
WWW-Authenticate 头。
```

(Relevant to D6 terminal/agent WS proxy: the new mgr proxy route extends this same unauthenticated boundary — the pty WS hands out full shells; document it here.)

## 5. .trellis/spec/backend/api-contracts.md — mgr section (lines 201-265, full)

```markdown
# mgr 控制面 API(`mgr/src/routes.rs`)

sandbox-mgr(09-08-sandbox-mgr-tui Phase 1/3)。base path:容器形态经总网关
`http://mgr.localhost` → `mgr-api:8089`;裸跑形态直连 `MGR_BIND`(默认
`:8089`)。mgr-web SPA 与静态 `/api` seam 同 app 模式(见 GET /api/stats 的
路由顺序规则)。所有错误统一 `{"error": "<message>"}` JSON;校验类 400,
内部失败 500(anyhow 链尾)。

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

列表项字段(12): `name/status/live/adopted/created_at/cpus/mem_mb/env/
image/entry_url/piweb_url/services[]`。URL 字段在 payload 里内联生成
(`http://sbx-<name>.mgr.localhost/`),sbx- 前缀与 caddy.rs render 及
composegen 网络别名三方共享同一身份——改前缀必须三处同改。

### 4. Validation & Error Matrix

- name slug: `[a-z0-9-]+`,≤32 字符,字母数字开头;保留 `mgr` 与 `sbx-`
  前缀 → 400。前端 `mgr-web/src/pages/CreatePage.tsx::NAME_RE` 必须与
  `validate_name` 等价(提交前先本地拒)。

### 5. Tests Required

- `caddy.rs` render 单测(站点块存在/删除消失/mgr 静态站优先)锁 URL 形状。

## PUT /api/sandboxes/:name — limits 三态语义

**缺失 = 保留当前值;`>0` = 设置;`0` = 清除(无限制)。** `null` 与缺失
同义(serde Option),所以 UI 的"清除"必须显式发 `0`,不能发 `null`——纯
null 方案下 UI 无法区分"未更改"与"清除"。创建侧对偶:`0`/null 归一化为
无限制。负数 400(`check_limits`)。改 env 走 recreate job(202 + `{job}`),
卷保留。

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
```

(A new per-sandbox proxy route group must be documented here per the seam rule.)

## 6. .trellis/spec/frontend/directory-structure.md — full current text

### web/ tree (lines 1-31) — note STALE

The spec's `web/` tree (lines 4-30) still lists `src/layout.ts` (`:13`) which no longer exists; current layout building lives in App.tsx. The conventions section (lines 21-31):

```markdown
## Conventions

- **One generic pane per service `type`** in `panes/`. A new service is a
  `services.toml` entry - NO new React component unless it needs a new `type`.
  `PaneForService` (in `App.tsx`) dispatches on `service.type`.
- **`types.ts`** owns the manifest contract (`ServiceEntry`, `Manifest`) shared
  with the backend's `/api/manifest` response.
- **`layout.ts`** is the only place that knows golden-layout's config shape;
  `App.tsx` calls `buildLayoutConfig` and stays free of layout details.
- CSS is a single `styles.css` (global + pane classes like `.pane`,
  `.pane-iframe`, `.pane-xterm`); no CSS modules / styled-components yet.
```

### mgr-web section (lines 33-56, full)

```markdown
## mgr-web/ — 第二个 SPA(sandbox-mgr 管理面)

09-08-sandbox-mgr-tui Phase 3。纯管理页面,**无 golden-layout**;页面切换是
App 内内存 state(可辨识联合 view),无路由库。复用 web/ 的三件套模式:

```
mgr-web/src/
├── types.ts            # mgr API payload 单一 owner ↔ mgr/src/routes.rs
├── api.ts              # typed fetch 边界(as 只许在这里出现)
├── i18n.ts             # zh-CN/en flat table(同 web/src/i18n.ts)
├── styles.css          # Kumo token 层自 web/src/styles.css 移植裁剪
│                       # ([data-mode] 深浅色覆盖不变)
├── App.tsx             # shell: 侧边导航 + 主题/语言切换(键前缀 mgr.*)
└── pages/              # SandboxList / Create / Edit(共用 EnvPicker) /
                        # JobView(1.5s 轮询) / Images
```

约定:
- **types.ts 是 mgr-api 契约的唯一前端 owner**(PUT limits 三态语义等注释
  就写在这里,见 backend/api-contracts.md mgr 节)。
- always_on 场景在 EnvPicker 锁定显示(不可取消),且其 id **永远不进**
  `env.scenarios`(后端会拒,canonical env 契约)。
- 构建门同 web/: `npm run build` = `tsc --noEmit && vite build`;镜像经
  mgr/Dockerfile web-builder 阶段(node:20),由 mgr-api 静态服务。
```

(D1/D2 rewrite: "纯管理页面,**无 golden-layout**" becomes false — mgr-web gains golden-layout; the tree gains panes/ + workspace page; the pages inventory line gains Models/Usage/Adopt which are also missing from the spec's list.)

## 7. .trellis/spec/frontend/xterm-pane.md — full current text (65 lines)

```markdown
# Xterm Pane Guidelines

> Contracts for `web/src/panes/XtermPane.tsx` — the generic terminal pane for
> `service.type !== "web"`. Pairs with [component-guidelines.md](./component-guidelines.md)
> (imperative-lib lifecycle pattern).

## Terminal surface contract

```ts
new Terminal({
  fontFamily: "var(--font-mono)", // styles.css token, app mono stack
  fontSize: 13,
  lineHeight: 1.25,               // MUST be explicit — see below
  cursorBlink: true,
  theme: readTermTheme(),         // --term-* tokens via getComputedStyle
})
```

## Convention: always set `lineHeight` explicitly

**Contract**: the `Terminal` options must always include an explicit positive
`lineHeight > 1`. Never omit it and rely on the xterm default of `1.0`.

**Why**: xterm measures the row height from a hidden probe element rendered
with `line-height: normal` (`xterm.css .xterm-char-measure-element`), i.e. the
font's *intrinsic line box* (ascent + descent) — not a fixed multiple of
`fontSize`. The app's mono stack
(`ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono",
"Courier New", monospace` in `--font-mono`) has almost none of these installed
on Linux browsers, so it falls back to the system `monospace` (DejaVu / Noto
Sans Mono), whose intrinsic line height is tight (~1.15–1.2em). At the default
`lineHeight: 1.0` the rows are barely taller than the glyphs, so adjacent lines
visually crowd — the "narrow terminal line spacing" bug.

**Values**:
- `1.25` — project default; comfortable for web terminals.
- Tighter `1.2` / airier `1.35` are the acceptable range.
- Resize is automatic: `fit()` recomputes `rows`/`cols` from `lineHeight`, and
  the pty follows via the 5-byte resize control frame (`[0x01, cols_le, cols_hi,
  rows_le, rows_hi]`, wired in `XtermPane.tsx`), so full-screen TUIs reflow
  correctly at any value.

### Wrong vs Correct

```ts
// Wrong — defaults to lineHeight 1.0: tight, crowded rows on Linux mono fonts
new Terminal({ fontFamily: "var(--font-mono)", fontSize: 13 });

// Correct
new Terminal({ fontFamily: "var(--font-mono)", fontSize: 13, lineHeight: 1.25 });
```

## Verification

- Build gate is enough for the contract: `tsc --noEmit` accepts `lineHeight`
  (xterm 5.x option) and `vite build` bundles it.
- The visual regression signal (crowded rows) is **not** caught by
  `smoke-test.cjs` (it asserts interactions, not pixels). Verify by eye in a
  terminal pane after `make up`, or when swapping the mono stack / font size.

## Related

- `styles.css` `--font-mono` / `--term-*` tokens (surface colors for xterm).
- Terminal pty/resize protocol documented in the `XtermPane.tsx` header comment.
```

(D6 moves XtermPane to mgr-web — the spec's ownership path `web/src/panes/XtermPane.tsx` changes, and the terminal/agent WS route goes through the mgr proxy.)

## 8. Related spec surfaces (noted, not quoted)

- `.trellis/spec/backend/sandbox-mgr-ops.md` 契约 6 (lines 139-163) — mgr.localhost static site + MGR_WEB_DIR; the workspace page will live on this same site (no change needed to the site block itself).
- `.trellis/spec/backend/sandbox-mgr-ops.md` 契约 8 (lines 214-251) — adopt alias three-way identity (sbx-<name> / sbx-<name>-piweb); D6's proxy consumes the same aliases.
- `.trellis/spec/frontend/state-management.md`, `component-guidelines.md` — not read for this note (no explicit request); the imperative-lib lifecycle pattern referenced by xterm-pane.md lives there and applies to the golden-layout port.
