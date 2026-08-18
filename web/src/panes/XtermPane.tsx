// XtermPane - generic pane for type === "agent" services (terminal, opencode,
// user-registered buttons, ...). Opens an xterm.js Terminal and a WebSocket
// pty bridge to /api/term/ws?cmd=<service.cmd>.
//
// Closing = unmount: App removes the tab from the list, React unmounts this
// pane, and the effect cleanup below closes the WS - the backend pty process
// exits on WS close. Reopening the button mounts a fresh pane = fresh session
// (the "close kills, reopen restarts" toggle contract).
//
// If the WS drops mid-session the pane writes a notice and attempts at most
// ONE reconnect, then stops - no crash, no retry-spam.
//
// A new agent button only needs a manifest entry (services.toml built-in or a
// user-registered buttons.toml entry) - no new React component.

import { useEffect, useRef } from "react";
import { Terminal, type ITheme } from "@xterm/xterm";
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
      fontFamily: "var(--font-mono)",
      fontSize: 13,
      // Widens the inter-line gap above the font's intrinsic (tight on Linux
      // `monospace` fallbacks) line box so glyphs don't crowd adjacent rows.
      lineHeight: 1.25,
      cursorBlink: true,
      // Colors follow the Kumo tokens in styles.css (--term-*), read at mount
      // and re-read when the app switches light/dark (observer below).
      theme: readTermTheme(),
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
        term.writeln("\r\n\x1b[33m● Terminal disconnected.\x1b[0m");
        if (reconnectAttempts < MAX_RECONNECT_ATTEMPTS) {
          reconnectAttempts += 1;
          window.setTimeout(connect, 1000);
        }
      };
    };
    connect();

    // Refit on container resize (window resize, sidebar collapse, tab shown).
    // A hidden tab (inactive, display:none) reports size 0; safeFit tolerates
    // that and the observer fires again once the tab is visible.
    const resizeObserver = new ResizeObserver(() => safeFit(fitAddon));
    resizeObserver.observe(el);

    // Live retint on theme switch: App flips <html data-mode=...>, the token
    // values change, and the running terminal re-reads them - without
    // reconnecting the pty (so the session survives a theme toggle).
    const modeObserver = new MutationObserver(() => {
      term.options.theme = readTermTheme();
    });
    modeObserver.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-mode"],
    });

    return () => {
      disposed = true;
      resizeObserver.disconnect();
      modeObserver.disconnect();
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

/**
 * Resolve the Kumo --term-* tokens (styles.css) into an xterm theme. Values
 * stay CSS color strings (oklch / color-mix); xterm's DOM renderer applies
 * them as CSS colors, so they follow [data-mode] for free.
 */
function readTermTheme(): ITheme {
  const style = getComputedStyle(document.documentElement);
  const v = (name: string): string => style.getPropertyValue(name).trim();
  return {
    background: v("--term-bg"),
    foreground: v("--term-fg"),
    cursor: v("--term-fg"),
    cursorAccent: v("--term-bg"),
    selectionBackground: v("--term-selection"),
  };
}
