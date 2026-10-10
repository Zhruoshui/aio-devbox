// ModelsPane - the sandbox's agent model-config pane (10-10-models-
// workspace-pane R2/D2). A THIN wrapper: everything inside is the cc-switch
// SandboxModelConfigTab that used to be the sandbox edit page's "模型配置"
// tab (10-04-models-config Phase 4) - four agent sub-tabs (claude/codex
// switch-style, pi/opencode incremental), the template-library dropdown, all
// reads/writes still going through mgr's /api/sbx/:name proxy. Moving the UI
// here changed only its HOST: the create/edit flow no longer carries model
// config, and the pane opens from the sandbox tree exactly like terminal /
// code-server (title "模型配置@<name>", one pane per click, per-(sandbox,
// service) seq pool for the "(2)" suffixes).
//
// Rendered by PaneForService for the client-synthesized manifest entry
// { id: "modelsConfig", type: "page" } (workspace/types.ts
// injectSyntheticServices) - never for a services.toml-baked entry, which is
// why unknown page ids keep the generic placeholder.
//
// `status` is a CREATION-TIME snapshot of the sandbox's live state (the
// golden-layout component factory builds panes outside React's prop flow -
// same "latest value for imperative code" pattern as langRef, and the same
// accepted staleness: an already-open pane keeps its snapshot across a
// mid-session stop; its saves then fail through the tab's normal error
// path). Unknown ("") resolves optimistically to running: the popout-child
// path builds panes before the first sandbox-list poll lands, and a wrong
// guess only degrades to the tab's load-error message - never a dead hint
// for a live sandbox.
//
// Layout: the tab's form was designed for the 1200px .page container; a
// golden-layout pane can be much narrower. .pane-models (styles.css) gives
// the pane a scrollable box whose content keeps a min-width floor, so the
// cc-switch form grids stay usable and the pane scrolls instead of
// crushing the inputs.

import type { Lang } from "../../../i18n";
import { SandboxModelConfigTab } from "../../models/SandboxModelConfigTab";

export function ModelsPane({
  sandbox,
  status,
  lang,
}: {
  sandbox: string;
  status: string;
  lang: Lang;
}): JSX.Element {
  return (
    <div className="pane pane-models">
      <SandboxModelConfigTab
        sandboxName={sandbox}
        sandboxStatus={status === "" ? "running" : status}
        lang={lang}
      />
    </div>
  );
}
