// golden-layout config builder. Maps the enabled-services list (from
// /api/manifest) to a tiling layout tree: one component content item per
// enabled service, each carrying the service as componentState. A single
// service becomes a stack (so it gets a tab header); multiple become a row so
// panes split the viewport and can be dragged/resized.
//
// The componentType is a fixed string registered once in App.tsx; golden-layout
// calls the registered factory for each item, which mounts the right generic
// pane (IframePane / XtermPane) by service.type. Adding a service is config in
// services.toml - no new React component, no layout change here (design §14).

import type { LayoutConfig } from "golden-layout";
import type { ServiceEntry } from "./types";

/** Registered component-type name for all workspace panes. */
export const PANE_COMPONENT_TYPE = "aio-pane";

/** Build the golden-layout config for a set of enabled services. */
export function buildLayoutConfig(services: ServiceEntry[]): LayoutConfig {
  const content = services.map((service) => ({
    type: "component" as const,
    componentType: PANE_COMPONENT_TYPE,
    // golden-layout treats componentState as an opaque JsonValue; the factory
    // reads it back through readServiceState() (App.tsx) - the single decoder.
    componentState: { service } as unknown as Record<string, unknown>,
    title: service.id,
  }));

  const root =
    services.length === 1
      ? { type: "stack" as const, content }
      : { type: "row" as const, content };

  return {
    root,
    settings: { reorderEnabled: true },
  };
}
