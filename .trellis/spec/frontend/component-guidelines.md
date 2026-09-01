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
