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
main.tsx` (entry), `App.tsx` (shell, see below), `styles.css` (Kumo token layer
under `[data-mode]`) + `components.css` (designer component layer — rail/panel/
badge/btn/segmented/tabs/dialog/menu/pop/drawer/table/tree; **09-11 prototype
redesign**, imported after styles.css so it can only consume tokens),
`icons.tsx` (24px stroke icon set as `PATHS`/`IconName`), `pages/
WorkspacePage.tsx` (golden-layout owner), `pages/workspace/paneUrl.ts`
(per-sandbox URL factory), `pages/workspace/panes/` (`IframePane` / `XtermPane`
/ `CodeServerPane` — generic panes by service type, each bound to its
sandbox), `pages/workspace/SandboxTree.tsx` (sandbox tree panel: search,
status dots, manifest lazy-load, button registration) + `NodeMenu.tsx`
(fixed-position `更多` menu), `pages/models/` (cc-switch refactor: provider library only, no profile
bar, no five tabs; Phase 1.2/3). Admin pages in `pages/` (list/create/adopt/edit/job/images/
usage) all use the `.page` container (max-width 1200px) + `.page-head` header
scaffold. `types.ts`/`api.ts` mirror the mgr control-plane API —
backend/api-contracts.md.

**Shell (09-11 prototype redesign)**: `App.tsx` renders a 48px icon rail
(`nav.rail` — brand + five nav buttons + theme/lang in `rail-foot`) and, on
the workspace page only, the sandbox-tree side panel (`aside.panel`,
persisted under `mgr.panelHidden`; the pre-prototype 216px sidebar and its
`mgr.sidebarCollapsed` key are retired). The workspace main area is full-width
(golden-layout); every admin page is wrapped in `.page` (centered, 1200px cap)
with a `.page-head` (h1 + `.sub` + `.page-actions`). Golden-layout popout
child windows still render the lone workspace without the shell
(`IS_POPOUT_CHILD`).

## Pane types (`ServiceEntry.type`)

- `"web"` → `IframePane` (containerized service in an iframe; TCP-probed `enabled`).
- `"agent"` → `XtermPane` (pty CLI over the mgr proxy; `enabled` = `command_exists`).
- `"page"` → native React pane rendered by mgr-web itself (10-10-models-workspace-pane).
  The ONLY page entry is `modelsConfig` (ModelsPane → SandboxModelConfigTab, per-sandbox
  model config like a code-server pane), and it is **client-synthesized**: injected after
  manifest decode by `injectSyntheticServices` (`pages/workspace/types.ts`, id-dedup) at
  the single fetch site `WorkspacePage.loadManifest`. Do NOT add `type = "page"` entries
  to `app/services.toml` — it is `include_str!`-baked into per-sandbox images, so a baked
  entry never reaches existing sandboxes (09-09's "retired" note was about the old
  in-sandbox model UI; the pane mechanism itself is alive again via synthesis).

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
| [Theming](./theming.md) | filled |
