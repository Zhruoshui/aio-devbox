// XtermPane - generic pane for type === "agent" services (terminal, opencode,
// future codex, ...). Opens an xterm.js Terminal and a WebSocket pty bridge to
// /api/term/ws?cmd=<service.cmd>.
//
// Phase C note: the backend /api/term/ws route does NOT exist yet (it is a 502
// reserved seam until Phase E). So the WebSocket upgrade fails immediately.
// This pane must degrade gracefully: write a clear "backend available in
// Phase E" line, attempt at most ONE reconnect, then stop - no crash, no
// retry-spam. Once Phase E wires the pty WS, this pane works unchanged.
//
// A new agent service only needs a services.toml entry - no new React component
// (design §14A).

import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import type { ServiceEntry } from "../types";

const MAX_RECONNECT_ATTEMPTS = 1;

/** Build the same-origin pty WebSocket URL for a service command. */
function buildWsUrl(cmd: string): string {
  const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${proto}//${window.location.host}/api/term/ws?cmd=${encodeURIComponent(cmd)}`;
}

export function XtermPane({ service }: { service: ServiceEntry }): JSX.Element {
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    const term = new Terminal({
      fontFamily: "monospace",
      fontSize: 13,
      cursorBlink: true,
    });
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(el);
    // The container may be 0x0 on first paint; fit safely.
    safeFit(fitAddon);

    let currentWs: WebSocket | null = null;
    let disposed = false;
    let reconnectAttempts = 0;

    // Wire terminal -> ws once (avoids stacking handlers across reconnects).
    // Keystrokes go as Text; terminal size changes go as a 5-byte Binary
    // control frame [0x01, cols_le, rows_le] (matches terminal.rs) so the pty
    // is resized (TIOCSWINSZ) and the shell/TUI redraws at the pane's size.
    // Without this the pty stays at 80x24 and a full-screen TUI like opencode
    // renders at the wrong width, leaving a black gap on the right.
    const sendResize = () => {
      if (currentWs && currentWs.readyState === WebSocket.OPEN) {
        const c = term.cols;
        const r = term.rows;
        const b = new Uint8Array(5);
        b[0] = 0x01;
        b[1] = c & 0xff;
        b[2] = (c >> 8) & 0xff;
        b[3] = r & 0xff;
        b[4] = (r >> 8) & 0xff;
        currentWs.send(b);
      }
    };
    term.onData((data) => {
      if (currentWs && currentWs.readyState === WebSocket.OPEN) {
        currentWs.send(data);
      }
    });
    term.onResize(sendResize);

    const connect = () => {
      if (disposed) return;
      const ws = new WebSocket(buildWsUrl(service.cmd ?? ""));
      currentWs = ws;

      ws.onopen = () => {
        reconnectAttempts = 0;
        term.writeln("\r\n\x1b[32m● Terminal connected.\x1b[0m");
        // Sync the pty to the current terminal size immediately (before the
        // first fit-driven onResize), so the shell/TUI starts at the right size.
        sendResize();
      };
      ws.onmessage = (ev) => {
        if (typeof ev.data === "string") term.write(ev.data);
      };
      ws.onerror = () => {
        // Swallow; onclose handles the user-facing message + reconnect.
      };
      ws.onclose = () => {
        if (disposed) return;
        term.writeln(
          "\r\n\x1b[33m● Terminal backend will be available in Phase E.\x1b[0m",
        );
        if (reconnectAttempts < MAX_RECONNECT_ATTEMPTS) {
          reconnectAttempts += 1;
          window.setTimeout(connect, 1000);
        }
      };
    };
    connect();

    // Refit on container resize (golden-layout splitter drags, window resize).
    const resizeObserver = new ResizeObserver(() => safeFit(fitAddon));
    resizeObserver.observe(el);

    return () => {
      disposed = true;
      resizeObserver.disconnect();
      currentWs?.close();
      term.dispose();
    };
  }, [service]);

  return <div className="pane pane-xterm" ref={containerRef} />;
}

function safeFit(fitAddon: FitAddon): void {
  try {
    fitAddon.fit();
  } catch {
    // Element not yet visible/sized; the ResizeObserver will retry.
  }
}
