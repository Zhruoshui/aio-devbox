// IframePane - generic pane for type === "web" services (code-server, vnc,
// piWeb, user web buttons, ...). Ported from web/src/panes/IframePane.tsx
// (Phase 2); the ONLY change is the src computation, which now resolves
// through paneUrl.urlFor against the pane's bound sandbox (the workbench
// assumed same-origin gateway paths; mgr-web embeds a different sandbox in
// each pane).
//
// iframe drag-capture trick: a transparent .drag-overlay sits over the
// iframe. It is hidden by default; WorkspacePage toggles an `is-dragging`
// class on the golden-layout root while a splitter/tab drag is in progress,
// and CSS reveals the overlay then - so the iframe cannot swallow the
// pointer events needed to rearrange/resize panes.
//
// This component is generic: a new web service only needs a services.toml
// entry (+ container/profile/caddy route) - no new React component.

import type { ServiceEntry } from "../types";
import { urlFor } from "../paneUrl";

export function IframePane({
  service,
  sandbox,
}: {
  service: ServiceEntry;
  sandbox: string;
}): JSX.Element {
  const src = urlFor(service, sandbox);
  return (
    <div className="pane pane-iframe-wrap">
      <iframe className="pane-iframe" src={src} title={`${service.label}@${sandbox}`} />
      <div className="drag-overlay" aria-hidden="true" />
    </div>
  );
}
