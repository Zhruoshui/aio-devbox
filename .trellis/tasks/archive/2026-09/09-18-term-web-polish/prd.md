# terminal-web 体验与性能优化：xterm addon 补齐 + WebGL 渲染 + WS 协议增强

## Goal

借鉴 ghostty-web 调研结论（2026-09-18 会话）落地的第一批优化：补齐 xterm.js
addon 短板、切换 GPU 渲染器、增强 WS 协议（退出码通知、cwd 参数）。终端
渲染层与 pty 桥的核心架构不变。

**Out of scope**（另行 brainstorm，不在本任务）：

- pty 会话持久化 / 重连策略改造（"close kills, reopen restarts" 契约变更
  的取舍讨论）
- ghostty-web 前端组件替换 spike（记 backlog，等其 stable 或真撞渲染 bug）

## Background

- 现状：`XtermPane.tsx` 仅挂 `@xterm/addon-fit`，使用 xterm.js 默认 DOM 渲染器；
  scrollback 为默认 1000 行；WS 协议只有 0x01 resize 控制帧；pty 固定
  `cwd=/root`、退出时静默断开。
- 对照：coder/ghostty-web（xterm.js API 兼容的 Ghostty WASM 前端）虽不提供
  服务端，但其"渲染质量 + 会话元信息"关注点指出了本实现的短板。
- 已知约束：WebGL 渲染器走 canvas `ctx.font`，**不解析 CSS 变量** —— 现有
  `fontFamily: "var(--font-mono)"` 必须先经 `getComputedStyle` 解析为具体
  字体串（与 `readTermTheme()` 同模式）。

## Requirements

### R1 scrollback 提升（P1，一行）

`Terminal` 构造加 `scrollback: 10000`（默认 1000 不够跑构建/日志）。

### R2 clipboard addon（P1）

挂 `@xterm/addon-clipboard`，启用 OSC52 复制：终端内选中即复制到宿主
剪贴板（浏览器权限允许时），agent 面板输出的代码块可直达宿主。

### R3 WebGL 渲染器（P1，本批最大性能项）

- 挂 `@xterm/addon-webgl`（DOM 渲染器全屏重绘 TUI 卡顿；opencode/pi 高频刷新场景受益）。
- `fontFamily` 改为 mount 时经 `getComputedStyle` 解析 `--font-mono` 的
  具体字体串（canvas `ctx.font` 不解析 CSS var，否则静默回退默认字体）。
- `onContextLoss` / addon 失败时 dispose 并回退 DOM 渲染器，不能白屏。
- 主题热切换（`readTermTheme` MutationObserver 路径）须在 WebGL 下验证仍生效。

### R4 web-links addon（P2）

挂 `@xterm/addon-web-links`，终端输出中的 URL 可点击。

### R5 search addon（P2）

挂 `@xterm/addon-search`，pane 内 Ctrl+F 弹出最小搜索条（输入框 + 上/下
一个），Esc 关闭。不做正则/大小写等高级选项。

### R6 退出码控制帧（P1，WS 协议增强，双端）

- 服务端 `terminal.rs`：teardown 处 `child.wait()` 已有，取 exit code，
  断开前发送 Binary 控制帧 `[0x02, exit_code_le_u32]`（5 字节，与 0x01
  resize 帧同构：type + payload LE）。正常退出（0）也发。
- 前端 `XtermPane.tsx`：收到 0x02 帧后写 notice（如
  `● 进程已退出 (code N)`），再按现有断线逻辑处理。
- 帧类型分配：`0x01` = resize（不变），`0x02` = exit code（新增）。

### R7 `?cwd=` 参数（P3，可选，便宜就做）

- `TermQuery` 加可选 `cwd`；`spawn_pty` 用它设 `CommandBuilder.cwd()`
  （校验：存在且为目录，否则回退 `/root`）。
- `termWsUrl` 透传；services.toml 的 agent 条目可选 `cwd` 字段。
- 默认行为不变（`/root`，即工作区卷根）。

## Acceptance Criteria

- [ ] R1：终端 pane 滚回缓冲 ≥ 10000 行（`seq 1 20000 | tail` 后可上滚找到第 10000 行之前的输出）。
- [ ] R2：终端内选中文本，宿主系统剪贴板出现同样内容（浏览器授权 OSC52 场景）；未授权时静默降级不报错。
- [ ] R3：`opencode` 或 `vim` 快速滚动无肉眼卡顿；字体与 DOM 渲染器一致（非浏览器默认字体）；开发者工具禁用 WebGL 后 pane 仍可用（DOM 回退）；主题切换终端即时重新着色。
- [ ] R4：`echo https://example.com` 输出的 URL 可点击打开。
- [ ] R5：Ctrl+F 弹出搜索条，回车/按钮可在滚回缓冲中跳转匹配项，Esc 关闭且焦点回终端。
- [ ] R6：`cmd=pi` 的 pane 中退出 pi（或直接 `exit`），pane 显示 `● 进程已退出 (code N)` 且 N 与实际退出码一致；异常路径（pty spawn 失败）行为不变。
- [ ] R7（若做）：带 `cwd` 打开的 pane 初始 `pwd` 为指定目录；不合法路径回退 `/root`。
- [ ] 全量：`make mgr-up` 后手动冒烟：terminal / opencode 两个 pane 开、用、关、重开均正常；`cd mgr-web && npx tsc --noEmit` 与 vite build 通过。
- [ ] spec：`frontend/xterm-pane.md` 的 WS 协议段补 0x02 帧契约（Phase 3.3）。

## Notes

- 协议帧格式与 "close kills, reopen restarts"、重连上限 1 等既有契约见
  `frontend/xterm-pane.md`，本任务不触碰生命周期契约。
- 新依赖：`@xterm/addon-webgl`、`@xterm/addon-clipboard`、
  `@xterm/addon-search`、`@xterm/addon-web-links`（均官方 addon，与
  xterm 5.5 版本配套）。
- 依赖安装注意沙箱网络策略（npm registry 需在允许列表）。
