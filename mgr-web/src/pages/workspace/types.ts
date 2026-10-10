// Shared types + decoders for the workspace page (sandbox-mgr unified
// Phase 2, design §2) - ported from web/src/types.ts, the deprecated
// workbench SPA's manifest contract.
//
// Single boundary owner (cross-layer-thinking-guide): these mirror the
// sandbox app's /api/manifest + /api/buttons payloads EXACTLY (backend
// owners app/src/config.rs ManifestEntry and app/src/routes/buttons.rs
// ButtonInput/ButtonOut). mgr-web reaches them through mgr's per-sandbox
// proxy /api/sbx/:name/* (mgr/src/proxy.rs, Phase 1), which forwards status
// codes and bodies verbatim - so the shapes are the app's, not mgr's.
//
// Decoding happens once here (decodeManifest / isServiceEntry); rendering
// consumes the typed ServiceEntry everywhere - never raw JSON. The same
// guard doubles as the componentState decoder for golden-layout panes
// (WorkspacePage readPaneState reuses isServiceEntry).

// ── GET /api/sbx/:name/api/manifest ───────────────────────────────

export type ServiceType = "web" | "agent" | "page";

export interface ServiceEntry {
  id: string;
  type: ServiceType;
  /** Button visible? (web: TCP-reachable; agent: command_exists on PATH). */
  enabled: boolean;
  /** Display name in the tree button / tab. */
  label: string;
  /** True for user-registered buttons (deletable in the tree). */
  deletable: boolean;
  /** Iframe src. Present only for type === "web": gateway paths
   * (/code-server/), /preview/<port>/ for user web buttons, or absolute
   * URLs (piWeb - see paneUrl.ts for how each form resolves). */
  url?: string;
  /** Command launched in the pty ("" = default shell). Present only for type === "agent". */
  cmd?: string;
  /** Initial pty working directory (backend validates and falls back to
   * /root when missing or not a directory). Present only for type === "agent". */
  cwd?: string;
}

export interface Manifest {
  services: ServiceEntry[];
}

/** Runtime guard for a manifest entry. The same tolerances as the
 * workbench's App.tsx guard: id/type/enabled are validated, everything
 * else is trusted (serde guarantees them server-side). */
export function isServiceEntry(v: unknown): v is ServiceEntry {
  if (typeof v !== "object" || v === null) return false;
  const s = v as Record<string, unknown>;
  return (
    typeof s.id === "string" &&
    (s.type === "web" || s.type === "agent" || s.type === "page") &&
    typeof s.enabled === "boolean"
  );
}

/** Decode GET /api/sbx/:name/api/manifest. A malformed envelope throws
 * (caught by the tree's error state); individual malformed entries are
 * DROPPED, never fatal - an older app image must not blank the tree. */
export function decodeManifest(json: unknown): Manifest {
  if (typeof json !== "object" || json === null) {
    throw new Error("invalid manifest payload");
  }
  const services = (json as { services?: unknown }).services;
  if (!Array.isArray(services)) {
    throw new Error("invalid manifest payload");
  }
  return { services: services.filter(isServiceEntry) };
}

// ── POST /api/sbx/:name/api/buttons ───────────────────────────────

export type RegisterButtonType = "agent" | "web";

export interface RegisterButtonInput {
  label: string;
  /** Required for type="agent" ("" accepted by TS but rejected server-side). */
  cmd?: string;
  type?: RegisterButtonType;
  /** Required for type="web"; 1-65535, 8088 rejected (the sandbox app's own
   * port - proxying it would recurse inside the sandbox). */
  port?: number;
}

// ── client-synthesized entries (10-10-models-workspace-pane D2) ────

/** Service id of the client-synthesized models-config entry. The "page"
 * render path (mgr-web React pane, no iframe/pty) is driven by entries
 * mgr-web injects AFTER the manifest decode - services.toml deliberately
 * bakes none (include_str! freezes it into each per-sandbox app image, so
 * a baked entry would never reach already-built sandboxes; see the comment
 * block at the bottom of app/services.toml). SandboxTree's serviceIcon
 * maps this id to the "sliders" glyph. */
export const MODELS_CONFIG_ID = "modelsConfig";

/** Append mgr-web's client-synthesized manifest entries to a decoded
 * manifest's services. Called by the manifest-state owner
 * (WorkspacePage.loadManifest) as the SINGLE injection point - every
 * surface (tree buttons, tab-glyph lookup, pane launch) reads the stored
 * services array, so one injection covers all consumers. Idempotent by
 * id: a future app image that bakes its own modelsConfig entry is not
 * duplicated. `label` is the caller's localized "模型配置" (the injected
 * label is a snapshot of the fetch-time lang; a mid-session language
 * switch re-localizes on the next manifest refresh - same accepted
 * limitation as pane titles). */
export function injectSyntheticServices(
  services: ServiceEntry[],
  label: string,
): ServiceEntry[] {
  if (services.some((s) => s.id === MODELS_CONFIG_ID)) return services;
  return [
    ...services,
    {
      id: MODELS_CONFIG_ID,
      type: "page",
      enabled: true,
      label,
      deletable: false,
    },
  ];
}

// ── on-demand services (prd D4) ────────────────────────────────────

/** The service ids whose pane starts their container on demand (D4:
 * code-server only). Its container is profile-gated and NOT started with
 * the sandbox (mgr's compose up carries only the vnc profile - mgr/src/
 * docker.rs UP_PROFILES), so the manifest's `enabled` (a TCP probe of
 * localhost:8200) is false exactly when clicking the button is supposed to
 * start it. Mirrors the backend whitelist (mgr/src/routes.rs
 * ON_DEMAND_SERVICES, compose service "code-server") and the manifest id
 * (app/services.toml "codeServer") - keep the three in sync. Consumers:
 * SandboxTree shows these entries regardless of `enabled` (09-20-sandbox-
 * service-buttons D2: but still behind the INSTALLED_GATE - an uninstalled
 * code-server has no service block, no image, no pane, so the button is
 * not rendered at all); WorkspacePage routes them to CodeServerPane (the
 * probe -> start -> poll machine). */
export const ON_DEMAND_SERVICE_IDS: ReadonlySet<string> = new Set(["codeServer"]);
