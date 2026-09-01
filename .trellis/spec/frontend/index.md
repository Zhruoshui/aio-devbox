# Frontend Development Guidelines

> React SPA (Vite + TypeScript) that renders the AIO sandbox workspace: a
> golden-layout tiling of generic panes, one per enabled service. Built in the
> app image's `web-builder` stage and served by the axum backend at `/`.

## Stack

- React 18 + TypeScript 5.6 (strict), Vite 5.
- `golden-layout` 2.6 (imperative tiling workspace).
- `@xterm/xterm` + `@xterm/addon-fit` (terminal panes).
- Build: `tsc --noEmit && vite build` (typecheck is a build gate).

## Where things live

See [directory-structure.md](./directory-structure.md). In short: `web/src/main.tsx`
(entry), `App.tsx` (workspace shell + golden-layout owner), `layout.ts`
(golden-layout config builder), `types.ts` (Manifest/ServiceEntry),
`panes/IframePane.tsx` + `panes/XtermPane.tsx` (generic panes by service type),
`panes/models/` (native in-app `"page"` pane — unified model config, split into
`ModelsPane.tsx` shell + view sub-components; `index.tsx` re-exports
`ModelsPane`. Backend contract in the backend Model Config Guide).

## Pane types (`ServiceEntry.type`)

- `"web"` → `IframePane` (containerized service in an iframe; TCP-probed `enabled`).
- `"agent"` → `XtermPane` (pty CLI; `enabled` = `command_exists`).
- `"page"` → native React pane served by axum itself (ModelsPane); `enabled`
  always true, no `url`/`cmd`. Added to `ServiceType`, `isServiceEntry`/
  `PaneForService` in `App.tsx`, sidebar "System" group, `serviceIcon`.

## Guidelines Index

| Guide | Status |
|-------|--------|
| [Directory Structure](./directory-structure.md) | filled |
| [Component Guidelines](./component-guidelines.md) | filled |
| [Hook Guidelines](./hook-guidelines.md) | filled |
| [State Management](./state-management.md) | filled |
| [Type Safety](./type-safety.md) | filled |
| [Quality Guidelines](./quality-guidelines.md) | filled |
| [Xterm Pane](./xterm-pane.md) | filled |
