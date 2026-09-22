// XtermPane - generic pane for type === "agent" services (terminal, pi,
// opencode, user-registered buttons, ...). Opens an xterm.js Terminal and a
// WebSocket pty bridge to the pane's sandbox via mgr's proxy:
// /api/sbx/<sandbox>/api/term/ws?cmd=<service.cmd>[&cwd=...] (same-origin on
// mgr, forwarded by mgr/src/proxy.rs). Ported from web/src/panes/XtermPane.tsx
// (Phase 2) - only the WS URL construction changed; the pane contracts are
// verbatim (spec frontend/xterm-pane.md):
//
//   - lineHeight 1.25 is explicit (Linux mono fallbacks crowd at 1.0);
//   - closing = unmount: the effect cleanup closes the WS - the backend pty
//     process exits on WS close. Reopening mounts a fresh pane = fresh
//     session (the "close kills, reopen restarts" toggle contract);
//   - if the WS drops mid-session the pane writes a notice and attempts at
//     most ONE reconnect, then stops - no crash, no retry-spam;
//   - keystrokes go as Text frames; size changes go as a 5-byte Binary
//     control frame [0x01, cols_le, cols_hi, rows_le, rows_hi] so the pty is
//     resized (TIOCSWINSZ) and the shell/TUI redraws at the pane's size;
//   - the server replies with its own 5-byte Binary control frame
//     [0x02, exit_code_le_u32] right before closing the WS when the pty
//     child exits (09-18-term-web-polish R6, normal exit 0 included). The
//     pane writes a "process exited" notice and then does nothing else -
//     the close that follows drives the existing disconnect/reconnect path,
//     so the lifecycle contract is untouched.
//
// Addons (09-18-term-web-polish): webgl (GPU renderer; DOM fallback on
// context loss or load failure), clipboard (OSC52 - the pty program copies
// to the host clipboard when the browser allows), web-links (URLs
// clickable) and search (in-pane Ctrl+F bar, R5). The two WebGL pitfalls
// (CSS-var fontFamily and context-loss fallback) are commented inline.
//
// A new agent button only needs a manifest entry (services.toml built-in or
// a user-registered buttons.toml entry) - no new React component.

