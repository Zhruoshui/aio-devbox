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

// ── on-demand services (prd D4) ────────────────────────────────────

/** The service ids whose pane starts their container on demand (D4:
 * code-server only). Its container is profile-gated and NOT started with
 * the sandbox (mgr's compose up carries only the vnc profile - mgr/src/
 * docker.rs UP_PROFILES), so the manifest's `enabled` (a TCP probe of
 * app:8200) is false exactly when clicking the button is supposed to start
 * it. Mirrors the backend whitelist (mgr/src/routes.rs ON_DEMAND_SERVICES,
 * compose service "code-server") and the manifest id (app/services.toml
 * "codeServer") - keep the three in sync. Consumers: SandboxTree shows
 * these entries regardless of `enabled`; WorkspacePage routes them to
 * CodeServerPane (the probe -> start -> poll machine). */
export const ON_DEMAND_SERVICE_IDS: ReadonlySet<string> = new Set(["codeServer"]);
