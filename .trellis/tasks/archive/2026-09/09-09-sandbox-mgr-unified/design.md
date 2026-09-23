# Design: Sandbox 管理器统一聚合界面改版

对应 prd.md 决策 D1-D8。研究锚点见 `research/`（workbench-frontend / mgr-backend / model-sync-chain / app-static-and-manifest / mgrweb-frontend / contracts-quote）。

## 0. 总体架构

```
浏览器 ── mgr.localhost:80 (mgr-gateway/caddy, 唯一宿主端口)
            │
            ├─ mgr-web SPA (golden-layout 工作区 + 管理页, 唯一 UI)
            │     │ iframe: code-server/vnc → http://sbx-<name>.mgr.localhost/...
            │     │ iframe: pi-web → http://sbx-<name>-piweb.mgr.localhost/
            │     └─ fetch/WS: 终端/按钮/manifest → mgr-api 代理
            │
            └─ mgr-api:8089 (axum)
                  ├─ 既有控制面 API (sandboxes/models/usage/...)
                  ├─ 新: /api/sbx/:name/*path → http://sbx-<name>-piweb:8088/*path
                  │     (HTTP 流式 + WebSocket 透传, D6/D7)
                  └─ 新: code-server 按需拉起 (D4)
                        │
                        ├─ sbx-alpha-piweb:8088 (app, aio-mgr-net 别名)
                        │     ├─ /api/term/ws (pty, 经代理)
                        │     ├─ /api/manifest /api/buttons* (经代理)
                        │     └─ /preview/:port/* (用户 web 按钮, 经代理)
                        └─ 其余容器拓扑不变 (gateway/vnc 常驻, code-server 按需)
```

不变量（本设计不动）：零宿主端口 + 子域名路由（契约 9）、总网关 Host 改写（契约 3）、别名三方一致（契约 8）、Caddyfile 原地写（契约 2）、compose CLI 而非 Docker API（D2 原决策）。

## 1. mgr-api 沙箱代理（D6/D7 的地基）—— 新模块 `mgr/src/proxy.rs`

**路由**：`/api/sbx/:name/*path`（any-method），merged 在 `/api` seam 之前（routes.rs:46-47 同 models/usage 的合并模式；`/api/sbx` 静态段胜出 `/api/*rest` catch-all，与 app `/api/manifest` 先例同机制）。

**转发规则**：
- 上游 = `http://sbx-{name}-piweb:8088/{path 原样}`（别名与 usage.rs:58-59 扇出同源——app 的 aio-mgr-net 别名，不是 gateway 别名 `sbx-<name>`）。
- `:name` 必须存在于 sandboxes 表（require_row 模式，routes.rs:595-599）；**宿主永远由 name 派生，绝不由请求参数决定**——代理是封闭的，不存在 SSRF 面。
- 不做存活过滤：stopped 沙箱的连接失败原样返回 502（mgr-web 侧按 D6 用 live 状态置灰按钮，代理不重复判断）。
- 路径透传示例：`/api/sbx/alpha/api/manifest` → `app:8088/api/manifest`；`/api/sbx/alpha/preview/3000/x` → `/preview/3000/x`；`/api/sbx/alpha/api/term/ws` → WS 透传。

**实现**：以 `app/src/routes/preview.rs` 为模板（同仓 axum 0.7 HTTP+WS 代理先例）：
- HTTP：剥 hop-by-hop 头（preview.rs:59-65），reqwest `bytes_stream()` → `Body::from_stream`（preview.rs:157-160）。
- WS：`Option<WebSocketUpgrade>` 探测 Upgrade 头（preview.rs:80-83），tokio-tungstenite 明文 `connect_async` 2s 握手超时，回传协商的 `Sec-WebSocket-Protocol`，双向消息泵（preview.rs:220-271）。

**依赖变更（mgr/Cargo.toml，契约 5 的 dep-cache 覆盖无需改 Dockerfile，但须重生成 Cargo.lock）**：
- `axum` 加 `features = ["ws"]`（当前 default only）。
- `reqwest` 加 `"stream"` feature（当前仅 json+rustls-tls）。
- 新增 `tokio-tungstenite = "0.23"`（lock 里已有，app 在用）。

**安全边界（并入契约 9 的扩展条目）**：此路由把每个沙箱 app:8088 的全部 API（**含 pty 全 shell 的 /api/term/ws**）暴露到 mgr.localhost 无认证边界——与现状"任何能访问 mgr.localhost 的人可通过子域名直达各沙箱"等价，无新增暴露面；但契约 9 的"三处同步加认证"清单需加入第四处：mgr-api 代理路由。

**测试**：路由合并顺序（seam 不吞 `/api/sbx/...`）、name 不存在 → 404 `{"error":...}`、host 派生不可注入（name 校验复用 validate_name）。

