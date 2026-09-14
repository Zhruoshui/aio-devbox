# Component Guidelines

## Functional components only, with typed props

```ts
export function XtermPane({ service }: { service: ServiceEntry }): JSX.Element { ... }
```

- Props are destructured and typed inline (or via a `type` for >2 props).
- Return type is annotated `JSX.Element`.
- No class components, no `React.FC`.

## Dispatch by service type, not by id

`PaneForService` (`App.tsx`) is the single switch on `service.type`:
`"web"` -> `IframePane`, else -> `XtermPane`. Never branch on a specific service
`id` - that breaks the data-driven model (adding a service should need no
React change).

## Imperative libraries: useEffect + useRef

`golden-layout` and `xterm.js` are imperative. The pattern (see `App.tsx`,
`XtermPane.tsx`):

1. Hold the imperative instance in a `useRef` (`glRef`, `containerRef`).
2. Set it up inside `useEffect` (create, configure, register listeners).
3. **Clean up in the effect's return** (`gl.destroy()`, `term.dispose()`,
   `resizeObserver.disconnect()`, `ws.close()`).

For golden-layout's React roots: a `WeakMap<ComponentContainer, Root>` tracks
roots; unmount on `beforeComponentRelease` so closed/dragged-out panes don't
leak effects (see `App.tsx`).

## File-level comments

Each `.tsx` starts with a `//` block explaining what the component is, the
service `type` it serves, and any non-obvious behavior (e.g. the iframe
drag-overlay trick in `IframePane.tsx`, the resize protocol in `XtermPane.tsx`).

## Manifest `url` `{host}` placeholder

A `type=web` service whose container publishes its own port (pi-web on
30141) cannot hardcode a hostname in `services.toml` — the workbench may be
browsed via localhost or a LAN IP. Convention: the manifest url carries a
literal `{host}` (`http://{host}:30141/`); `IframePane` substitutes
`window.location.hostname` at render time. Path-style urls (`/code-server/`,
`/vnc/...`) contain no placeholder and pass through untouched.

**Every consumer of `manifest.url` must apply the same substitution.**
`smoke-test.cjs` builds iframe `src` prefixes with
`.replace("{host}", "localhost")` to match the browser's origin — forgetting
this makes the iframe wait time out on a selector containing the literal
`{host}`.

## Clipboard writes need a non-secure-context fallback

`navigator.clipboard` is **undefined** when the workbench is reached over
plain http on a LAN IP (not a secure context; localhost is exempt). Any copy
button must (see the statusbar host-copy in `Statusbar.tsx`):

1. use `navigator.clipboard.writeText(...)` when available;
2. fall back to a hidden `<textarea>` + `document.execCommand("copy")`;
3. show transient feedback (a "copied" tag reverting after ~1.5s) on success
   and stay silent on failure.

Headless-test note: puppeteer cannot reliably READ the clipboard
(`readText()` returns "" or throws `NotAllowedError` even with overridden
permissions). Assert the write contract instead: stub
`navigator.clipboard.writeText` in-page and assert its argument.

## Component layer first: reach for `components.css` classes (09-11)

The prototype redesign moved every shared visual to the designer component
layer (`components.css`, sourced from `docs/Web-Prototype/mgr-web.css`).
Before writing any page-local CSS, check the component layer for an
equivalent: `.page`/`.page-head` (admin-page scaffold), `.btn` family,
`.badge`, `.segmented`, `.tabs`, `.chip`, `.card`, `.field`+`.input`+`.err`,
`.overlay`+`.dialog`, `.menu`, `.pop`, `.drawer`, `.table`, `.tree-*`,
`.statusbar`, `.spinner`. Page-local classes in `styles.css` are for layout
the component layer genuinely does not cover (e.g. usage charts,
golden-layout integration) — and they may only consume tokens, never
hardcoded colors (dark/light both come from `[data-mode]`).

Overlay components (menu/popover/drawer/dialog) share one contract —
copy it from an existing one:

- `role` semantics: `menu`+`menuitem` (NodeMenu), `dialog` (pop/drawer),
  `alertdialog` (destructive confirms).
- Escape closes; outside `pointerdown` (menu/pop) or scrim click (drawer/
  overlay) closes; focus returns to the opener where practical.
- Anchored overlays are `position: fixed` with caller-computed coordinates
  (NodeMenu's `useLayoutEffect` measure pattern); full-screen ones use the
  `.overlay`/`.drawer` wrappers.

Icons come from `icons.tsx` (`IconName` keys aligned with the prototype's
`mgr-shell.js`): add the path there, never inline a one-off `<svg>` in a
page. New user-facing strings go through `i18n.ts` — `t()` is typed
`keyof Strings`, so a missing key (or a missing language column) fails
`tsc`; the build gate doubles as the bilingual gate.
