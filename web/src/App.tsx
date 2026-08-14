// App - workspace shell.
//
// Fetches GET /api/manifest and renders a collapsible left sidebar of LAUNCHER
// buttons + a golden-layout workspace on the right. Each click on a button
// creates a NEW instance (click again = another instance); instances are tabs
// that can be dragged into split/tiled layouts. Closing happens via the tab's
// close icon (golden-layout native), which releases the pane component and -
// for agent panes - ends the pty (XtermPane cleanup closes the WS).
//
//   type === "web"   -> IframePane (iframe embedding a containerized service)
//   type === "agent" -> XtermPane  (xterm.js over the /api/term/ws pty WS)
//
// Button visibility is server-driven by `enabled` (web: TCP-reachable;
// agent: command_exists on PATH), so a button only appears when the capability
// is actually present - no dead panes. User-registered buttons are created via
// POST /api/buttons (persisted in buttons.toml on the workspace volume).
//
// golden-layout is imperative; this component owns ONE GoldenLayout instance
// in a ref, mounted into a container <div>. Pane components are registered
// with a single factory that mounts a React root (createRoot) into the
// golden-layout-provided container.element. Roots are unmounted on
// `beforeComponentRelease` so panes torn down by close/drag-out are cleaned up.
//
// The service payload travels as componentState (opaque JsonValue); it is
// decoded back to a typed ServiceEntry through ONE type guard
// (readServiceState) rather than inline casts - the manifest contract has a
// single owner on each side of the boundary (cross-layer-thinking-guide).

import { createRoot, type Root } from "react-dom/client";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  GoldenLayout,
  type ComponentContainer,
  type JsonValue,
  type LayoutConfig,
} from "golden-layout";
import "golden-layout/dist/css/goldenlayout-base.css";
import "golden-layout/dist/css/themes/goldenlayout-light-theme.css";

import type { Manifest, ServiceEntry } from "./types";
import { Sidebar } from "./Sidebar";
import { IframePane } from "./panes/IframePane";
import { XtermPane } from "./panes/XtermPane";
import "./styles.css";

type Status = "loading" | "error" | "ready";

const PANE_COMPONENT_TYPE = "aio-pane";
const TERMINAL_ID = "terminal";
const COLLAPSE_KEY = "aio.sidebar.collapsed";

