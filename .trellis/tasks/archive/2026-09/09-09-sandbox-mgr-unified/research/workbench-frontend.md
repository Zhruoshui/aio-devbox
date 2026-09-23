# Research: web/ workbench SPA internals (for porting the golden-layout workspace into mgr-web)

- **Query**: web/ SPA internals — golden-layout version & registration pattern, layout save/load/popout, XtermPane/IframePane URL contracts, RegisterDialog, Sidebar, i18n/theme. What is origin-coupled vs portable.
- **Scope**: internal
- **Date**: 2026-09-09

## 1. Package / build facts

- `web/package.json:17` — `"golden-layout": "^2.6.0"` (lockfile pins exactly **2.6.0**, `web/package-lock.json`).
- `web/package.json:16` — `"@xterm/xterm": "^5.5.0"` (pinned 5.5.0), `:15` `@xterm/addon-fit ^0.10.0`.
- Other deps: react 18.3.1, react-dom 18.3.1, `@fontsource/inter ^5.1.0`, `puppeteer-core ^23.11.1` (smoke-test only, listed as a *dependency* not devDependency — `web/package.json:18`).
- Build: `web/package.json:9` `build: "tsc --noEmit && vite build"`; `vite.config.ts:14` `base: "/"`, `:16` outDir `dist`.
- Dev proxy `web/vite.config.ts:20-24`: `/api` → `http://localhost:8080`, `/code-server` and `/vnc` → same with `ws: true`.
- Served at `/` by the app's ServeDir (baked to `/app/static` by `app/Dockerfile:94` `COPY --from=web-builder /web/dist /app/static`).
- `web/src/main.tsx:9-11` — deliberately **no StrictMode** (golden-layout is imperative; double-invoked effects would create two instances).

## 2. Golden-layout wiring (App.tsx)

- Component type: single constant `"aio-pane"` — `web/src/App.tsx:56` (`PANE_COMPONENT_TYPE`).
- One `GoldenLayout` instance in a ref, mounted into a container `<div>` — `web/src/App.tsx:273-274` (`new GoldenLayout(el)`, `gl.resizeWithContainerAutomatically = true`).
- Registration: `gl.registerComponentFactoryFunction(PANE_COMPONENT_TYPE, factory)` — `web/src/App.tsx:276-314`. The factory:
  - Decodes `componentState` via `readPaneState` (`web/src/App.tsx:564-572` — single decoder; validates `service` through `isServiceEntry` `:574-582` and optional numeric `seq`).
  - Mounts a React root: `createRoot(container.element)` + `root.render(<PaneForService service={pane.service} />)` (`web/src/App.tsx:281-283`); roots tracked in a `WeakMap<ComponentContainer, Root>` (`web/src/App.tsx:110`).
  - Cleanup on `container.on("beforeComponentRelease", ...)` — unmounts the root and returns the instance's seq number to the per-service pool (`web/src/App.tsx:302-311`).
- **How panes map to components**: `PaneForService` at `web/src/App.tsx:550-555` dispatches on `service.type`:
  - `"web"` → `<IframePane service={...} />`
  - `"page"` → `<ModelsPane service={...} />` (the model-config page)
  - else (`"agent"`) → `<XtermPane service={...} />`
- Launch flow: `launch(service)` at `web/src/App.tsx:244-257` — every sidebar click creates a NEW instance via `gl.newComponent(PANE_COMPONENT_TYPE, { service, seq: n }, title)`; seq pool = smallest unused positive int per service id (`getSeqPool` `web/src/App.tsx:589-596`; restore walk `collectInUseSeq` `:604-612`).
- Layout is built ONCE (`useEffect` guarded by `glRef.current`, deps frozen `[glStatus]` — `web/src/App.tsx:265-434`, comment at `:430-433`: manifest refreshes must NOT rebuild the layout).
- Default layout when nothing saved: single stack with one terminal component (`web/src/App.tsx:343-369`).
- iframe drag-capture: pointerdown on `.lm_splitter/.lm_header/.lm_tab` adds `is-dragging` class to the layout root so CSS reveals `.drag-overlay` over iframes (`web/src/App.tsx:411-422`); the overlay itself is rendered inside IframePane (`web/src/panes/IframePane.tsx:25`).
- Tab glyphs: MutationObserver re-patches `data-icon` on every `.lm_tab` (`web/src/App.tsx:395-406`, `patchTabs` `:178-187`); glyphs drawn by CSS masks keyed on `data-icon` in `web/src/gl-kumo.css:70-81` (code/browser/terminal/chat).

