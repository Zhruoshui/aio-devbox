# State Management

## No state library

No Redux/Zustand/Jotai. State is local and flows down as props.

## Where state lives

- **`App.tsx`** is the single owner of:
  - `status` ("loading" | "error" | "ready"), `errorMsg`, `enabledServices`
    (from `GET /api/manifest`).
  - The `GoldenLayout` instance (`glRef`) and its React-root `WeakMap`.
- **Panes** (`IframePane`, `XtermPane`) own only their imperative instance
  (xterm terminal, WS) in `useRef`; no shared state.

## Data flow

`App.tsx` fetches the manifest -> filters `enabled` services -> builds the
golden-layout tree (`buildLayoutConfig`) -> each component item carries its
`service` as `componentState` -> the factory decodes it (`readServiceState`) and
renders `<PaneForService service={...} />`. No prop drilling beyond one level;
no global store.

## golden-layout owns pane layout state

Tab order, split sizes, drag positions are golden-layout's state, not React's.
React roots are mounted INTO golden-layout containers; React does not manage
pane arrangement.

### Layout persistence (`aio.layout`, localStorage)

golden-layout state is persisted by the GL init effect in `App.tsx`:

- **Save**: `gl.on("stateChanged")` → 500ms debounce → JSON of
  `ResolvedLayoutConfig.minifyConfig(gl.saveLayout())` under `aio.layout`.
  The save itself is wrapped in try/catch — a failure (e.g. saving while a
  popout is open) is non-fatal and the next change retries.
- **Restore**: the init effect tries
  `unminifyConfig → LayoutConfig.fromResolved → headerHeight=40 → loadLayout`;
  ANY failure (missing key, corrupt JSON, unminifiable config) falls back to
  the default single-terminal layout. Never let a bad archive brick the app.
- **SUB_WINDOW guard (mandatory)**: attach the save listener ONLY in the main
  window. The popout child runs the same effect; its `stateChanged` would
  overwrite the parent's archive with the child's single-pane layout.
- **Seq pool (in-use set, smallest-free-number reuse)**: tab instance numbers
  (`seqRef[id]: Set<number>`) are NOT a monotonic counter. `launch` claims the
  smallest unused positive int; `beforeComponentRelease` (tab close / drag-out
  popout / layout destroy) returns it; restore re-adds each saved `{service,
  seq}` via `collectInUseSeq`. The component factory ALSO re-claims the seq on
  every creation — mandatory for `BrowserPopout.popIn()`, which re-creates the
  pane from persisted componentState with no `launch` call; without the
  factory re-add, popping a tab back in frees its number for a duplicate
  title. Titles of already-open tabs are never rewritten (only new launches
  reuse freed numbers).
- Known limitation: a layout saved while a popout was open may fail to
  restore (caught → default). Accepted; revisit if popouts become long-lived.
- **Reset button**: `removeItem` + `location.reload()`. After reload the
  autosave re-persists the (now default) layout — the key coming back
  non-null is CORRECT, not a bug; the observable contract is "back to the
  default single Terminal tab".
