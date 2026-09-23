# PRD: Sandbox 管理器统一聚合界面改版

## 目标

把"进入沙箱开新标签页打开 per-sandbox workbench"模式，改为**单一网页（Sandbox 管理器）**聚合所有沙箱的工作能力：拖拽工作区、终端、agent、code-server、VNC/Chromium、pi-web、模型配置、自定义按钮，全部在 mgr-web 内完成访问。

## 背景与现状（代码勘察确认）

- 两个独立 SPA：`mgr-web/`（管理器，React+Vite+TS，侧栏导航四页，无 golden-layout）与 `web/`（per-sandbox workbench，golden-layout 拖拽工作区 + 侧栏按钮 + `aio.layout` localStorage 布局 + popout 子窗口）。"进入沙箱" = 新标签页打开 `http://sbx-<name>.mgr.localhost/`（SandboxListPage.tsx:220-229）。
- 沙箱由 mgr-api（Rust axum :8089）经 docker compose CLI（DooD）管理：每沙箱 4 容器（gateway/app/code-server/vnc）、零宿主端口、`aio-mgr-net` 别名 + 总网关子域名路由（契约 2/3/8/9）。
- vnc sidecar：Xvnc(:5900) + openbox + Chromium（**CDP :9222 是 pi agent-browser 硬依赖**）+ noVNC(:6080)，`network_mode: service:app`；pi 为 always_on 场景 → vnc 必须常驻。
- code-server sidecar：:8200 `--auth none`，网关 `/code-server/*` 反代，iframe 嵌入；无任何东西依赖其常驻。
- pi-web：app 容器内 :30141，Next.js 必须独立 origin，子域名 `sbx-<name>-piweb.mgr.localhost`（契约 3）。
- 终端：自研 xterm.js + pty WS（`app:8088/api/term/ws`，文本帧=按键、5 字节二进制帧=resize）；agent（pi/opencode）= 带命令的终端 pane，多开多实例。
- 模型配置：真值已上收 mgr SQLite kv 单行（契约 7），沙箱 60s 拉取（`app/src/mgr_sync.rs`）、本地写端点 403；mgr-web 有 Models 页，workbench 残留 ModelsPane。
- 自定义按钮：每沙箱 `/root/.aio/buttons.toml`（随 workspace 卷），CRUD 走沙箱 app API，`/preview/<port>/` 反代 web 类按钮。
- 存量栈 `docker-compose.yml` + `make up` 不依赖 mgr；adopt 向导可纳管（契约 8）。
- adopt 旧栈的 app 镜像**没有 CORS 头**——决定了 mgr-web 必须经 mgr-api 代理访问沙箱后端，而非浏览器跨子域直连。

## 需求（决策 D1-D8）

- **D1 唯一 UI**：彻底废弃 `web/` 作为独立入口；golden-layout 工作区/pane 组件/侧栏整体迁入 mgr-web；app 保留全部后端 API（终端 WS、buttons、manifest、preview），静态目录换 redirect 页；stock 栈需要 UI 走 mgr + adopt。
- **D2 每 tab 绑定沙箱**：侧栏为沙箱树（沙箱 → 工具按钮 + 自定义按钮），pane 绑定各自沙箱，tab 标题 `label@name`；布局全局唯一持久化，可混排多沙箱 pane。
- **D3 VNC 入口统一、实例不统一**：Chromium/Xvnc 留沙箱内不动；VNC pane iframe 该沙箱网关 `/vnc/`（零后端改动）。
- **D4 code-server 统一入口 + 按需实例（否决 SSH）**：容器保留但不随沙箱启动；pane 打开时 mgr-api 拉起再 iframe；沙箱 stop 一并停。否决 SSH 理由：sshd 进 base + 密钥分发 + vscode-server 离线烘焙版本耦合 + 扩展两处维护 + adopt 旧栈不可用。
- **D5 pi-web**：沙箱树按钮 + iframe 直嵌子域名，pane 标题区分；不做聚合页。
- **D6 终端/agent 挂沙箱树**：多开 XtermPane；stopped 置灰（树节点提供启动，不自动拉起）；mgr-web ↔ 沙箱后端走 **mgr-api 统一代理**（`/api/sbx/:name/*` → `sbx-<name>-piweb:8088`，HTTP+WS），理由：adopt 旧栈无 CORS 直连失效 + 未来认证单一收口。
- **D7 自定义按钮存沙箱内 + mgr 代理注册**：数据留 buttons.toml 随卷走；注册/删除对话框在 mgr-web 经代理调用；adopt 栈零改动复用。
- **D8 模型配置多 profile + 每沙箱指派**：mgr 存多套 profile，每沙箱指派一套；沙箱仍按现有渲染管线落 agent 文件；沙箱内模型 UI 消失；迁移时旧全局配置转 default profile 并指派全部现存沙箱（行为零变化）。
- 工作区为 mgr-web 默认落地页；管理页（沙箱/镜像/模型/用量）保留。

## 验收标准

- [ ] mgr.localhost 单页内：打开 ≥2 个沙箱的 pane 混排（终端@A + pi-web@B + VNC@C），拖拽/分屏/刷新后布局恢复；不再有任何"必须开新标签页"的入口（列表页"进入"= 切工作区）。
- [ ] 终端/agent：XtermPane 经 mgr 代理连任意沙箱 pty，打字/resize/多开/关闭杀进程全通；pi TUI 可正常启动。
- [ ] code-server：新沙箱 up 后仅 3 容器 running（app/gateway/vnc）；点开 pane 拉起并 iframe 可编辑；stop 后全停；adopt 栈同样可用。
- [ ] VNC/pi-web：iframe 可用（VNC 操作 Chromium、pi-web 加载各自子域名且不串沙箱）。
- [ ] 模型：建两个 profile 指派不同沙箱，改 A 的 profile ≤60s 后仅 A 的 agent 配置文件变化；解绑后沙箱保持本地；沙箱内无模型配置 UI；旧 kv 自动迁移后所有现存沙箱行为不变。
- [ ] 自定义按钮：在 mgr-web 给 A 注册 agent/web 按钮各一，A 树下出现且可启动/访问，A 的 buttons.toml 落盘；删除生效。
- [ ] `web/` 目录删除后：stock 栈 app :8088 返回指路 redirect 页，`/api/*` 全部完好；mgr 全栈 `cargo test` + mgr-web `npm run build` 绿。
- [ ] 契约 4/7/9 改写、新契约 10（代理）与前端规格落盘（.trellis/spec/）。

## 范围外

- 不改零宿主端口/子域名路由/无认证边界（契约 9）；代理的 pty 暴露面并入契约 9 记录，不加新认证。
- 不做 mgr 侧 pi-web 会话聚合页、usage 的 profile 维度、profile 间差异比较工具。
- 不做按钮跨沙箱复制/模板化（数据在沙箱内，未来可加）。
- 不动 CDP/agent-browser、场景系统、镜像构建链。

## 开放问题（阻塞规划）

无（D1-D8 全部收敛；设计细节见 design.md，含两处设计期默认值：未指派沙箱 sync 返回 404=保持本地、工作区为默认落地页）。
