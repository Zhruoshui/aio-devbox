# Frontend Development Guidelines

> React SPA (Vite + TypeScript) — **mgr-web**, the single unified UI
> (09-09-sandbox-mgr-unified D1/D2): golden-layout workspace (panes from every
> sandbox, mixed freely) + admin pages (sandboxes/images/models/usage). Built
> in mgr's `web-builder` stage and served by mgr-api at `mgr.localhost`. The
> old per-sandbox workbench `web/` is retired (tombstone in
> directory-structure.md).

## Stack

- React 18 + TypeScript 5.6 (strict), Vite 5.
- `golden-layout` 2.6 (imperative tiling workspace).
- `@xterm/xterm` + `@xterm/addon-fit` (terminal panes).
- Build: `tsc --noEmit && vite build` (typecheck is a build gate).

## Where things live

See [directory-structure.md](./directory-structure.md). In short: `mgr-web/src/
main.tsx` (entry), `App.tsx` (shell: sidebar nav + theme/lang, in-memory view
state instead of routing), `pages/WorkspacePage.tsx` (golden-layout owner),
`pages/workspace/paneUrl.ts` (per-sandbox URL factory), `pages/workspace/panes/`
(`IframePane` / `XtermPane` / `CodeServerPane` — generic panes by service
type, each bound to its sandbox), `pages/workspace/SandboxTree.tsx` (sandbox
tree + manifest lazy-load + button registration), `pages/models/` (model
config page with profile selector, D8). Admin pages in `pages/` (list/create/
adopt/edit/job/images/usage). `types.ts`/`api.ts` mirror the mgr control-plane
API — backend/api-contracts.md.

## Pane types (`ServiceEntry.type`)

- `"web"` → `IframePane` (containerized service in an iframe; TCP-probed `enabled`).
- `"agent"` → `XtermPane` (pty CLI over the mgr proxy; `enabled` = `command_exists`).
- `"page"` pane type is RETIRED with web/ (09-09 D1/D8): the in-sandbox model
  config UI is gone — model config lives in mgr-web's Models page (per-profile,
  契约 7). Do not re-introduce `"page"` panes.

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
