# Implement - Web 端侧边栏按钮化改造

## 完成状态(2026-08-14)

全部完成并验证：A1-A7、B1-B7 落地(cargo test 11/11、clippy -D warnings 干净、npm run build 通过)；
C0-C7 整栈验证通过(puppeteer E2E smoke ok=true 两轮：无 opencode 基线 + opencode 在场；survivor 按钮
down/up 存活；mock opencode 点击启动断言通过)。实施中发现并修复一个设计缺陷：command_exists 原按整条
cmd 串探测，带参数命令(如 `ls -la`)必失败——改为只探测首 token(可执行文件)。README 双语已同步。

> 工作流：**inline**(不派发子代理，直接实现)。故跳过 implement.jsonl/check.jsonl 的 JSONL 门禁。
> 顺序：后端先(可独立 `cargo test`/`curl` 验)，再前端(可独立 `npm run build` 验)，最后整栈 `make up` 跑 AC。

## A. 后端(`app/`)

- [ ] A1 `app/src/config.rs`
  - `Service`：删 `enable: Option<String>`；增 `label: Option<String>`、`#[serde(default)] deletable: bool`。
  - `ManifestEntry`：增 `label: String`、`deletable: bool`。
  - 删 `is_agent_enabled`；`build_manifest` 签名改 `async fn build_manifest(services: &[Service], cache: &PathCache)`，
    agent 分支用 `command_exists(svc.cmd.as_deref().unwrap_or(""), cache)`；web 不变。
  - 新增 `PathCache` 结构 + `resolve_login_path()`(bash -lc 'printf %s "$PATH"'，失败回退 std env PATH，TTL 60s) +
    `command_exists(cmd, cache)`(空 cmd => true；否则遍历缓存 dirs 查 is_file)。
  - 新增 `pub fn load_buttons(path: &Path) -> Vec<Service>`：解析 `[[button]]`，缺/空文件 => 空 vec，解析错 => 空 vec+warn；
    每条 `deletable=true`、`service_type=Agent`、`cmd` 取 button.cmd。
  - agent 探测并行化(`futures::future::join_all` over services，web TCP 探活也并入同一并行批次)，避免串行 400ms 累加。
- [ ] A2 `app/src/state.rs`：`AppState` 改为 struct{ `builtin: Arc<Vec<Service>>`, `buttons_file: PathBuf`,
      `path_cache: Arc<RwLock<PathCache>>`, `file_lock: Arc<Mutex<()>>` }；`new(builtin, buttons_file)` 初始化空缓存。
- [ ] A3 `app/src/routes/buttons.rs`(新)：`POST /api/buttons`(校验 label/cmd -> 锁 -> read-modify-write 原子 rename ->
      201 + entry)、`DELETE /api/buttons/:id`(锁 -> 移除 -> 204/404)。id = slug(label)，冲突加 `-2`/`-3`。
- [ ] A4 `app/src/routes/manifest.rs`：handler 合并 `state.builtin` + `load_buttons(state.buttons_file)`(同 id 内置优先，
      用户重复 id warn 跳过) -> `build_manifest(&merged, &state.path_cache).await`。
- [ ] A5 `app/src/main.rs`：构造新 AppState(`load_services()`、`buttons_file` 从 env `AIO_BUTTONS_FILE` 或
      `/home/gem/.aio/buttons.toml`)；router 增 `.route("/api/buttons", post(create_button))`
      `.route("/api/buttons/:id", delete(delete_button))`(置于 `/api/*rest` catch-all 前)。
- [ ] A6 `app/services.toml`：agent 记录删 `enable`；4 条都加 `label`(code-server / Chromium / Terminal / opencode)。
- [ ] A7 `docker-compose.yml`：app.environment 删 `ENABLE_TERMINAL`/`ENABLE_OPENCODE` 及其注释。

**A 验证**：
- `cd app && cargo test`(+ 新增单测：load_buttons 空/缺/解析、command_exists 空 cmd => true、label 回退、id 去重)。
- `cd app && cargo clippy -- -D warnings`。
- `cargo build --release` 通过。

## B. 前端(`web/`)

- [ ] B1 `web/src/types.ts`：`ServiceEntry` 增 `label: string`、`deletable: boolean`。
- [ ] B2 `web/src/App.tsx` 重写：fetch manifest；state(manifest / openTabs:string[] / activeTab / collapsed)；
      terminal 若 enabled 首次默认 open；渲染 `<Sidebar>` + `<TabStack>`；`PaneForService` 选 Iframe/Xterm；
      `toggleTab(id)`、`closeTab(id)`、`refresh()`、`registerButton(label,cmd)`。
- [ ] B3 `web/src/Sidebar.tsx`(新)：列 `manifest.filter(enabled)`，toggle 按钮(active 高亮)；顶部 ☰ 折叠 + ↻ 刷新；
      底部 “+ 注册按钮” 表单(label/cmd -> POST /api/buttons -> refresh)；用户按钮(deletable)悬停 ✕(DELETE -> refresh)。