## 3. Layout save / load / popout-subwindow mechanism

- Persist key: `aio.layout` (`LAYOUT_KEY`, `web/src/App.tsx:63`), localStorage, `ResolvedLayoutConfig.minifyConfig(gl.saveLayout())` JSON, 500ms debounce on `stateChanged` (`web/src/App.tsx:375-388`).
- Restore: `ResolvedLayoutConfig.unminifyConfig(JSON.parse(raw))` + `LayoutConfig.fromResolved` + **`dimensions.headerHeight: 40` re-applied** (golden-layout writes header height as an inline style; gl-kumo.css lays out a 40px strip — `web/src/App.tsx:329-341`, `:94-97`). Any failure → default single-terminal layout.
- **Popout subwindows**: golden-layout's built-in popout path is BYPASSED. `consumeSubWindowLayout()` (`web/src/App.tsx:74-103`, runs once at module load, `SUB_WINDOW` const at `:104`):
  - The parent's BrowserPopout writes the popped config to localStorage under the `gl-window` URL param key.
  - The child strips the param via `history.replaceState`, reads + removes the key, `unminifyConfig`s it, and re-applies `headerHeight: 40`.
  - In the child, the effect loads `SUB_WINDOW.config` as the whole layout, sets `document.title`, and sets `window.__glInstance = gl` so the parent's BrowserPopout can popIn (`web/src/App.tsx:316-321`).
  - The child renders a lone workspace + a dock-back button that emits `gl.emit("popIn")` (`web/src/App.tsx:440-455`).
  - Rationale in `web/src/App.tsx:65-73`: the library's built-in subwindow path would wipe document.body (killing the React root) and defer init() past our loadLayout call.
  - The child must NOT attach the stateChanged save listener (would overwrite the parent archive with the single-pane layout) — `web/src/App.tsx:371-374`.

## 4. XtermPane — props & WS URL contract

File: `web/src/panes/XtermPane.tsx`.

