# Implement: Sandbox 管理器统一聚合界面改版

依据 design.md §1-§8。Phase 之间独立提交，每个 Phase 结束跑验证命令；Phase 6（删 web/）是不可逆点，置于最后。

## Phase 1: mgr-api 沙箱代理（design §1）

- [ ] `mgr/Cargo.toml`: axum 加 `ws` feature；reqwest 加 `stream`；新增 `tokio-tungstenite = "0.23"`；重生成 Cargo.lock
- [ ] 新 `mgr/src/proxy.rs`：`/api/sbx/:name/*path` any-method；name 走 require_row + validate_name（宿主封闭派生）；HTTP 剥 hop-by-hop + `Body::from_stream`；WS 以 `app/src/routes/preview.rs` 为模板（Upgrade 探测 / 2s 握手 / subprotocol 回传 / 双向泵）
- [ ] routes.rs merge 代理 router（seam 之前）；上游 `http://sbx-{name}-piweb:8088/{path}`
- [ ] 测试：路由顺序（seam 不吞代理路径）、未知沙箱 404、name slug 拒绝
- 验证：`cargo test -p aio-mgr`；手动 `make mgr-up` 后 `curl http://mgr.localhost/api/sbx/<name>/api/manifest` 返回该沙箱 manifest

## Phase 2: mgr-web 工作区（design §2，最大块）

- [ ] 依赖：golden-layout ^2.6.0 / @xterm/xterm ^5.5.0 / @xterm/addon-fit ^0.10.0；CSS：gl-kumo.css + styles.css 裁剪段（pane/term/--term-*）；main.tsx no-StrictMode
- [ ] `pages/workspace/paneUrl.ts`：urlFor(service, sandbox) 四类 URL 规则（design §2.4 表）
- [ ] `pages/workspace/panes/`：XtermPane（buildWsUrl 改造）+ IframePane（src 走 paneUrl）平移；契约（lineHeight 1.25 / resize 帧 / MAX_RECONNECT=1 / close-kills）原样
- [ ] `pages/workspace/SandboxTree.tsx`：listSandboxes 轮询 + 展开懒加载 manifest；stopped 置灰 + 节点启动按钮；注册按钮入口（RegisterDialog 平移，CRUD 走 `/api/sbx/:name/api/buttons*`）
- [ ] `pages/WorkspacePage.tsx`：golden-layout 机制平移（componentState 加 sandbox / 键 `mgr.layout` / seq 池 / headerHeight 40 / popout / 拖拽遮罩 / glyph patch）；默认布局 = 第一个 running 沙箱终端或空态
- [ ] App.tsx：Page union 加 workspace（默认页）+ nav 第五项；SandboxListPage "进入沙箱"改 goWorkspace(name)
- [ ] i18n 键块从 web/src/i18n.ts 移植（侧栏/工作区/终端键）
- 验证：`cd mgr-web && npm run build`（tsc --noEmit 门）；`make mgr-up` 手测：多沙箱 pane 混排 / 拖拽 / 刷新布局恢复 / 终端打字 / pi TUI / VNC iframe / pi-web iframe / popout

## Phase 3: code-server 按需（design §3）

- [ ] docker.rs：SANDBOX_PROFILES 拆 UP_PROFILES（仅 vnc）+ 全量保留给 stop/down/restart；新 compose_service_up
- [ ] routes.rs：`POST /api/sandboxes/:name/service/:service/start`（白名单 code-server，同步，adopted 走 file 变体）
- [ ] mgr-web code-server pane：探测可达直接 iframe；否则占位 → start → probe 轮询（`/api/sbx/:name/api/buttons/probe?port=8200`）→ iframe
- [ ] 存量 compose_up 调用点审计（start_sandbox / adopt 重连 / jobs.rs recreate 均须只带 vnc 起）
- 验证：`cargo test -p aio-mgr`；创建沙箱后 `docker compose -p sbx-<n> ps` 应见 3 容器 running + code-server exited/不存在；点开 pane 后 4 容器；stop 后全停

## Phase 4: 模型多 profile + 指派（design §4）

- [ ] mgr models.rs：kv 新键 `models_profiles`（version/profiles/assignments）+ 启动迁移（旧 models_config → default profile + 全沙箱指派，双向兼容读）
- [ ] API：GET/POST/DELETE profiles、PUT profiles/:id、PUT sandboxes/:name/model_profile、sync 加 `?name=`（未指派 404）；sandbox_json 加 model_profile
- [ ] composegen.rs 注入 `MGR_SANDBOX_NAME`；app mgr_sync.rs 带 name + 404→debug 静默保持本地
- [ ] 双向 wire-shape 测试对更新（mgr sync_handler_shape_* / app sync_payload_decodes_*）
- [ ] mgr-web：ModelsPage profile 下拉+CRUD；EditPage 指派 select（三态）；卡片显示 profile
- 验证：`cargo test -p aio-mgr && cargo test -p aio-app`；端到端：改 A 沙箱所指 profile ≤60s 后 A 内 agent 文件更新、B 不动；解绑后沙箱保持本地

## Phase 5: 规格更新（design §6）

- [ ] sandbox-mgr-ops.md：契约 4 改写 / 契约 7 改写 / 契约 9 加第四处 / 新契约 10（代理）
- [ ] api-contracts.md：代理路由组 + 新端点 + sandbox_json 字段
- [ ] frontend/directory-structure.md：mgr-web 节重写 + web/ 节墓碑
- [ ] frontend/xterm-pane.md：路径与 WS 路由更新
- 验证：对照 research/contracts-quote.md 逐条核（不丢既有验证点）

## Phase 6: web/ 退役（design §5，不可逆点）

- [ ] `app/services.toml` 删 modelsConfig 条目
- [ ] `app/redirect/index.html`（MGR_URL 有→跳 mgr.localhost；无→说明页）；app/Dockerfile 删 web-builder 段改 COPY redirect
- [ ] 删 `web/` 目录；审计 Makefile / .github / 文档引用
- [ ] 重建 sandbox-app 镜像验证（注意 no-cache 重建前 builder prune，memory: no-cache-rebuild-disk-full）
- 验证：`make build && make up` 后 stock 栈 app :8088 返回 redirect 页；API 路由完好（`curl :8088/api/manifest` 正常）

## 回滚点

- Phase 1-5 任意回退 = revert 对应提交；web/ 在 Phase 6 前完好，mgr-web 不加工作区入口即回到旧形态。
- Phase 6 前确认 Phase 2 工作区已充分手测（多沙箱/拖拽/终端/iframe 全通）。

## task.py start 前检查

- [ ] prd.md 收敛 pass（无重复事实/无已解 open question）
- [ ] design.md / implement.md 用户已评审
- [ ] implement.jsonl / check.jsonl 已含真实条目（✓ 已填）
