# Component Guidelines

## Functional components only, with typed props

```ts
export function XtermPane({ service }: { service: ServiceEntry }): JSX.Element { ... }
```

- Props are destructured and typed inline (or via a `type` for >2 props).
- Return type is annotated `JSX.Element`.
- No class components, no `React.FC`.

## Scenario grouping: layer (`category`) × install rung (`installer`)

`EnvPicker.tsx` renders the scenario catalog in two nested groupings, and BOTH
must mirror the config TUI (`config/src/scenario.rs`) — a cross-layer contract,
not a frontend detail:

- **Layer** = `scenario.category` (`os`/`shell`/`lang`/`app`), ordered by the
  local `LAYERS` table. Backend counterpart: `scenario::category_rank`.
- **Rung** = `scenario.installer` (`mise`/`apt`/`npm`/`tarball`), ordered by the
  local `INSTALLERS` table. Backend counterpart: `scenario::installer_rank`.

Rungs render **only when a layer mixes installers** (`layerRows()` checks
`new Set(items.map(s => s.installer)).size < 2` → flat). Deliberate: L2 shell is
100% mise, so a sub-heading there is pure noise, while L3 genuinely needs the
split (`mise 托管` rust/go/uv/ruff vs `系统路径·apt` c23 — the "两派" the
scenario PRD always described). The TUI applies the same rule via
`scenario::has_multiple_installers`; keep the two in sync.

The rung label names **where the tool lands** (`/opt/mise shims` vs a system apt
path), because that is what the user is actually choosing between. Each surface
owns its own label strings (SPA i18n, TUI `installer_title`) rather than
shipping display text over the API — same pattern as the layer headings.

Adding an installer means touching **three** places in lockstep: Rust
`INSTALLER_ORDER`, SPA `INSTALLERS`, and the `ins*` i18n pair. An unknown
installer degrades gracefully on both sides (renders last, labelled by its raw
string) — the same "never break on the unknown" rule as unknown categories.

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

## Dialogs: use `components/Dialogs.tsx`, never browser-native (09-15)

Browser-native `confirm()` / `prompt()` / `alert()` are **banned** — all five
legacy call sites (ModelsPage import/rename/delete, PresetList delete) were
migrated to `src/components/Dialogs.tsx`. New confirmation or text-input flows
MUST reuse these instead of rolling a new inline dialog:

- `ConfirmDialog { open, title, desc?, danger?, confirmLabel?, cancelLabel?, onConfirm, onCancel }`
  — destructive confirms pass `danger` (renders `role="alertdialog"` +
  `btn-danger`), plain confirms use primary styling (`role="dialog"`).
- `PromptDialog { ..., defaultValue?, placeholder? }` — submit is a real
  `<form>`, so Enter submits natively; opener's focus is captured on mount and
  restored on unmount via the shared `useDialogChrome` hook (Esc via document
  keydown, first-control focus, select-on-open for prefilled values).

Both are conditional renders of `.overlay.open` > `.dialog` (no CSS of their
own). A shared confirm label lives at i18n key `dialogConfirm` — don't reuse
`confirmDelete` ("确认删除") for non-destructive confirms.

## Table numeric columns: `th.ml-num` needs its own override

`.ml-table th { text-align: left }` out-specifies `.ml-num { text-align:
right }`, so a right-aligned `th` with `className="ml-num"` alone renders
left-aligned while its `td`s are right-aligned — this was the actual cause of
the usage-table "misalignment" report (09-15), not padding. Any new table with
numeric columns must include an explicit `th.ml-num { text-align: right }`
rule (styles.css) rather than relying on the shared `.ml-num` class.

> **Warning**: CSS cascade between layers is order-dependent — `components.css`
> is imported after `styles.css` (see `main.tsx` header comment) so its rules
> win ties. A components.css override of a styles.css property that styles.css
> sets with equal specificity works today only because of this import order;
> reordering imports silently breaks such overrides. Prefer higher specificity
> or explicit property resets over relying on the tie-break when touching
> layered component CSS (e.g. `.ws-tree-foot` needs an explicit
> `flex-direction: row` because the legacy styles.css block sets `column`).

Icons come from `icons.tsx` (`IconName` keys aligned with the prototype's
`mgr-shell.js`): add the path there, never inline a one-off `<svg>` in a
page. New user-facing strings go through `i18n.ts` — `t()` is typed
`keyof Strings`, so a missing key (or a missing language column) fails
`tsc`; the build gate doubles as the bilingual gate. Deleting a key means
deleting it from BOTH the zh-CN and en blocks in the same change and removing
every call site — grep for the key name afterward, watching for false
substring hits on shared prefixes (e.g. `wzSub` vs the still-valid
`wzSubmitting`).