export function App(): JSX.Element {
  const containerRef = useRef<HTMLDivElement>(null);
  const glRef = useRef<GoldenLayout | null>(null);
  // React roots per golden-layout component container (unmount on release).
  const rootsRef = useRef(new WeakMap<ComponentContainer, Root>());
  // Per-service instance counter for tab titles: "Terminal", "Terminal (2)", ...
  const seqRef = useRef<Record<string, number>>({});

  const [status, setStatus] = useState<Status>("loading");
  const [errorMsg, setErrorMsg] = useState("");
  const [services, setServices] = useState<ServiceEntry[]>([]);
  const [collapsed, setCollapsed] = useState<boolean>(
    () => localStorage.getItem(COLLAPSE_KEY) === "1",
  );

  // Persist sidebar collapse state.
  useEffect(() => {
    localStorage.setItem(COLLAPSE_KEY, collapsed ? "1" : "0");
  }, [collapsed]);

  const fetchManifest = useCallback(async (): Promise<ServiceEntry[]> => {
    const r = await fetch("/api/manifest");
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    return (await r.json() as Manifest).services;
  }, []);

  // Initial load.
  useEffect(() => {
    let cancelled = false;
    fetchManifest()
      .then((svcs) => {
        if (cancelled) return;
        setServices(svcs);
        setStatus("ready");
      })
      .catch((e) => {
        if (cancelled) return;
        setErrorMsg(e instanceof Error ? e.message : String(e));
        setStatus("error");
      });
    return () => {
      cancelled = true;
    };
  }, [fetchManifest]);

  // Refresh: re-fetch the manifest. Open instances are left alone (their
  // sessions stay alive); a service that disappeared simply loses its button.
  const refresh = useCallback(() => {
    fetchManifest().then(setServices).catch(() => {
      /* keep current state on refresh failure */
    });
  }, [fetchManifest]);

  // Re-fetch when the window regains focus (catches runtime tool installs /
  // profile changes without a manual refresh). Ignore very rapid refocuses.
  const lastRefresh = useRef(0);
  useEffect(() => {
    const onFocus = () => {
      const now = Date.now();
      if (now - lastRefresh.current > 2000) {
        lastRefresh.current = now;
        refresh();
      }
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refresh]);

  // Launch ONE new instance of a service as a golden-layout component. The
  // default placement puts it in the focused stack (or the root stack), so a
  // click adds a tab next to what the user is working on; dragging the tab
  // header splits/tiles the workspace.
  const launch = useCallback((service: ServiceEntry) => {
    const gl = glRef.current;
    if (!gl) return;
    const n = (seqRef.current[service.id] ?? 0) + 1;
    seqRef.current[service.id] = n;
    const title = n === 1 ? service.label : `${service.label} (${n})`;
    gl.newComponent(PANE_COMPONENT_TYPE, { service, seq: n }, title);
  }, []);

  // Build golden-layout once the manifest is ready. Runs once (guarded by
  // glRef) - manifest refreshes must NOT rebuild the layout.
  useEffect(() => {
    if (status !== "ready") return;
    const el = containerRef.current;
    if (!el || glRef.current) return;
    const enabled = services.filter((s) => s.enabled);
    if (enabled.length === 0) return; // empty-state message rendered instead

    const gl = new GoldenLayout(el);
    gl.resizeWithContainerAutomatically = true;

    gl.registerComponentFactoryFunction(
      PANE_COMPONENT_TYPE,
      (container: ComponentContainer, state: JsonValue | undefined) => {
        const service = readServiceState(state);
        if (!service) return undefined;
        const root = createRoot(container.element);
        rootsRef.current.set(container, root);
        root.render(<PaneForService service={service} />);

        // golden-layout emits beforeComponentRelease before tearing down a
        // component (tab close / drag-out / layout destroy). Unmount the React
        // tree so its effects (xterm WS, iframe, observers) are cleaned up.
        container.on("beforeComponentRelease", () => {
          const r = rootsRef.current.get(container);
          if (r) {
            r.unmount();
            rootsRef.current.delete(container);
          }
        });
        return undefined;
      },
    );

    // Default layout: a single stack (one page) holding one terminal instance
    // when the terminal is enabled - which it always is (bash exists).
    const terminal = enabled.find((s) => s.id === TERMINAL_ID) ?? enabled[0];
    seqRef.current[terminal.id] = 1;
    const config: LayoutConfig = {
      root: {
        type: "stack",
        content: [
          {
            type: "component",
            componentType: PANE_COMPONENT_TYPE,
            componentState: { service: terminal, seq: 1 },
            title: terminal.label,
          },
        ],
      },
      settings: { reorderEnabled: true },
    };
    gl.loadLayout(config);
    glRef.current = gl;

    // iframe drag-capture: while a splitter or tab/header drag is in progress,
    // set an `is-dragging` class on the layout root so CSS reveals the
    // transparent overlay over iframes (IframePane) and they stop swallowing
    // pointer events. pointerup anywhere ends the drag.
    const onPointerDown = (e: PointerEvent) => {
      const target = e.target as HTMLElement | null;
      if (target?.closest(".lm_splitter, .lm_header, .lm_tab")) {
        el.classList.add("is-dragging");
      }
    };
    const onPointerUp = () => {
      el.classList.remove("is-dragging");
    };
    el.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("pointerup", onPointerUp);

    return () => {
      el.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("pointerup", onPointerUp);
      gl.destroy();
      glRef.current = null;
    };
    // `services` intentionally not a dep: the layout is built once from the
    // first manifest; refreshes only update the sidebar.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status]);

  const enabledServices = services.filter((s) => s.enabled);

  if (status === "loading") return <div className="status">Loading workspace…</div>;
  if (status === "error")
    return <div className="status error">Failed to load manifest: {errorMsg}</div>;
  if (enabledServices.length === 0) {
    return (
      <div className="status">
        No buttons available. Start a compose profile (e.g.{" "}
        <code>--profile code-server</code>) or bake a scenario into the image.
      </div>
    );
  }

  return (
    <div className="app">
      <Sidebar
        services={services}
        collapsed={collapsed}
        onToggleCollapse={() => setCollapsed((c) => !c)}
        onLaunch={launch}
        onRefresh={refresh}
        onRegister={registerButton}
        onDelete={deleteButton}
      />
      <main className="main">
        <div className="gl-root" ref={containerRef} />
      </main>
    </div>
  );

  // Register a user button via POST /api/buttons, then refresh so it appears
  // (command_exists is probed on the next manifest fetch).
  async function registerButton(label: string, cmd: string): Promise<boolean> {
    try {
      const r = await fetch("/api/buttons", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ label, cmd }),
      });
      if (!r.ok) return false;
      refresh();
      return true;
    } catch {
      return false;
    }
  }

  async function deleteButton(id: string): Promise<void> {
    try {
      const r = await fetch(`/api/buttons/${encodeURIComponent(id)}`, {
        method: "DELETE",
      });
      if (!r.ok && r.status !== 404) return;
    } catch {
      return;
    }
    refresh();
  }
}

/** Render the generic pane for a service by its type. */
function PaneForService({ service }: { service: ServiceEntry }): JSX.Element {
  if (service.type === "web") return <IframePane service={service} />;
  return <XtermPane service={service} />;
}

/**
 * Decode the golden-layout componentState back to a typed ServiceEntry.
 * Single decoder for the manifest payload on the pane side - callers must not
 * cast `state.service` inline (cross-layer-thinking-guide: one owner).
 */
function readServiceState(state: JsonValue | undefined): ServiceEntry | undefined {
  if (!state || typeof state !== "object") return undefined;
  const maybe = state as { service?: unknown };
  return isServiceEntry(maybe.service) ? maybe.service : undefined;
}

function isServiceEntry(v: unknown): v is ServiceEntry {
  if (typeof v !== "object" || v === null) return false;
  const s = v as Record<string, unknown>;
  return (
    typeof s.id === "string" &&
    (s.type === "web" || s.type === "agent") &&
    typeof s.enabled === "boolean"
  );
}
