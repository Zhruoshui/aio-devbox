# Xterm Pane Guidelines

> Contracts for `mgr-web/src/pages/workspace/panes/XtermPane.tsx` — the
> generic terminal pane for `service.type !== "web"`, bound per-sandbox
> (`componentState.sandbox`, unified Phase 2/D2). Ported verbatim from
> web/src/panes/XtermPane.tsx (retired, see directory-structure.md) — only
> the WS URL construction changed. Pairs with
> [component-guidelines.md](./component-guidelines.md) (imperative-lib
> lifecycle pattern).

## Terminal surface contract

```ts
new Terminal({
  fontFamily: readTermFont(),     // concrete stack, NOT "var(--font-mono)" — see 渲染器与字体契约
  fontSize: 13,
  lineHeight: 1.25,               // MUST be explicit — see below
  scrollback: 10000,              // R1: default 1000 too small for builds/logs
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

## WS 路由契约(经 mgr 代理,unified Phase 1/D6)

`paneUrl.ts::termWsUrl` 是唯一构造点:

```
ws(s)://<mgr origin>/api/sbx/<sandbox>/api/term/ws?cmd=<encodeURIComponent(cmd)>[&cwd=<encodeURIComponent(cwd)>]
```

- **same-origin on mgr**: 浏览器不再跨子域直连 `sbx-<name>-piweb:8088`
  (adopted 旧栈无 CORS 头);mgr/src/proxy.rs 转发(契约 10)。
- **一条 pane 一个会话**: 关闭 pane = unmount,WS 关闭,后端 pty 进程
  随 WS close 退出("close kills, reopen restarts");重开 = 全新会话。
- **掉线重连上限 1**: WS 中途断开写一条 notice、至多重试一次、之后停
  (不 crash、不 retry-spam)。
- **帧协议(09-18-term-web-polish 后)**: Text 帧 = pty stdout(下行)/
  按键(上行)。Binary 5 字节控制帧,**双向**:
  - client→server `0x01` = resize(`[0x01, cols_le_u16, rows_le_u16]`,
    `TIOCSWINSZ`);
  - server→client `0x02` = exit code(`[0x02, exit_code_le_u32]`),pty
    teardown 后、WS close 前发一次,正常退出(0)也发——该帧区分"进程
    以 N 退出"与"中途掉线"(后者不发)。前端收到后写
    `● 进程已退出 (code N)` notice,随后照常走 onclose 断线/重连逻辑,
    生命周期契约不变。mgr proxy 对两个方向的 Binary 帧均透明中继。
- **`?cwd=` 参数(R7)**: 可选,pty 初始工作目录。后端校验:存在且为目录
  才生效,否则回退 `/root`(debug log)。services.toml agent 条目可选
  `cwd` 字段(manifest 透传,仅 agent 类型序列化,缺省省略)。不带参数
  = `/root` 不变。
- **exit code 语义**: 客户端主动关 pane 时,后端 teardown kill 子进程,
  portable-pty 将 signal-kill 映射为 code 1(非 137)——用户关 pane 后
  的重连若收到 0x02 code 1 属预期,不是崩溃信号。

## 渲染器与字体契约(09-18-term-web-polish R3)

- **WebGL 渲染器优先**(`@xterm/addon-webgl`): constructor/loadAddon 抛
  异常(无 WebGL 上下文、shader 失败、devtools 禁 GPU)与
  `onContextLoss` → dispose 两条失败路径都自然回退 DOM 渲染器,pane
  不会白屏。
- **fontFamily 必须是具体字体串,不能是 `var(--font-mono)` 字面量**:
  WebGL 经 canvas `ctx.font` 绘制,**不解析 CSS 变量**——传 var() 字面量
  会静默回退浏览器默认字体(DOM 渲染器无所谓,所以这个坑只在 WebGL 激活
  后显形)。`XtermPane.tsx::readTermFont()` 在 mount 时用
  `getComputedStyle` 解析 `--font-mono`(与 `readTermTheme()` 同模式),
  空值回退 `"monospace"`。
- **主题色走 ThemeService,两种渲染器共用**: `readTermTheme()` 的
  oklch/color-mix 值由 xterm 的 canvas fillStyle 往返解析
  (`css.toColor`);解析失败(如带 alpha 的 `--term-selection`)静默回退
  xterm 默认色——既有行为,与渲染器无关。主题热切换
  (`term.options.theme = ...`)在两种渲染器下都触发重绘。

## Addon 清单(09-18-term-web-polish)

`XtermPane.tsx` 挂载的 addon:fit(既有)、webgl、clipboard(OSC52,
权限被拒时 provider 包装为静默 no-op——默认 provider 会经
`queueMicrotask` 重抛拒绝,必须捕获)、web-links(URL 可点击)、
search(Ctrl+F 搜索条,React 状态渲染 + addon handle 走 ref 桥接)。

## Verification

- Build gate is enough for the contract: `tsc --noEmit` accepts `lineHeight`
  (xterm 5.x option) and `vite build` bundles it.
- The visual regression signal (crowded rows) is **not** caught by any smoke
  test (asserts interactions, not pixels). Verify by eye in a terminal pane
  after `make mgr-up`, or when swapping the mono stack / font size.

## Related

- `styles.css` `--font-mono` / `--term-*` tokens (surface colors for xterm).
- `paneUrl.ts` — per-sandbox URL factory (term WS via mgr proxy, web buttons
  via `/api/sbx/<name>/preview/<port>/`, gateways via subdomain origins).
- Terminal pty/resize protocol documented in the `XtermPane.tsx` header comment.