## 2. mgr-web 工作区（D1/D2/D3/D5/D6 的 UI 面）

### 2.1 页面结构

- `App.tsx` Page union 增 `"workspace"`，nav 增第五项"工作区"，**设为默认落地页**（沙箱管理器现在是主工作界面；管理页沙箱/镜像/模型/用量保留在 nav 下方）。
- 新 `pages/WorkspacePage.tsx`：左侧沙箱树 + 右侧 golden-layout。App 只保留全局 chrome（主题/语言），工作区状态自持。
- golden-layout 依赖进 mgr-web：`golden-layout ^2.6.0`、`@xterm/xterm ^5.5.0`、`@xterm/addon-fit ^0.10.0`（版本对齐 web/ 锁定值）；CSS 搬 `gl-kumo.css`（278 行）+ web/src/styles.css 被裁剪的 pane/term/--term-* 段；`main.tsx` 加 no-StrictMode（web/src/main.tsx:9-11 同理由：golden-layout 命令式库，双调用 effect 会造出两个实例）。

### 2.2 沙箱树（新 `pages/workspace/SandboxTree.tsx`）

- 数据：`listSandboxes()`（已有 4s 轮询复用）+ 展开时懒加载 `GET /api/sbx/:name/api/manifest`（首次展开/手动刷新/pane 打开时拉取，不做全量常驻轮询）。
- 树节点 = 沙箱（live 状态角标，stopped 时子按钮整体置灰 + 节点行内"启动"按钮调既有 `POST /api/sandboxes/:name/start`）。子按钮 = 该沙箱 manifest 条目（内置 code-server/vnc/terminal/opencode/pi/piWeb + 自定义按钮，分组沿用 web/Sidebar 的 web/tui/custom 语义压平为一级列表）。
- 右键/尾随按钮：注册自定义按钮（弹 RegisterDialog，D7）、模型 profile 指派快捷入口（跳 EditPage）。

### 2.3 golden-layout 移植（App.tsx → WorkspacePage.tsx）

整体搬 web/src/App.tsx 的机制，改名换键：
- componentState = `{ service, sandbox: string, seq?: number }`；title = `<label>@<name>`（i18n 里 label 已有 zh/en）。
- 布局持久化键 `mgr.layout`（localStorage；popout 子窗口父+子同 origin 于 mgr.localhost，localStorage 互通机制原样成立）。
- 保留：单工厂注册 + React root per container + `beforeComponentRelease` 清理、seq 池（getSeqPool/collectInUseSeq）、`headerHeight: 40` 恢复、500ms debounce 保存、popout `consumeSubWindowLayout` 旁路 + `window.__glInstance`、iframe 拖拽遮罩（is-dragging class + drag-overlay）、tab glyph MutationObserver。
- 默认布局（无存档时）：单 stack 单终端 pane——终端选哪个沙箱？默认打开**第一个 running 沙箱的终端**；无 running 沙箱则空态提示"创建或启动一个沙箱"。

### 2.4 Pane URL/WS 解析（origin 解耦的核心改动）

新 `pages/workspace/paneUrl.ts`，`urlFor(service, sandbox): string`：

| service | URL | 依据 |
|---|---|---|
| codeServer / vnc（相对 url） | `http://sbx-<name>.mgr.localhost` + service.url | 沙箱自己的 gateway 剥前缀反代（D3/D4，现有路由零改动） |
| piWeb（绝对 url） | service.url 原样（= PI_WEB_URL 覆盖后的子域名） | 契约 3 |
| 用户 web 按钮（`/preview/<port>/`） | `/api/sbx/<name>/preview/<port>/`（**相对 mgr.localhost 同源**） | 经 §1 代理；同源避开 iframe 混合内容与将来认证 cookie |
| terminal / agent（XtermPane） | WS `ws(s)://<mgr origin>/api/sbx/<name>/api/term/ws?cmd=...` | §1 代理透传 |

XtermPane/IframePane 组件本体从 web/src/panes/ 平移：XtermPane 仅改 `buildWsUrl` 为接收 `(name, cmd)`；IframePane 仅改 `src` 计算调 `paneUrl.urlFor`。xterm-pane.md 的 lineHeight=1.25 契约、5 字节 resize 帧、MAX_RECONNECT=1、close-kills-pty 语义原样保留（规格文件路径改指 mgr-web）。

### 2.5 code-server pane 的按需启动（D4 前端侧）

打开 code-server pane 时：先渲染"启动中"占位 → 调 `POST /api/sandboxes/:name/service/code-server/start`（§3）→ 轮询 TCP 探测（`GET /api/sbx/:name/api/buttons/probe?port=8200`，复用现有 probe 端点，app 网关内 app:8200 可达即服务就绪）→ iframe。若沙箱 stopped 直接置灰不可点（树层已拦截）。pane 关闭**不**停 code-server（保留会话/未保存状态；显式停 = 沙箱 stop）。存量/adopt 栈 code-server 可能本就在跑：pane 打开时先探测，已可达则跳过 start 直接 iframe。