import { useEffect, useRef, useState } from "react";
import { Terminal, type ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { ClipboardAddon, BrowserClipboardProvider } from "@xterm/addon-clipboard";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { SearchAddon } from "@xterm/addon-search";
import "@xterm/xterm/css/xterm.css";

import { fmt, t, type Lang } from "../../../i18n";
import type { ServiceEntry } from "../types";
import { termWsUrl } from "../paneUrl";

const MAX_RECONNECT_ATTEMPTS = 1;

/**
 * Resolve the Kumo --font-mono token (styles.css) into a concrete font stack.
 * The WebGL renderer draws glyphs via canvas `ctx.font`, which does NOT
 * resolve CSS variables - passing the literal `var(--font-mono)` would
 * silently fall back to the browser default font under WebGL (the DOM
 * renderer resolves vars fine, so this only bites once the GPU renderer is
 * active). Same getComputedStyle pattern as readTermTheme below.
 */
function readTermFont(): string {
  const v = getComputedStyle(document.documentElement)
    .getPropertyValue("--font-mono")
    .trim();
  return v || "monospace";
}

export function XtermPane({
  service,
  sandbox,
  lang,
}: {
  service: ServiceEntry;
  sandbox: string;
  lang: Lang;
}): JSX.Element {
  const containerRef = useRef<HTMLDivElement>(null);
  // R5 search bar. React state renders the bar; refs bridge the imperative
  // side (the addon handle for the button handlers, the term for refocus).
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const searchInputRef = useRef<HTMLInputElement>(null);
  const searchAddonRef = useRef<SearchAddon | null>(null);
  const termRef = useRef<Terminal | null>(null);
  // Mirror of searchOpen for the terminal effect's closures - state updates
  // are async and the custom key handler must see the current value.
  const searchOpenRef = useRef(false);

  /** Single funnel for open/close so the ref mirror, decoration cleanup and
   * terminal refocus stay in sync (the effect's Escape path, the input's
   * Escape path and the cleanup all go through this). */
  const setSearchVisible = (visible: boolean): void => {
    searchOpenRef.current = visible;
    setSearchOpen(visible);
    if (!visible) {
      searchAddonRef.current?.clearDecorations();
      termRef.current?.focus();
    }
  };

  // Focus the input after React commits - Ctrl+F fires while the bar may not
  // be in the DOM yet (first open), so the key handler can't focus directly.
  useEffect(() => {
    if (searchOpen) searchInputRef.current?.focus();
  }, [searchOpen]);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    const term = new Terminal({
      // Concrete font stack, NOT the "var(--font-mono)" literal - see
      // readTermFont (WebGL's ctx.font cannot resolve CSS variables).
      fontFamily: readTermFont(),
      fontSize: 13,
      // Widens the inter-line gap above the font's intrinsic (tight on Linux
      // `monospace` fallbacks) line box so glyphs don't crowd adjacent rows.
      lineHeight: 1.25,
      // Default 1000 is too small for builds/logs (R1).
      scrollback: 10000,
      cursorBlink: true,
      // Colors follow the --term-* tokens in styles.css (surface + ANSI-16
      // per active theme), read at mount and re-read when the app switches
      // theme scheme (observer below).
      theme: readTermTheme(),
    });
    termRef.current = term;
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(el);
    // The container may be 0x0 on first paint; fit safely.
    safeFit(fitAddon);

    // R3: GPU renderer. Both failure paths leave the DOM renderer active
    // (xterm swaps renderers on addon load/dispose, so a failed or disposed
    // WebGL addon = the pre-existing DOM rendering):
    //   - constructor/loadAddon throwing (no WebGL context, shader compile
    //     failure, devtools GPU disable) is caught here;
    //   - a context lost AFTER a successful load fires onContextLoss, whose
    //     handler disposes the addon.
    // The theme hot-switch (modeObserver below) only sets term.options.theme,
    // which re-renders under BOTH renderers - no WebGL-specific handling.
    //
    // Held in an effect-scoped local so the cleanup can dispose it FIRST —
    // see the cleanup comment; clearing it here is what keeps a context-loss
    // dispose from racing the unmount dispose.
    let webgl: WebglAddon | null = null;
    try {
      webgl = new WebglAddon();
      // Disposing on context loss throws the SAME internal `_isDisposed`
      // error as the unmount path below (observed 2026-09-22 by forcing a
      // real loss via WEBGL_lose_context): by the time the loss handler
      // runs, xterm's renderer has already torn down its disposable guards,
      // and they read `term._core._store` unguarded. The addon is dead at
      // this point and the DOM renderer takes over, so swallow it - an
      // uncaught error here would fire on every real GPU context loss.
      webgl.onContextLoss(() => {
        try {
          webgl?.dispose();
        } catch {
          /* already torn down; DOM renderer continues */
        }
      });
      term.loadAddon(webgl);
    } catch {
      // DOM renderer stays; the pane remains usable.
      webgl = null;
    }
    // R2: OSC52 - programs write escape sequences to push the selection /
    // system clipboard to the host (agent "copy" actions reach the host).
    // The browser provider's writeText REJECTS on a denied permission or an
    // unfocused document, and xterm's WriteBuffer re-throws async
    // parser-handler errors as uncaught microtask exceptions - so an
    // unguarded rejection would turn every denied copy into console noise
    // instead of the silent degradation R2's acceptance requires. Swallow
    // clipboard failures; a query reports an empty clipboard on denial.
    const browserClipboard = new BrowserClipboardProvider();
    term.loadAddon(
      new ClipboardAddon(undefined, {
        readText: async (sel) => {
          try {
            return await browserClipboard.readText(sel);
          } catch {
            return ""; // Denied/unavailable: report an empty clipboard.
          }
        },
        writeText: async (sel, data) => {
          try {
            await browserClipboard.writeText(sel, data);
          } catch {
            // Permission denied / document unfocused: silent no-op.
          }
        },
      }),
    );
    // R4: URLs in the output become clickable; the default handler opens a
    // new tab.
    term.loadAddon(new WebLinksAddon());
    // R5: search machinery - the UI bar is React state (below); the addon
    // only finds/highlights and lives in a ref for the bar's handlers.
    const searchAddon = new SearchAddon();
    searchAddonRef.current = searchAddon;
    term.loadAddon(searchAddon);

    // R5: Ctrl+F opens the search bar (returning false stops xterm from
    // ALSO processing the key, i.e. no ^F reaches the pty; preventDefault
    // stops the browser's own find dialog); Escape closes it while the
    // terminal - not the search input - has focus.
    term.attachCustomKeyEventHandler((ev: KeyboardEvent): boolean => {
      if (ev.type !== "keydown") return true;
      if (ev.ctrlKey && !ev.metaKey && !ev.altKey && (ev.key === "f" || ev.key === "F")) {
        ev.preventDefault();
        setSearchVisible(true);
        return false;
      }
      if (ev.key === "Escape" && searchOpenRef.current) {
        ev.preventDefault();
        setSearchVisible(false);
        return false;
      }
      return true;
    });

    let currentWs: WebSocket | null = null;
    let disposed = false;
    let reconnectAttempts = 0;

    // Wire terminal -> ws once (avoids stacking handlers across reconnects).
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
      const ws = new WebSocket(termWsUrl(sandbox, service.cmd ?? "", service.cwd));
      currentWs = ws;
      // binaryType shapes received Binary frames as ArrayBuffers (the 0x02
      // exit-code notice); text frames still arrive as strings, and the
      // Uint8Array resize send above is unaffected.
      ws.binaryType = "arraybuffer";

      ws.onopen = () => {
        reconnectAttempts = 0;
        term.writeln("\r\n\x1b[32m● Terminal connected.\x1b[0m");
        // Sync the pty to the current terminal size immediately (before the
        // first fit-driven onResize), so the shell/TUI starts at the right size.
        sendResize();
      };
      ws.onmessage = (ev) => {
        if (typeof ev.data === "string") {
          term.write(ev.data);
          return;
        }
        // 0x02 exit-code frame (server -> client): parse the LE u32 and
        // write the notice - nothing else. The server closes the WS right
        // after this frame, and the onclose below runs the existing
        // disconnect notice + at-most-one-reconnect logic (lifecycle
        // contract unchanged). Any other binary shape is ignored.
        if (!(ev.data instanceof ArrayBuffer) || ev.data.byteLength !== 5) return;
        const view = new DataView(ev.data);
        if (view.getUint8(0) !== 0x02) return;
        const code = view.getUint32(1, true);
        term.writeln(`\r\n\x1b[33m● ${fmt(lang, "termExitNotice", code)}\x1b[0m`);
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

    // Refit on container resize (window resize, tree width change, tab shown).
    // A hidden tab (inactive, display:none) reports size 0; safeFit tolerates
    // that and the observer fires again once the tab is visible.
    const resizeObserver = new ResizeObserver(() => safeFit(fitAddon));
    resizeObserver.observe(el);

    // Live retint on theme switch: App flips <html data-theme> (scheme key)
    // and <html data-mode> (the scheme's static light/dark class), the token
    // values change, and the running terminal re-reads them - without
    // reconnecting the pty (so the session survives a theme toggle). Both
    // attributes are watched: either flip alone must retint (App writes both
    // on every switch, but a scheme whose mode is unchanged only changes
    // data-theme meaningfully). This keeps working under WebGL: setting
    // term.options.theme re-renders the GPU renderer too (it rebuilds its
    // palette on the option change).
    const modeObserver = new MutationObserver(() => {
      term.options.theme = readTermTheme();
    });
    modeObserver.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-mode", "data-theme"],
    });

    return () => {
      disposed = true;
      resizeObserver.disconnect();
      modeObserver.disconnect();
      currentWs?.close();
      // Reset the bar state before tearing the term down (decoration cleanup
      // is covered by term.dispose; the React state must not survive).
      setSearchVisible(false);
      // Dispose the WebGL addon BEFORE term.dispose(), deliberately.
      //
      // term.dispose() also disposes every loaded addon, but it does so while
      // the terminal's core is already torn down - and the WebGL renderer's
      // internal disposables guard themselves by reading
      // `term._core._store._isDisposed`, so that read throws
      // "Cannot read properties of undefined (reading '_isDisposed')" on
      // every unmount (observed 2026-09-22: one uncaught error per page
      // navigation away from the workspace).
      //
      // Disposing it here runs those same disposables while the core is still
      // alive. Calling the addon's dispose is safe to repeat: loadAddon
      // swaps it for xterm's _wrappedAddonDispose, which no-ops when already
      // disposed AND splices the addon out of term._addons - so the
      // term.dispose() below won't touch it a second time. That also makes
      // the onContextLoss handler above (same wrapped dispose) non-racing.
      try {
        webgl?.dispose();
      } catch {
        // Already disposed (context loss) or the addon never loaded.
      }
      term.dispose();
      termRef.current = null;
      searchAddonRef.current = null;
    };
    // `lang` is deliberately NOT a dep: panes keep their creation language
    // for the session (WorkspacePage.langRef contract - the golden-layout
    // factory renders each pane once, so the prop never actually changes);
    // re-running this effect would kill the live pty session.
  }, [service, sandbox]);

  return (
    // Fragment: the terminal div is exactly as before (golden-layout's
    // .lm_content hosts the React root; .pane fills it 100%); the search bar
    // is a SIBLING overlay - never a child of the xterm container, whose DOM
    // belongs to term.open(). As a sibling it is position:absolute against
    // .lm_content (position: relative per goldenlayout-base.css), whose box
    // is identical to .pane-xterm's since .pane fills it 100%.
    <>
      <div className="pane pane-xterm" ref={containerRef} />
      {searchOpen && (
        <div className="term-searchbar">
          <input
            ref={searchInputRef}
            className="term-searchbar-input"
            value={searchQuery}
            placeholder={t(lang, "termSearchPlaceholder")}
            onChange={(e) => setSearchQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                if (!searchQuery) return;
                if (e.shiftKey) searchAddonRef.current?.findPrevious(searchQuery);
                else searchAddonRef.current?.findNext(searchQuery);
              } else if (e.key === "Escape") {
                e.preventDefault();
                setSearchVisible(false);
              } else if (e.ctrlKey && (e.key === "f" || e.key === "F")) {
                // Already open; keep the browser's find dialog closed
                // (keydowns in the input never reach the terminal-side
                // custom key handler).
                e.preventDefault();
              }
            }}
          />
          <button
            type="button"
            className="term-searchbar-btn"
            aria-label={t(lang, "termSearchPrev")}
            // Keep focus in the input while clicking (finds don't need the
            // button focused; typing continues right after).
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => {
              if (searchQuery) searchAddonRef.current?.findPrevious(searchQuery);
            }}
          >
            ↑
          </button>
          <button
            type="button"
            className="term-searchbar-btn"
            aria-label={t(lang, "termSearchNext")}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => {
              if (searchQuery) searchAddonRef.current?.findNext(searchQuery);
            }}
          >
            ↓
          </button>
        </div>
      )}
    </>
  );
}

