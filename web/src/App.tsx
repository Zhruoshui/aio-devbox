// App - workspace shell.
//
// Fetches GET /api/manifest, filters to enabled services, and renders one
// generic pane per service in a golden-layout tiling workspace:
//   type === "web"   -> IframePane (iframe embedding a containerized service)
//   type === "agent" -> XtermPane  (xterm.js over the /api/term/ws pty WS)
//
// golden-layout is imperative; this component owns ONE GoldenLayout instance in
// a ref, mounted into a container <div>. Component items are registered with a
// single factory that mounts a React root (createRoot) into the
// golden-layout-provided container.element, rendering the pane for that
// service. Roots are unmounted on golden-layout's `beforeComponentRelease`
// event so panes torn down by drag/close are cleaned up.
//
// The service payload travels as componentState (opaque JsonValue); it is
// decoded back to a typed ServiceEntry through ONE type guard
// (readServiceState) rather than inline casts - the manifest contract has a
// single owner on each side of the boundary (cross-layer-thinking-guide).

import { createRoot, type Root } from "react-dom/client";
import { useEffect, useRef, useState } from "react";
import {
  GoldenLayout,
  type ComponentContainer,
  type JsonValue,
  type LayoutConfig,
} from "golden-layout";
import "golden-layout/dist/css/goldenlayout-base.css";
import "golden-layout/dist/css/themes/goldenlayout-light-theme.css";

import type { Manifest, ServiceEntry } from "./types";
import { PANE_COMPONENT_TYPE, buildLayoutConfig } from "./layout";
import { IframePane } from "./panes/IframePane";
import { XtermPane } from "./panes/XtermPane";
import "./styles.css";

type Status = "loading" | "error" | "ready";

export function App(): JSX.Element {
  const containerRef = useRef<HTMLDivElement>(null);
  const glRef = useRef<GoldenLayout | null>(null);
  const [status, setStatus] = useState<Status>("loading");
  const [errorMsg, setErrorMsg] = useState("");
  const [enabledServices, setEnabledServices] = useState<ServiceEntry[]>([]);

  // Fetch the manifest once.
  useEffect(() => {
    let cancelled = false;
    fetch("/api/manifest")
      .then((r) => {
        if (!r.ok) throw new Error(`HTTP ${r.status}`);
        return r.json() as Promise<Manifest>;
      })
      .then((data) => {
        if (cancelled) return;
        setEnabledServices(data.services.filter((s) => s.enabled));
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
  }, []);

  // Build golden-layout once the manifest is ready and there is at least one
  // enabled service.
  useEffect(() => {
    if (status !== "ready") return;
    if (enabledServices.length === 0) return;
    const el = containerRef.current;
    if (!el || glRef.current) return;

    const gl = new GoldenLayout(el);
    gl.resizeWithContainerAutomatically = true;

    // Track React roots so we never leak one when a pane is released.
    const roots = new WeakMap<ComponentContainer, Root>();

    gl.registerComponentFactoryFunction(
      PANE_COMPONENT_TYPE,
      (container: ComponentContainer, state: JsonValue | undefined) => {
        const service = readServiceState(state);
        if (!service) return undefined;
        const root = createRoot(container.element);
        roots.set(container, root);
        root.render(<PaneForService service={service} />);

        // golden-layout emits beforeComponentRelease before tearing down a
        // component (close / drag-out / layout destroy). Unmount the React
        // tree so its effects (xterm, ws, observers) are cleaned up.
        container.on("beforeComponentRelease", () => {
          const r = roots.get(container);
          if (r) {
            r.unmount();
            roots.delete(container);
          }
        });
        return undefined;
      },
    );

    const config: LayoutConfig = buildLayoutConfig(enabledServices);
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
  }, [status, enabledServices]);

  if (status === "loading") {
    return <div className="status">Loading workspace…</div>;
  }
  if (status === "error") {
    return <div className="status error">Failed to load manifest: {errorMsg}</div>;
  }
  if (enabledServices.length === 0) {
    return (
      <div className="status">
        No services enabled. Start a compose profile (e.g.{" "}
        <code>--profile code-server</code>) to add panes.
      </div>
    );
  }
  return <div className="gl-root" ref={containerRef} />;
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
