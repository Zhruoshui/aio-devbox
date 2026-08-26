// IframePane - generic pane for type === "web" services (code-server, vnc,
// future jupyter, ...). Embeds the service's gateway path in an iframe.
//
// iframe drag-capture trick: a transparent .drag-overlay sits over the iframe.
// It is hidden by default; App.tsx toggles an `is-dragging` class on the
// golden-layout root while a splitter/tab drag is in progress, and CSS reveals
// the overlay then - so the iframe cannot swallow the pointer events needed to
// rearrange/resize panes.
//
// This component is generic: a new web service only needs a services.toml entry
// (+ container/profile/caddy route) - no new React component.

import type { ServiceEntry } from "../types";

export function IframePane({ service }: { service: ServiceEntry }): JSX.Element {
  // `{host}` placeholder: services that publish their own port (pi-web on
  // 30141) cannot hardcode a hostname - the workbench may be browsed via
  // localhost or a LAN IP, and the manifest url is static. Substitute at
  // render time with whatever hostname the workbench itself was reached on.
  // Path-style urls (code-server, vnc) contain no placeholder and pass through.
  const src = service.url?.replace("{host}", window.location.hostname);
  return (
    <div className="pane pane-iframe-wrap">
      <iframe className="pane-iframe" src={src} title={service.label} />
      <div className="drag-overlay" aria-hidden="true" />
    </div>
  );
}