function safeFit(fitAddon: FitAddon): void {
  try {
    fitAddon.fit();
  } catch {
    // Element not yet visible/sized; the ResizeObserver will retry.
  }
}

/**
 * Resolve the --term-* tokens (styles.css) of the ACTIVE theme into an
 * xterm theme. The surface colors stay CSS color strings (oklch /
 * color-mix; the DOM renderer applies them as CSS colors, and the WebGL
 * renderer's css.toColor parses the shipped Kumo values — the scheme
 * blocks since 09-20-theme-schemes use hex anyway). The ANSI-16 entries
 * are read from --term-ansi-* tokens, which are hex for every theme
 * (WebGL's color parser is unreliable beyond hex — design.md §3.2), and
 * map to the camelCase ITheme fields (bright-black -> brightBlack).
 */
function readTermTheme(): ITheme {
  const style = getComputedStyle(document.documentElement);
  const v = (name: string): string => style.getPropertyValue(name).trim();
  // ANSI-16 from the --term-ansi-* tokens (styles.css defines every
  // theme's 16 hex values; token suffix "bright-x" maps to ITheme's
  // camelCase brightX). Written out field by field: ITheme's index
  // signature admits `string[]` values (gradient colors), so a
  // keyed loop would need a cast — an explicit literal needs none.
  return {
    background: v("--term-bg"),
    foreground: v("--term-fg"),
    cursor: v("--term-fg"),
    cursorAccent: v("--term-bg"),
    selectionBackground: v("--term-selection"),
    black: v("--term-ansi-black"),
    red: v("--term-ansi-red"),
    green: v("--term-ansi-green"),
    yellow: v("--term-ansi-yellow"),
    blue: v("--term-ansi-blue"),
    magenta: v("--term-ansi-magenta"),
    cyan: v("--term-ansi-cyan"),
    white: v("--term-ansi-white"),
    brightBlack: v("--term-ansi-bright-black"),
    brightRed: v("--term-ansi-bright-red"),
    brightGreen: v("--term-ansi-bright-green"),
    brightYellow: v("--term-ansi-bright-yellow"),
    brightBlue: v("--term-ansi-bright-blue"),
    brightMagenta: v("--term-ansi-bright-magenta"),
    brightCyan: v("--term-ansi-bright-cyan"),
    brightWhite: v("--term-ansi-bright-white"),
  };
}