- Props: `{ service: ServiceEntry }` — the whole manifest entry; uses `service.cmd` (may be `undefined` → `""`).
- **WS URL construction — ORIGIN-COUPLED** (`web/src/panes/XtermPane.tsx:25-28`):
  ```ts
  function buildWsUrl(cmd: string): string {
    const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
    return `${proto}//${window.location.host}/api/term/ws?cmd=${encodeURIComponent(cmd)}`;
  }
  ```
  Assumes the pty API is on the SAME origin as the SPA. Portable to mgr-web only by changing the host to the mgr proxy (e.g. `/api/sbx/<name>/term/ws?cmd=...` same-origin on mgr).
- Terminal options (`web/src/panes/XtermPane.tsx:37-47`): `fontFamily: "var(--font-mono)"`, `fontSize: 13`, `lineHeight: 1.25` (spec: frontend/xterm-pane.md — explicit lineHeight is a contract), `cursorBlink: true`, `theme: readTermTheme()`.
- Resize protocol: Binary 5-byte control frame `[0x01, cols_le, rows_le]` sent on `term.onResize` and on open (`web/src/panes/XtermPane.tsx:64-82`); keystrokes as Text frames (`:77-81`).
- **Reconnect logic** (`web/src/panes/XtermPane.tsx:22, :102-109`): `MAX_RECONNECT_ATTEMPTS = 1`; on close, write "Terminal disconnected", retry once after 1000ms, then stop (no retry spam).
- Theme: `readTermTheme()` resolves Kumo `--term-bg/--term-fg/--term-selection` tokens via `getComputedStyle` (`web/src/panes/XtermPane.tsx:155-165`); a MutationObserver on `<html data-mode>` live-retints without reconnecting (`:122-128`).
- Fit: `safeFit` tolerates 0×0 hidden tabs; ResizeObserver refits (`:113-117, :142-148`).
- Cleanup: unmount closes WS (pty process exits server-side) — "close kills, reopen restarts" contract (`web/src/panes/XtermPane.tsx:130-136`).

## 5. IframePane — URL contract, {host} substitution

File: `web/src/panes/IframePane.tsx` (28 lines, fully generic).

- Props: `{ service: ServiceEntry }`; `src = service.url?.replace("{host}", window.location.hostname)` (`web/src/panes/IframePane.tsx:21`).
- **`{host}` substitution is ORIGIN-COUPLED**: it substitutes the hostname *the workbench itself* was reached on. Only piWeb uses it (`app/services.toml:102` `url = "http://{host}:{env:PI_WEB_HOST_PORT:30141}/"`). Under mgr the sandbox publishes no host ports, so this path is irrelevant for mgr-created sandboxes; for the mgr-web port the `{host}` form should be replaced by the pi-web subdomain URL (D5) or an mgr-side proxy.
- Gateway path URLs (`/code-server/`, `/vnc/vnc.html?...&path=vnc/websockify`) are **relative** — same-origin as the workbench origin; in mgr-web these must be rewritten to per-sandbox proxy routes (or subdomain URLs).
- Drag overlay div (`:25`) + App's `is-dragging` class cooperate (see §2).
- VNC URL note (`app/services.toml:39-46`): `path=vnc/websockify` is REQUIRED because noVNC builds the WS URL absolute to host root.

## 6. RegisterDialog

File: `web/src/RegisterDialog.tsx`.

- Two button types ("agent" cmd / "web" port) with a segmented control (`web/src/RegisterDialog.tsx:192-205`).
- Port probe: debounced 500ms `GET /api/buttons/probe?port=N` → `{listening: boolean}` (`:98-124`); result stale-dropped; port 8088 excluded client-side too (`:101`, `:149`).
- Submit → `onRegister(RegisterButtonInput)` → App POSTs `/api/buttons` (`web/src/App.tsx:522-535`) and DELETEs `/api/buttons/:id` (`:537-547`) — **same-origin /api assumption**.
- Kumo dialog guidance: keep mounted + open state; focus first field on open, restored to opener on close; Escape/backdrop close (`web/src/RegisterDialog.tsx:12-14` header comment).
- POST body contract mirror: `web/src/types.ts:41-50` (`RegisterButtonInput`: label, cmd?, type?, port?).

## 7. Sidebar grouping

File: `web/src/Sidebar.tsx`.

- Four groups (`web/src/Sidebar.tsx:43-67`):
  - `web` — "Web 工具": type=web AND NOT deletable (user web buttons go to custom, per 09-02 fix R1).
  - `page` — "系统": type=page (modelsConfig).
  - `tui` — "终端与 Agent": type=agent AND NOT deletable.
  - `custom` — "自定义": everything `deletable === true`.
- Collapsible rail (localStorage `aio.sidebar.collapsed`), manual refresh with 900ms spinner window, register button in `sb-foot` (`:76-151`).
- Buttons are launchers: each click → new instance (contract in header comment `:4-8`).

## 8. Statusbar / stats / manifest fetch

- `useStats` polls `GET /api/stats` every 3s; doubles as heartbeat (`web/src/useStats.ts:16, :24-51`) — **same-origin fetch**.
- App fetches `GET /api/manifest` (`web/src/App.tsx:192-196`) — same-origin; refresh on window focus (2s throttle, `:228-238`).
- Statusbar copy button copies `window.location.origin` (`web/src/Statusbar.tsx:89`), reset-layout (`web/src/App.tsx:146-149`), theme/lang toggles.
- Theme/lang persistence keys: `aio.theme`, `aio.lang` (`web/src/App.tsx:59-60`); pre-paint restore script in `web/index.html:7-23`. Data-mode is Kumo's native hook (`web/src/App.tsx:157-160`).

## 9. i18n / theme wiring

- `web/src/i18n.ts` — flat string table zh-CN/en, `t(lang, key)` + `fmt(lang, key, n)` (`:410-418`). ~200 keys including the full model-config set (`mc*`/`ma*`). No framework by design (`:1-4`).
- Kumo token layer: `web/src/styles.css:14-97` (light `:root`) and `:100-121` (dark `[data-mode=dark]` overrides) — tokens: `--bg/--surface/--surface-warm/--fg/--muted/--border(-soft)/--accent*/--success/--warn/--danger/--font-*/--text-*/--space-*/--radius-*/--elev-*/--focus-ring/--motion-*/--term-*/--scrollbar-*`. Dark values override ONLY through `[data-mode]` (`:14` comment "no manual dark variants below the token layer" — same rule copied into mgr-web `mgr-web/src/styles.css:5-8`).
- golden-layout theming: `web/src/gl-kumo.css` (278 lines) replaces goldenlayout-light-theme.css entirely; imports `golden-layout/dist/css/goldenlayout-base.css` at `web/src/App.tsx:38`.

## 10. Origin-coupled vs portable (explicit list)

### Origin-coupled (assumes same-origin /api; must change for mgr-web)

| Item | Anchor | Coupling |
|---|---|---|
| XtermPane WS URL | `web/src/panes/XtermPane.tsx:25-28` | builds `ws(s)://<same host>/api/term/ws?cmd=` |
| IframePane `{host}` substitution | `web/src/panes/IframePane.tsx:21` | substitutes the workbench's own hostname |
| IframePane relative urls (/code-server/, /vnc/) | `app/services.toml:31,47` | must resolve against sandbox gateway origin |
| Manifest fetch | `web/src/App.tsx:192-196` | `fetch("/api/manifest")` |
| Stats poll | `web/src/useStats.ts:32` | `fetch("/api/stats")` |
| Buttons CRUD + probe | `web/src/App.tsx:524,539`; `web/src/RegisterDialog.tsx:110` | `/api/buttons*` same-origin |
| ModelsPane ALL handlers | `web/src/panes/models/ModelsPane.tsx` (fetches at :145, :171, :402, :432, :461, :533, :595, :642, :800, :817, :847, :882, :914, :951, :984, :1013) | `/api/models/*` same-origin (already read-only under mgr; being deprecated by D1) |
| Statusbar host copy | `web/src/Statusbar.tsx:89` | copies `window.location.origin` |
| Popout subwindow | `web/src/App.tsx:74-104` | child window is same-origin (localStorage shared) — still fine if mgr-web serves the workspace page |

