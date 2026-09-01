# Hook Guidelines

## Current usage

The codebase uses React built-in hooks only (`useEffect`, `useRef`, `useState`).
No custom hooks are extracted yet - the logic is component-local.

## Patterns (follow these when adding code)

- **`useEffect` for imperative lifecycle**: xterm/golden-layout/WS setup runs in
  `useEffect`, with the deps array naming what triggers re-setup (e.g.
  `[service]` in `XtermPane.tsx`, `[status, enabledServices]` in `App.tsx`).
- **`useRef` for instances you must not re-create** every render: the
  `GoldenLayout` instance, the xterm `Terminal`, the container `<div>`.
- **`useState` for render-driving UI state**: `status`, `errorMsg`,
  `enabledServices` in `App.tsx`.
- **Cancellation flags in async effects**: `let cancelled = false; ... return () => {
  cancelled = true; }` (see the manifest fetch in `App.tsx`) - prevents setting
  state after unmount.

## When to extract a custom hook

Extract only when the SAME logic is reused across components. Today nothing
qualifies - prefer keeping the xterm/golden-layout lifecycle inline with its
component so the cleanup is visible next to the setup.