- [ ] B4 `web/src/TabStack.tsx`(新)：tab 栏(openTabs 顺序，label + ✕ 关，点切 active) + 主区 active 面板。
- [ ] B5 `web/src/panes/XtermPane.tsx`：保留 WS/resize 逻辑；把 `:97` 过时 “Phase E” 文案改中性(如 “● Terminal disconnected.”)；
      确认 cleanup(`:111-116`) 关 WS+dispose 不变(toggle 关 tab 走此路径杀 pty)。
- [ ] B6 删 `web/src/layout.ts`；`web/src/main.tsx` 去掉 golden-layout CSS import；`web/src/styles.css` 重写为
      sidebar(折叠 ~48px/展开 ~200px) + tab 栈 + 主区 flex:1。
- [ ] B7 `web/package.json`：移除 `golden-layout` 依赖(保留 @xterm/* 等)。

**B 验证**：
- `cd web && npm ci && npm run build`(`tsc --noEmit && vite build` 通过；无 golden-layout 残留引用)。
- `grep -rn golden-layout web/src web/package.json` 应空。

## C. 整栈验证(AC1-AC9)

> 注意(记忆 `compose-up-build-skips-running`)：改 Dockerfile/services.toml/web 后，`make up` 不重建运行中的 app。
> 须先 `make down`(或显式 build + `--force-recreate`)。

- [ ] C0 基线(当前 enabled.toml 无 opencode)：`make down && make up` -> 浏览器 :8080 -> 侧边栏无 opencode 按钮(AC1)；
      terminal 默认开 tab(AC4)；code-server/vnc 无按钮(AC3 前半)。
- [ ] C1 `make up PROFILES=code-server,vnc` -> code-server/Chromium 按钮出现，可展开/收起(AC3 后半)。
- [ ] C2 注册自定义按钮：侧边栏 “+ 注册按钮” 填 htop/htop -> 按钮出现 -> 点击开 xterm 跑 htop；再点收起(AC5)。
- [ ] C3 删除按钮：用户按钮 ✕ -> 消失；`docker exec aio-app-1 cat /home/gem/.aio/buttons.toml` 确认已移除(AC6)。
- [ ] C4 重建持久：`make down && make up` -> 自定义按钮仍在(AC5 后半)。
- [ ] C5 运行时补装探测：`docker exec aio-app-1 bash -lc 'install -m755 /dev/stdin ~/.local/bin/demo <<<"#!/bin/sh\necho hi"'` ->
      注册一个 cmd=demo 的按钮 -> 应 enabled(command_exists 命中 ~/.local/bin) -> 点 ↻ 刷新出现(AC7)。
- [ ] C6 `grep -n ENABLE docker-compose.yml` 空(AC9)；`grep -n golden-layout web/package.json` 空(AC8)。
- [ ] C7 opencode 场景回路(AC2)：`make config` 勾选 opencode -> `make down && make up` 重建 base+app ->
      `docker exec aio-app-1 bash -lc 'command -v opencode'` 有 -> 侧边栏出现 opencode 按钮 -> 点击跑起 opencode ->
      再点收起 -> 再点为新会话。

## D. 收尾

- [ ] D1 `cargo test` 全绿、`cargo clippy -D warnings` 干净、`npm run build` 通过。
- [ ] D2 README/README.zh-CN：补“侧边栏按钮 + 自定义按钮注册”用法小节(可选，与现有文档风格一致)。
- [ ] D3 更新/新增记忆：把本任务决议(标签栈/command_exists/buttons.toml CRUD)写入 memory，并标注
      `compose-up-build-skips-running` 在本任务的 force-recreate 用法。

## 风险文件 / 回滚点

- **`app/src/config.rs`**：build_manifest 签名变更，牵动 manifest.rs/main.rs；一处改错全 manifest 崩。回滚 = git revert。
- **`app/services.toml`**：`include_str!` 烘进，改后须 `cargo build`/app 重建才生效(非运行时)。
- **`web/src/App.tsx`** 全量重写：最大前端风险；保留 IframePane/XtermPane 降低面。回滚 = revert + 恢复 layout.ts/GL。
- **buttons.toml 卷写**：app 以 gem 写 /home/gem/.aio；若 sandbox-base 未给 gem 建 /home/gem 属主，mkdir 可能失败 --
  实测 `docker exec aio-app-1 ls -ld /home/gem` 确认属主 gem；若不是，design §7 需补 entrypoint mkdir。
- **command_exists 首请求延迟**：PATH 解析 bash 启动 ~100ms；若实测 manifest 明显卡，缩 TTL 或预热(启动时 resolve 一次)。

## 实现前再确认(阻塞项)

- 无阻塞项。所有产品/范围/交互决议已落在 prd.md Decisions 与 Assumptions。如对 Assumptions(侧边栏折叠默认/manifest
  手动刷新/关 tab 杀进程)有异议，在 `task.py start` 前提出。