## 3. code-server 按需实例（D4 后端）

### 3.1 docker.rs 变更

- `SANDBOX_PROFILES` 拆分：`const UP_PROFILES = ["--profile","vnc"]`（up 只带 vnc——pi agent-browser 硬依赖常驻），stop/restart/down/ps 保持全量 `SANDBOX_PROFILES`（**down/stop 必须仍能看到 code-server 才能拆干净/停干净**，契约 4 的后半句保留）。
- 新函数 `compose_service_up(project, file, profiles, service)`：`docker compose -p <p> -f <f> --profile code-server up -d code-server`（用 `up -d <svc>` 而非 `start <svc>`：profile 门控服务未被 up 创建过时 `start` 找不到容器）。
- 首次打开的启动延迟 ≈ 容器 create+start（镜像已构建，秒级）。

### 3.2 路由

`POST /api/sandboxes/:name/service/:service/start`，`service ∈ {code-server}` 白名单（预留扩展）；adopted 栈也可用（compose_up_file 同理拆 profile；adopt 默认 gateway/app 两服务，若其 compose 里有 code-server profile 同样能拉）。同步接口（不走 job，秒级操作），404 未知服务/沙箱。

### 3.3 契约 4 改写要点

"up 必须全起"改为：**up 带 vnc（常驻依赖）不带 code-server（按需）；down/stop/restart 带全量 profile**。验证点从"4 容器 running"改为"3 容器 running（app/gateway/vnc）+ code-server 在按需 start 后可达"。

## 4. 模型配置多 profile + 每沙箱指派（D8）

### 4.1 数据层（mgr kv，无 schema 迁移负担）

- 新键 `models_profiles`，值：`{ "version": u64, "profiles": [ { "id": string, "name": string, "config": CanonicalConfig } ], "assignments": { "<sandbox-name>": "<profile-id>" } }`。
- **迁移**：启动读到旧键 `models_config` → 转为 `id: "default"` profile，**所有现存沙箱指派到它**（保持现行为零变化），写新键删旧键。aio-models crate 本身零改动（profile 就是一整份 CanonicalConfig，包装层变化）。
- profile id 复用 `gen_preset_id` 风格 slug；version 为全局递增（内容深比较已容忍 version 漂移，mgr_sync.rs:26-28——无需 per-profile version 的比较成本优化）。

### 4.2 API（mgr/src/models.rs 扩展）

| 路由 | 行为 |
|---|---|
| `GET /api/models/profiles` | 列表：`[{id, name, version, assigned: [names]}]`（不携 config） |
| `POST /api/models/profiles` | `{name}` → 建（空 default config） |
| `PUT /api/models/profiles/:id` | 整份 masked-echo 编辑（复用 merge_api_keys/validate 流水线） |
| `DELETE /api/models/profiles/:id` | 拒绝最后一个 profile；解绑其 assignments |
| `PUT /api/sandboxes/:name/model_profile` | `{profile: id \| null}` 指派/解绑。**独立于 PUT /api/sandboxes/:name**——env 改动走 recreate job，profile 指派是纯 kv 写 + 等沙箱下轮拉取，绝不触发 recreate。sandbox_json 增 `model_profile` 字段 |
| `GET /api/models/sync?name=<sandbox>` | 返回所指派 profile 的**未 mask** `{version, config}`；**未指派 → 404**（app 侧按"无指派，保持本地"静默处理，见 4.4）；name 不在表 → 404。**sync 从此必须带 name**（现契约 7 的无身份拉取是本设计唯一的破坏性变更） |
| import_pi / discover / test | 加 `?profile=`（或路径）作用域；catalog 全局不变 |

### 4.3 沙箱侧身份注入（同步链唯一新增契约）

- composegen.rs:91 旁增注 `MGR_SANDBOX_NAME: <name>`（与 MGR_URL 同块）。adopted 栈无 MGR_URL → 不参与，行为同现状（本地自治模型配置）。
- app `mgr_sync.rs`：请求 URL 追加 `?name={MGR_SANDBOX_NAME}`；**404 响应 = 未指派**，降级为一条 `debug` 日志 + 保持本地（区别于现有的 transport 失败 `warn` + 保持本地——两者都不写不退）；其余逻辑（60s、深比较、overwrite_and_render、models_lock）零改动，因为返回的仍是"该沙箱的那份" CanonicalConfig。
- 双向 wire-shape 测试对（mgr `sync_handler_shape_*` / app `sync_payload_decodes_*`）同步更新为带 name 的形状。

### 4.4 mgr-web

