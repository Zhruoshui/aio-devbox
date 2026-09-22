# S5 侧栏折叠：图标栏 + flyout

父任务：09-10-mgr-web-ux-batch2（决策 D6，全量背景见父 prd.md）。

## Goal

管理器左侧导航可折叠为图标栏；工作区 SandboxTree 可折成竖向图标条 +
hover flyout，最大化工作区。

## Requirements

- R1 App.tsx sidebar 折叠态：只露图标（~48px），展开/收起按钮常驻，
  状态存 localStorage（键如 aio.mgr.sidebarCollapsed）
- R2 SandboxTree 折叠态：竖向细条，每沙箱一个首字母圆形图标；hover 弹
  flyout 浮层显示该沙箱服务按钮组（终端/agent/code-server/vnc/pi-web，
  按钮可见性与展开态一致——manifest 探测）；点击按钮即开 pane 并不关闭
  flyout（连续开多个）
- R3 折叠状态下 stopped 沙箱的置灰/启动入口语义保持（flyout 内同样置灰）
- R4 golden-layout tab 条不动；两个折叠互不影响、各自记忆
- R5 响应式：flyout 超出视口时自动翻转方向

## Acceptance Criteria

> 2026-09-22 实机验收（puppeteer + 容器内 chromium，打到运行中的 mgr
> `http://mgr.localhost/`，脚本 `/tmp/mgrverify/final.mjs`）。注意实测口径：
> **工作区树的折叠是 `.ws-tree.collapsed`（宽 272px→48px），不是 App 的
> `.app.panel-hidden`**——两者是独立的键与状态（`mgr.treeCollapsed`
> vs `mgr.panelHidden`），验收时别测错对象。

- [x] AC1 管理器侧栏折叠/展开流畅，刷新后状态保持
      —— 折叠后 `.ws-tree` 带 `collapsed` 类、宽度 272px→**48px**、搜索框
      隐藏、`localStorage.mgr.treeCollapsed=1`；刷新后仍 48px。（实测）
- [x] AC2 SandboxTree 折叠后 hover 任意沙箱图标弹 flyout，点击按钮开 pane
      —— 折叠态 `.ws-cavatar` 出现；hover 弹出 `.ws-flyout[role=tooltip]`，
      含沙箱名 `feiver`、状态 badge 与 `.launch-btn` 动作。**注**：实测该
      沙箱当时只有「注册按钮」一项可启动（其真实服务在列表页可启动），
      故「点击按钮开 pane」这一步**未直接点通**，仅验到按钮渲染与可用态。
- [x] AC3 折叠态 stopped 沙箱按钮置灰且提供启动 —— **owner 手动验证**
      （需 stop 一个沙箱才有样本；AI 侧只确认代码路径存在：
      `.ws-cavatar.is-stopped` / `.sb-row.ws-disabled` + `onStart`）。
- [x] AC4 展开态行为与现状完全一致（回归）
      —— 展开后宽度回到 272px、无 `collapsed` 类、搜索框与
      `panel-count`（"1 · 1 运行中"）恢复。（实测）
- [x] AC5 tsc + build 全绿（纯前端，无后端改动）
      —— `npm run build`（`tsc --noEmit && vite build`）EXIT=0。（实测）

### 实测副产物：发现一个未记录的缺陷

离开工作区页时抛**未捕获** TypeError
`Cannot read properties of undefined (reading '_isDisposed')`，栈指向
`term.dispose()` → `_wrappedAddonDispose` → addon dispose（xterm）。
根因：`XtermPane.tsx` 卸载时只调 `term.dispose()`，**从未显式 dispose
WebGL addon**（`webgl.dispose()` 仅挂在 `onContextLoss` 回调上），
xterm 拆除时走到 addon 的内部状态已失效处。该错误与任何目标页无关
（切镜像/用量/列表/模型都复现），**当前无任务覆盖**，建议单开修复。

## Notes

- 轻量任务：PRD-only 即可启动，不强制 design.md
