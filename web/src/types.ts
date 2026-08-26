// Manifest contract - mirrors the axum backend `ManifestEntry` / `Manifest`
// types in app/src/config.rs EXACTLY. This is the single owner of the
// /api/manifest payload shape on the frontend (see cross-layer-thinking-guide:
// decode once at the boundary, do not re-cast payload fields inline elsewhere).
//
// Backend serialization (config.rs):
//   #[serde(rename = "type")] service_type: ServiceType   -> "web" | "agent"
//   label: String, deletable: bool                          -> always present
//   #[serde(skip_serializing_if = "Option::is_none")] url -> omitted when None
//   #[serde(skip_serializing_if = "Option::is_none")] cmd -> omitted when None

export type ServiceType = "web" | "agent" | "page";

export interface ServiceEntry {
  id: string;
  type: ServiceType;
  /** Button visible? (web: TCP-reachable; agent: command_exists on PATH). */
  enabled: boolean;
  /** Display name in the sidebar / tab. */
  label: string;
  /** True for user-registered buttons (deletable in the UI). */
  deletable: boolean;
  /** Iframe src. Present only for type === "web". Usually a gateway path
   * (/code-server/); may be absolute with a `{host}` placeholder that
   * IframePane substitutes with window.location.hostname (pi-web's dedicated
   * port, which cannot sit behind a gateway subpath). */
  url?: string;
  /** Command launched in the pty ("" = default shell). Present only for type === "agent". */
  cmd?: string;
}

export interface Manifest {
  services: ServiceEntry[];
}

// Stats contract - mirrors the Rust `StatsSnapshot` in app/src/routes/stats.rs
// EXACTLY. Backend is the single owner of the /api/stats payload shape; this
// interface is the single frontend-side decoder target (no inline casts).
// Semantics are CONTAINER-view (cgroup v2 for CPU/mem, statvfs of the
// workspace volume for disk). `memTotalBytes: null` means the container has
// no memory limit (docker compose sets none by default).
export interface StatsSnapshot {
  /** Container CPU usage, 0-100 (relative to the effective cpu quota). */
  cpuPct: number;
  memUsedBytes: number;
  /** Absent/null = no cgroup memory limit; show absolute usage only. */
  memTotalBytes?: number;
  diskUsedBytes: number;
  diskTotalBytes: number;
}