### Portable to mgr-web essentially unchanged

- The whole golden-layout registration/lifecycle pattern (§2) — imperative lib in a ref, single factory, React roots per container, beforeComponentRelease cleanup.
- Layout save/load + seq pool + popout consumption (§3) — pure browser mechanics, no origin dependency (localStorage is per-origin which is fine since parent+child are the same mgr-web origin).
- XtermPane everything EXCEPT `buildWsUrl` (terminal options, resize protocol, single reconnect, theme retint, safeFit).
- IframePane structure (drag-overlay trick) — only the `src` computation changes.
- RegisterDialog UI logic (probe endpoint URL changes to an mgr-proxied per-sandbox route per D7).
- Sidebar grouping logic; Statusbar (minus host-copy semantics); i18n table (mgr-web already has its own copy); Kumo token CSS + gl-kumo.css (mgr-web styles.css is already the ported token layer, missing only the golden-layout/pane/term sections — `mgr-web/src/styles.css:5-8` documents the deliberate trim).

## 11. Caveats / Not Found

- `web/src/layout.ts` referenced by `.trellis/spec/frontend/directory-structure.md:13` **no longer exists** — layout building now lives inline in App.tsx (spec is stale on this point; the spec will be edited by this task).
- `web/smoke-test.cjs` exists (puppeteer-core) but was not deep-read; it is the only place referencing puppeteer.
- The workbench SPA models page (`web/src/panes/models/*`) is a full duplicate of mgr-web's models pages — under D1 (web/ deprecated) this whole subtree dies; mgr-web already has its port (mgr-web/src/pages/models/*).
