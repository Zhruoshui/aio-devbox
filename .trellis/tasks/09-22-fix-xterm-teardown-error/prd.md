# 修复 mgr-web 离开工作区页时 xterm addon 拆除抛未捕获 TypeError

## Goal

消除 mgr-web 的一个**未捕获运行时错误**：每次从「工作区」页切到任何其它
页面（镜像 / 用量 / 沙箱列表 / 模型配置）时，`console` 与 `window` 各抛一次
`TypeError: Cannot read properties of undefined (reading '_isDisposed')`。

错误不阻断功能（目标页正常渲染、导航正常），但它污染 console、会让任何
以「console 零 error」为验收门的检查失败（S7 原型重构那次就是这样验的），
也让真实回归噪音被掩盖。

## Background

### 复现（2026-09-22 实测，稳定复现）

用容器内 chromium + puppeteer-core 打运行中的 mgr（`http://mgr.localhost/`）：

```
加载工作区后 console+pageerror 错误数 = 0
切到「镜像」页后                      = 2   ← 同一条，console + pageerror 各一次
切到「用量」/「沙箱列表」/「模型配置」：同样各 +2
```

**与目标页无关**——只要离开工作区页就抛。所以触发点是 **WorkspacePage 的卸载**，
而非某个页面的挂载。

### 栈（生产构建，已符号化到可读帧）

```
TypeError: Cannot read properties of undefined (reading '_isDisposed')
    at <anonymous> (assets/index-*.js)      ← 库内部
    at QT / clear
    at dispose
    at _wrappedAddonDispose      ← xterm 的 addon 拆除包装器
    at c.dispose                 ← addon.dispose()
    at dispose
```

### 代码现状（`mgr-web/src/pages/workspace/panes/XtermPane.tsx`）

effect 里先后 `term.loadAddon()` 了 5 个 addon：`FitAddon`(:125)、
`WebglAddon`(:142)、`ClipboardAddon`(:155)、`WebLinksAddon`(:175)、
`SearchAddon`(:180)。

**WebGL 尤其值得注意**：它只作为 try 块内的局部量存在，唯一的 dispose
路径是 context-loss 回调：

```js
try {
  const webgl = new WebglAddon();
  webgl.onContextLoss(() => webgl.dispose());   // :141
  term.loadAddon(webgl);                        // :142
} catch { /* DOM renderer 兜底 */ }
```

而 cleanup 只有一行终端拆除，**没有任何 addon 的显式拆除**：

```js
return () => {
  disposed = true;
  resizeObserver.disconnect();
  modeObserver.disconnect();
  currentWs?.close();
  setSearchVisible(false);
  term.dispose();                // :300 —— 只此一处
  termRef.current = null;
  searchAddonRef.current = null;
};
```

### 根因假设（**实现期须先证实再动手**）

`term.dispose()` 会由 xterm 内部连带拆除所有已加载 addon
（`_wrappedAddonDispose`）。在这条链上，某个 addon（最可能是
**WebglAddon**：它持有 GPU 上下文与 renderer 状态，且是唯一没有任何显式
生命周期管理的 addon）的 `dispose()` 访问了已被终端拆掉的内部状态里的
`undefined._isDisposed`。

可信度：栈指向 addon 拆除链 + WebGL 是唯一「无显式生命周期管理」的 addon。
**但有个反证**：headless 常常拿不到 WebGL 上下文（`new WebglAddon()` 直接抛、
被 catch 吞掉），此时错误不该来自 WebGL——而实测恰恰是在 headless 里复现的。
所以**先用 dev 构建（保留可读栈）确认到底是哪个 addon，再决定修法**，
不要照搬假设就改。

## Requirements

- R1 定位到**具体 addon 与具体调用点**（用可读栈/断点，不接受「大概是 webgl」）
- R2 离开工作区页后 console error 与 pageerror **均为零**
- R3 不回归终端能力：切页回来后新开终端仍能渲染、搜索、复制、resize
- R4 覆盖「WebGL 上下文丢失」路径：context-loss 回调已 dispose 一次，
  cleanup 可能再 dispose 一次 —— 重复 dispose 必须幂等或被防护

## Acceptance Criteria

- [ ] **AC1** 切页零错误：puppeteer 从工作区切到镜像/用量/沙箱列表/模型配置
      四个页面，`console` error 与 `pageerror` 计数**全为 0**
- [ ] **AC2** 反向也零错误：从各页面切**回**工作区，同样零错误
- [ ] **AC3** 终端功能回归：切页回来后新建终端可用（能回显、能 resize），
      Ctrl+F 搜索栏可用
- [ ] **AC4** WebGL 不可用路径不回归：在拿不到 WebGL 上下文的情形下
      （headless 默认即是），终端仍以 DOM renderer 工作、无新增错误
- [ ] **AC5** `npm run build`（`tsc --noEmit && vite build`）EXIT=0

## Out of Scope

- 不改终端 WS 协议、不改 pane 布局与 golden-layout 结构
- 不做 xterm 版本升级（除非证实根因就是版本 bug 且升级是最小修法）
- 不借机重构 XtermPane 的其它部分

## Notes

- 本任务由 2026-09-22 的归档清理会话发现（当时在用 puppeteer 给 S2-S5 做
  实机验收，拿「console 零错误」当验收门时撞出来的）。
- 复现脚本参考：该会话的 `/tmp/mgrverify/probe3.mjs`（逐目标页统计错误增量）
  与 `probe4.mjs`（抓栈）；浏览器容器做法 = vnc 镜像 + 补装 node +
  `--network aio-mgr-net` + `--host-resolver-rules=MAP mgr.localhost <gw-ip>`。
- 轻量任务，PRD-only 即可启动。若实现期发现改动面变大（如需要重构多个
  addon 的生命周期），再补 `design.md`。
