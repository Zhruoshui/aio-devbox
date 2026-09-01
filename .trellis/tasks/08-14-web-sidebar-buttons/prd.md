# Web 端侧边栏按钮化改造(agent TUI 按需启动)

## Goal

把 agent TUI 类产品(opencode 等)从“始终启动的服务面板”降级为“侧边栏按钮按需启动的终端任务”，并把
code-server / chromium / terminal 也统一成左侧可折叠侧边栏里的可点击按钮。让 web 工作台只展示“镜像里确实
装了 / 容器确实在跑”的能力，并支持用户自行注册新的“打开终端 + 跑命令”类按钮。本任务正是补
`.aio/PLAN-l1-versioning.md` “不做(范围外)”里推迟的 “opencode 死面板自动检测” + “L5 外部服务(按需启动)” 两项。

## Background(已核实事实)

### 问题根因(证据)
- `app/services.toml` 是单一事实源，`include_str!` 在编译期烘进 axum(`app/src/config.rs:12`)。当前含 4 个服务：
  codeServer(web)、vnc(web)、terminal(agent)、opencode(agent)。
- agent 面板可见性由 `enable` 字段指名的环境变量决定(`app/src/config.rs:114-122` `is_agent_enabled`，默认 true)。
- `docker-compose.yml:28-33` 把 `ENABLE_OPENCODE=true` 与 `ENABLE_TERMINAL=true` **硬编码**。
- 结果：opencode 已被移出当前构建场景(`.aio/enabled.toml` 现为 python-dev/shell-utils/uv，无 opencode；镜像里无
  `opencode` 二进制)，但 manifest 仍报 `opencode.enabled=true` -> 前端仍尝试打开 opencode pty -> **死面板**。
  记忆 `profile-layering-spike.md` 已记录此耦合；`.aio/PLAN-l1-versioning.md` “不做(范围外)”明确推迟
  “opencode 死面板自动检测” 与 “L5 外部服务”。

### 现有前端(证据，`web/src/`)
- React + golden-layout(^2.6.0)。7 个文件。**全屏一个 GoldenLayout 实例，无任何侧边栏/导航/按钮 chrome**
  (`web/src/App.tsx:77-108`，`web/src/styles.css:19-23` `.gl-root{100vw,100vh}`)。
- 面板只按 `enabled` 过滤(`App.tsx:56`)，不按 type。两类面板组件：`IframePane`(type=web，`src={service.url}`)
  与 `XtermPane`(type=agent，连 `/api/term/ws?cmd=<encodeURIComponent(cmd)>`)。
- 布局：>1 个启用服务 -> 平铺 row；==1 -> 单 stack tab(`web/src/layout.ts:19-38`)。无嵌套列/栈。
- 状态：3 个 `useState`(status/errorMsg/enabledServices)，无 store/context。**关掉的面板无法重开，必须刷新页面。**
- terminal 与 opencode 是**同一个 `XtermPane` 组件**，仅 `cmd` 不同(terminal `""`=登录 shell；opencode `"opencode"`)。

### 现有后端(证据，`app/src/`)
- `GET /api/manifest`(`routes/manifest.rs:11-14`)每请求调 `build_manifest`(`config.rs:76-96`)：
  - type=web：`is_web_reachable` 对 `target` host:port 做 400ms TCP 探活(`config.rs:101-110`)。code-server/vnc
    可见性已由此自动反映 compose profile(容器不在 -> 探活失败 -> 隐藏)。
  - type=agent：`is_agent_enabled` 读 `enable` 指名 env(默认 true)。
- **终端 WS 完全通用**：`GET /api/term/ws?cmd=<任意串>`(`routes/terminal.rs:55-63`)。`pty.rs:65-81`：空/缺 `cmd`
  -> `/bin/bash -l`；非空 -> `/bin/bash -l -c <cmd>`。**无白名单**--manifest 的 `cmd` 字段只是给前端的提示，
  WS 端不查 services.toml。前端可直接 `?cmd=opencode`/`?cmd=htop`/`?cmd=任意`，无需改后端。
