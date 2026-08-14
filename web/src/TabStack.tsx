// TabStack - tab bar + main area.
//
// All open tabs stay MOUNTED; inactive ones are hidden via CSS (display:none).
// This keeps their sessions alive across tab switches: the xterm WebSocket
// (pty) and the iframe (code-server/noVNC) are not torn down just because the
// user looked at another tab. Closing a tab removes it from the list, which
// unmounts the pane - XtermPane's cleanup then closes the WS and the pty
// process exits (the "close = kill, reopen = fresh session" toggle contract).

import type { ServiceEntry } from "./types";
import { IframePane } from "./panes/IframePane";
import { XtermPane } from "./panes/XtermPane";

interface Props {
  tabs: ServiceEntry[];
  activeId: string | null;
  onActivate: (id: string) => void;
  onClose: (id: string) => void;
}

export function TabStack({ tabs, activeId, onActivate, onClose }: Props): JSX.Element {
  return (
    <div className="stack">
      <div className="tabbar">
        {tabs.map((t) => (
          <div
            key={t.id}
            className={`tab${t.id === activeId ? " active" : ""}`}
            role="tab"
            aria-selected={t.id === activeId}
            onClick={() => onActivate(t.id)}
          >
            <span className="tab-label">{t.label}</span>
            <button
              className="tab-close"
              title={`Close ${t.label}`}
              onClick={(e) => {
                e.stopPropagation();
                onClose(t.id);
              }}
            >
              ✕
            </button>
          </div>
        ))}
        {tabs.length === 0 && (
          <span className="tabbar-empty">No panels open — pick a button on the left.</span>
        )}
      </div>
      <div className="pane-area">
        {tabs.map((t) => (
          <div
            key={t.id}
            className={`pane-slot${t.id === activeId ? " visible" : ""}`}
            aria-hidden={t.id !== activeId}
          >
            {t.type === "web" ? <IframePane service={t} /> : <XtermPane service={t} />}
          </div>
        ))}
      </div>
    </div>
  );
}
