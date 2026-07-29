// Manifest contract - mirrors the axum backend `ManifestEntry` / `Manifest`
// types in app/src/config.rs EXACTLY. This is the single owner of the
// /api/manifest payload shape on the frontend (see cross-layer-thinking-guide:
// decode once at the boundary, do not re-cast payload fields inline elsewhere).
//
// Backend serialization (config.rs):
//   #[serde(rename = "type")] service_type: ServiceType   -> "web" | "agent"
//   #[serde(skip_serializing_if = "Option::is_none")] url -> omitted when None
//   #[serde(skip_serializing_if = "Option::is_none")] cmd -> omitted when None

export type ServiceType = "web" | "agent";

export interface ServiceEntry {
  id: string;
  type: ServiceType;
  enabled: boolean;
  /** Gateway path the iframe opens. Present only for type === "web". */
  url?: string;
  /** Command launched in the pty ("" = default shell). Present only for type === "agent". */
  cmd?: string;
}

export interface Manifest {
  services: ServiceEntry[];
}