- opencode **无专属路由**，就是 `services.toml` 里一条 `cmd=opencode` 的 agent 记录走通用 pty。
- `/api` `/v1` `/mcp` 是 502 stub(`routes/seam.rs`)；`/api/*rest` catch-all 在 `manifest`/`term/ws` 静态路由之后，
  新增 `/api/buttons` 静态路由会自然优先，**无需改 gateway**(`gateway/Caddyfile` catch-all `handle{reverse_proxy app:8088}`
  已把 /api/* 透传到 app)。
- React dist 烘进镜像 `/app/static`(`app/Dockerfile:55-65`)，axum `ServeDir`+index.html fallback(`main.rs:44-57`)。
- **app 运行时 `USER gem`(uid 1000)**(`app/Dockerfile` 末段)，workspace 卷挂载在 `/home/gem`，故 axum 进程对
  `/home/gem/.aio/buttons.toml` 可读可写(同属 gem)。`Cargo.toml` 已有 tokio(full)/toml 0.8/serde/axum(ws)，
  无 `which` crate(探测自行实现，见 design)。

### 场景分层(证据，`config/src/scenario.rs`)
- `category` 自由串，已知层：`os`(L1)/`shell`(L2)/`lang`(L3)/`app`(L4)/`service`(L5 future)。
  `opencode/scenario.toml` 现为 `category="app"`，其 TUI 分组标题即 “L4 · 应用 / AI agent”(已含 AI agent)。
  > 注：用户口头“已移到 L5”指的 web 端按需启动层(本任务)，非 scenario category；故 opencode category 维持
  > `app`(“service/L5·外部服务”对 CLI agent 是误称)。见 Decisions。

## Requirements

- R1 agent/TUI 按钮(opencode 等)**按需启动**：点击 -> 在主区开一个 xterm 面板并以该按钮配置的命令启动
  (复用通用 `/api/term/ws?cmd=`)；不再作为常驻面板自动打开。
- R2 左侧**可折叠侧边栏**，统一放置所有服务按钮(code-server / chromium / terminal / 各 agent TUI / 用户自注册)。
- R2a **主区 = 标签页栈**：多个面板同时开着、以 tab 共存，点 tab 切换当前活动面板、点侧边栏按钮 toggle 该面板
  的展开/收起。当前活动 tab 独占主区(不被挤窄，适合全屏 TUI 与 iframe)。
- R2b **移除 golden-layout**：标签栈自写轻量实现，不再依赖 golden-layout 的 row/stack/drag(从 `web/package.json`
  移除依赖；`web/src/layout.ts` 删除)。
- R2c **每按钮单 tab · toggle**：每个按钮至多对应一个 tab；点已展开按钮 -> 收起(关闭 tab)；再点 -> 重开(新会话)。
  关 tab 即关 WS -> pty 子进程退出(非 detach)；重开是新会话。code-server/vnc/terminal/opencode 各一个 tab。
- R3 按钮可见性 = 能力真实存在：
  - web 按钮(code-server/vnc)：沿用 TCP 探活(已具备)。
  - agent/TUI 按钮：改为运行时探测二进制是否存在(command_exists)，替代硬编码 `ENABLE_*`。仅当装了该命令，
    按钮才出现；杜绝死面板。terminal(`cmd=""`)恒可见(bash 必在)。
- R3a **探测实现**：用登录 shell 解析出的 PATH(一次 `bash -lc 'printf %s "$PATH"'`，结果缓存 + TTL)在进程内遍历
  判定可执行文件是否存在(覆盖 `/usr/local/bin` 烘进工具 + `~/.local/bin` 运行时补装工具，与 pty 登录 shell 解析一致)。
  `is_agent_enabled` 读 `ENABLE_*` env 的旧机制废弃；`docker-compose.yml` 里 `ENABLE_OPENCODE/ENABLE_TERMINAL` 硬编码
  移除。缓存细节见 design.md。
- R4 terminal 按钮默认打开(主区默认有一个登录 shell xterm 面板)；可收起/再展开。
- R5 按钮注册入口：用户可自行注册“打开终端 + 跑命令”类按钮；启动命令可配置。
- R5a **存储/注册机制**：用户按钮存 `/home/gem/.aio/buttons.toml`(持久卷，跨重建存活、多端共享)。axum 运行时读
  并并入 manifest(与烘进编译期的 services.toml 合并)。侧边栏底部“+ 注册按钮”表单(名字+命令) -> `POST /api/buttons`
  写文件(配套 `DELETE /api/buttons/:id` 删除)。内置按钮(code-server/vnc/terminal/opencode)仍由 `services.toml`
  烘进，不进 buttons.toml。文件缺失时 axum 视为空列表(首次 `make up` 空卷场景)。
- R5b **注册范围**：MVP 仅 agent/TUI 型(名字+命令，点击开 xterm 跑命令)。buttons.toml 的 `type` 字段保留(默认
  `agent`)，未来加 web 型零改 schema(自定义 web 需端口预览/跨容器访问，已判 OOS)。
- R6 code-server / chromium 按钮“镜像构建时未代入则无按钮”：web 侧已由 TCP 探活天然满足，无需额外改动。

## Decisions(访谈决议)

| # | 决策 | 选项 | 落点 |
|---|------|------|------|
| Q1 | 主区交互模型 | ~~标签页栈~~ → **修订(2026-08-14 用户实测后)**：默认单页 tab 栈，但 tab 可拖拽拆分/平铺(一页与平铺都支持) | R2a' |
| Q4 | 是否移除 golden-layout | ~~移除~~ → **修订**：恢复 golden-layout(拖拽平铺是其原生能力，自写成本过高) | R2b' |
| Q(toggle) | 按钮点击语义 | ~~单 tab · toggle~~ → **修订**：**启动器**——点击创建新实例，再点再创建一个(可多开)；关闭走 tab 上的 ✕ | R2c' |
| Q2 | 自注册按钮存储/注册 | 卷上 `buttons.toml` + UI 表单 `POST /api/buttons` | R5a |
| Q3 | agent 可见性探测 | command_exists(登录 shell PATH 遍历) | R3a |
| Q5 | 自注册范围 | MVP 仅 agent/TUI(schema 留 type) | R5b |
| Q6 | opencode scenario category | 维持 `app`(L4)，“L5”= web 端按需层 | Background 注 |

### 修订 R2a'/R2b'/R2c'(用户实测第一版后提出)

- R2a' 主区默认单页 tab 栈；tab 头可拖拽重排、拖出拆分成分屏/平铺(golden-layout
  原生 row/column/stack)。布局不跨刷新持久化(未要求，OOS)。
- R2b' 恢复 golden-layout ^2.6.0 依赖与集成(组件工厂 + createRoot + beforeComponentRelease
  清理 + iframe 拖拽 overlay)。
- R2c' 侧边栏按钮 = 纯启动器：每次点击新建一个实例(第 n 个实例 tab 标题加 `(n)`)；
  关闭仅通过 tab 的 ✕(golden-layout 原生 close)，关即卸载面板 -> 杀 pty。按钮无
  toggle/高亮态。terminal 初始默认开一个实例不变。

> 修订已验证(2026-08-14)：冒烟测试全绿(sidebarOk/terminalDefaultOk/launcherOk/
> iframesOk/closeOk/userCmdRan/deletedOk)。实施备注：golden-layout 2.6 的 tab 关闭
> 图标是 `.lm_close_tab`(在 `.lm_tab` 内)；`.lm_close` 是 header 级整栈关闭控件，
> 选择器不可混用。另：用户测试期间已自行把 opencode 场景烘进 sandbox-base，当前
> 镜像 opencode 按钮真实可见(符合预期)。

## Assumptions(默认值，如不符请纠正)

- 侧边栏默认展开；折叠状态存浏览器 localStorage；折叠时收成图标轨(按钮仍可点)。
- 主区 tab 栏每个 tab 有 `[x]` 关闭(等价于 toggle 该按钮收起)；无拖拽/resize(golden-layout 移除后不需要)。
- manifest 刷新：侧边栏头部一个“↻”手动刷新按钮 + 窗口重新获焦时自动重取；**不设定时轮询**(运行时补装工具是
  低频动作，手动刷新足够)。新出现的按钮注入侧边栏，不影响已开 tab。
- buttons.toml 路径硬编码 `/home/gem/.aio/buttons.toml`(可由 env `AIO_BUTTONS_FILE` 覆盖)；目录不存在则 axum
  创建(以 gem 身份)。
- terminal 按钮的 `cmd` 仍为 `""`(登录 shell)；opencode 内置按钮 `cmd="opencode"`，两者仍走 services.toml。
- `services.toml` 的 agent 记录移除 `enable` 字段(改由 command_exists 决定)；web 记录不变。

## Acceptance Criteria

- [x] AC1 `.aio/enabled.toml` 现状(不含 opencode)构建并 `make up` 后，侧边栏**不出现** opencode 按钮(command_exists
  探测 opencode 二进制失败)；无死面板、无前端尝试连 opencode pty 的报错。(冒烟测试 sidebarOk，2026-08-14)
- [x] AC2 镜像含 opencode 二进制时，侧边栏**出现** opencode 按钮；点击在主区开 xterm 并运行；再点收起。
  (以容器内 `/usr/local/bin/opencode` mock 验证——command_exists 探测路径与真实烘进完全等价；
  定向 puppeteer 断言 opencodeLaunched=true、tabs=[Terminal, opencode]，验证后已清理 mock)
- [x] AC3 code-server/vnc 未起 profile 时侧边栏无对应按钮；`make up PROFILES=code-server,vnc` 后按钮出现并可
  展开/收起(TCP 探活)。(冒烟测试 iframesOk：code-server workbench + vnc frame 加载)
- [x] AC4 terminal 按钮默认打开一个登录 shell xterm tab；可收起、可再展开为新会话。(terminalDefaultOk + toggleOk)
- [x] AC5 侧边栏“+ 注册按钮”注册后按钮立即出现；点击开 xterm 跑该命令；`make down && up` 重建容器后按钮仍在
  (卷上 buttons.toml 持久)。(userCmdRan + survivor 按钮 down/up 后存活)
- [x] AC6 注册的按钮可删除(`DELETE /api/buttons/:id`)，buttons.toml 同步移除。(deletedOk；404 路径亦验证)
- [x] AC7 运行时在容器内补装工具到 `~/.local/bin` 后，刷新 manifest 对应按钮出现。(demotool 注册即 enabled=True)
- [x] AC8 golden-layout 依赖从 `web/package.json` 移除，`npm run build` 通过且无残留引用。(grep 验证)
- [x] AC9 `docker-compose.yml` 不再含 `ENABLE_OPENCODE/ENABLE_TERMINAL`；app 正常起、manifest 正常返回。(grep 验证)

## Out of Scope

- SDK / agent API(`/api` `/v1` `/mcp` 仍 502 stub)。
- 跨容器端口预览、TLS、多用户。
- 自定义 web 型按钮(需端口预览/跨容器访问)。
- 同类型按钮多开(终端多实例)、tab 拖拽/resize、面板布局持久化。
- scenario category 重分类(opencode 维持 `app`)。
- opencode 死面板“自动检测”之外的更深层 L5 服务编排(仅做可见性探测 + 按需启动)。
