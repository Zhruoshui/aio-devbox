// IframePane - generic pane for type === "web" services (code-server, vnc,
// future jupyter, ...). Embeds the service's gateway path in an iframe.
//
// This component is generic: a new web service only needs a services.toml entry
// (+ container/profile/caddy route) - no new React component.

import type { ServiceEntry } from "../types";

export function IframePane({ service }: { service: ServiceEntry }): JSX.Element {
  return (
    <div className="pane pane-iframe-wrap">
      <iframe className="pane-iframe" src={service.url} title={service.label} />
    </div>
  );
}