- ModelsPage：页头加 profile 下拉 + 新建/重命名/删除；所有既有 tab（providers/pi/opencode/claude/codex）作用于所选 profile；MgrNotice 的"去沙箱工作台"链接改为"去工作区"。
- EditPage：resources field-row（EditPage.tsx:146-172）与 dialog-actions（:174）之间加 profile 指派 select（三态语义：未变不发 / id 指派 / 显式 null 解绑，对齐 limits 三态的文档风格进 types.ts）。
- SandboxListPage 卡片 sbx-meta 显示所指派 profile 名；"进入沙箱"按钮（:219-229）改为切换到工作区并聚焦该沙箱（App 回调 `goWorkspace(name)`）。
- UsagePage：行数据本就按沙箱聚合，不加 profile 维度（不必要）。

## 5. web/ 退役与 app 静态替换（D1）

- **删除**：`web/` 整目录（含 smoke-test.cjs、panes/models/ 拷贝）。`app/services.toml` 删 `modelsConfig` page 条目（沙箱内模型 UI 随之消失）；其余条目保留（manifest 经代理仍是 mgr-web 树的数据源）。
- **替换**：`app/Dockerfile:69-77` web-builder 阶段删除，改为 `COPY app/redirect/ /app/static`；`app/redirect/index.html` = 极简跳转页：`MGR_URL` 设置时由 entrypoint 注入跳转目标（`sed` 占位符或 env 读取脚本），未设置（stock/adopt 前的裸栈）则显示静态说明"工作台已统一到 Sandbox 管理器 + 链接 http://mgr.localhost/"。`/api` seam 与全部 API 路由不动（main.rs:176-185）。
- **连带清理**：Makefile/CI 引用 web/ 的 target 审计（research/app-static-and-manifest.md §5 遗留项）；`docker-compose.yml` stock 栈 app 服务照旧（其静态页变为 redirect，功能走 adopt 后的 mgr-web）。
- **顺序约束**：先完成 §2 工作区移植并验证，再删 web/（回滚点见 implement.md）。

## 6. 规格更新（Phase 3.3 清单）

| 文件 | 变更 |
|---|---|
| sandbox-mgr-ops.md 契约 4 | §3.3 改写 |
| sandbox-mgr-ops.md 契约 7 | kv 形状、sync 带 name、404=未指派语义、指派端点；"唯一真相源"加"per-profile"维度 |
| sandbox-mgr-ops.md 契约 9 | 加认证第四处：mgr-api 代理路由（pty 全 shell 面） |
| sandbox-mgr-ops.md 新契约 10 | `/api/sbx/:name/*` 代理：宿主由 name 派生封闭性、alias `sbx-<name>-piweb`、adopted 同别名、HTTP+WS 透传 |
| api-contracts.md mgr 节 | 代理路由组 + `PUT /:name/model_profile` + service start + sandbox_json 新字段（seam 规则注明注册在 seam 前） |
| frontend/directory-structure.md | mgr-web 节重写（"无 golden-layout"作废；补 panes/ + workspace 页 + Models/Usage/Adopt 页）；web/ 节删除或改为墓碑注记 |
| frontend/xterm-pane.md | 路径 `web/src/panes/XtermPane.tsx` → `mgr-web/src/pages/workspace/panes/XtermPane.tsx`；WS 路径契约更新 |

## 7. 权衡与风险

| 风险 | 缓解 |
|---|---|
| golden-layout 移植是最大单块前端工作（App.tsx ~600 行机制） | 机制整体平移不重写；pane 组件逐个搬；工作区独立成页不与管理页纠缠 |
| 代理 pty WS 无认证（全 shell 暴露） | 与现状子域名直通等价（契约 9 信任边界=宿主机）；规格显式记录第四处同步点 |
| 契约 7 sync 加 name 是破坏性变更 | 迁移把全部现存沙箱指派 default → 行为零变化；旧 app（adopt 栈）无 MGR_URL 不受影响；唯一受影响 = mgr 自己生成的新栈（同时升级） |
| 删 web/ 后 stock 栈裸跑无 UI | adopt 纳管即恢复；redirect 页指路。已与用户确认（D1） |
| code-server 按需启动首开延迟 | 镜像已构建，仅容器 create+start（秒级）；"启动中"占位 UI |
| mgr Cargo 新增 3 依赖 | 全部在 lock 中已有（app 在用）；契约 5 dep-cache 覆盖，无 Dockerfile 改动，需 `--locked` 重建 |

## 8. 回滚

- 各 Phase 独立提交（见 implement.md）；工作区页合入前 web/ 完好——D1 删除是最后一步，之前任意回退 = mgr-web 不加"工作区"入口即可。
- kv 迁移（§4.1）保留旧键内容直至新键写入成功；回滚版本读到新键缺失 → 读旧键，双向兼容。
