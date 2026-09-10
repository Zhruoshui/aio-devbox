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

## WS 路由契约(经 mgr 代理,unified Phase 1/D6)

`paneUrl.ts::termWsUrl` 是唯一构造点:

```
ws(s)://<mgr origin>/api/sbx/<sandbox>/api/term/ws?cmd=<encodeURIComponent(cmd)>
```

- **same-origin on mgr**: 浏览器不再跨子域直连 `sbx-<name>-piweb:8088`
  (adopted 旧栈无 CORS 头);mgr/src/proxy.rs 转发(契约 10)。
- **一条 pane 一个会话**: 关闭 pane = unmount,WS 关闭,后端 pty 进程
  随 WS close 退出("close kills, reopen restarts");重开 = 全新会话。
- **掉线重连上限 1**: WS 中途断开写一条 notice、至多重试一次、之后停
  (不 crash、不 retry-spam)。
- 文本帧 = 按键;二进制 5 字节控制帧 = resize(`TIOCSWINSZ`)。

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
