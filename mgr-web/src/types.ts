// mgr API payload contract - the SINGLE owner of the /api/* shapes on the
// frontend side (cross-layer-thinking-guide: decode once at the boundary, do
// not re-cast payload fields inline elsewhere; mirrors web/src/types.ts's
// convention against app/src/config.rs).
//
// Backend owners (mgr/src/*.rs) - keep these mirrors exact:
//   GET  /api/scenarios      routes.rs scenarios      (scenario::ScenarioMeta)
//   GET  /api/sandboxes      routes.rs sandbox_json   (db::SandboxRow + ps)
//   GET  /api/sandboxes/:n   same shape as the list item
//   PUT  /api/sandboxes/:n   jobs.rs (async) -> {job, name}
//   POST /api/sandboxes      same -> {job, name}
//   GET  /api/images         routes.rs list_images    (db images table)
//   GET  /api/jobs/:id       routes.rs get_job        (state::JobShared)

// ── GET /api/scenarios ─────────────────────────────────────────────

export interface Scenario {
  id: string;
  name: string;
  description: string;
  category: string;
  /** Locked scenarios are baked unconditionally (node/python/pi/pi-web);
   * only their VERSION is selectable - same semantics as the config TUI. */
  always_on: boolean;
  /** Pre-selected label; null = unversioned or no default. */
  default_version: string | null;
  /** Selectable labels; empty = unversioned (checkbox only, no dropdown). */
  versions: string[];
}

export interface ScenarioList {
  scenarios: Scenario[];
}

// ── GET/POST /api/sandboxes, GET/PUT /api/sandboxes/:name ──────────

/** Sandbox env selection - mirrors mgr/src/envhash.rs SandboxEnv (the
 * canonical env_json format). scenarios MUST NOT contain always_on ids
 * (server-side validation rejects them; always_on scenarios appear only
 * in versions). */
export interface SandboxEnv {
  scenarios: string[];
  versions: Record<string, string>;
}

export interface SandboxService {
  service: string;
  name: string;
  state: string;
  status: string;
}

export interface Sandbox {
  name: string;
  /** DB intent: creating | running | stopped | error | adopted. */
  status: string;
  /** What compose ps actually says: running | stopped | gone | unknown. */
  live: string;
  adopted: boolean;
  created_at: number;
  cpus: number | null;
  mem_mb: number | null;
  env: SandboxEnv;
  image: string;
  entry_url: string;
  piweb_url: string;
  services: SandboxService[];
}

export interface SandboxList {
  sandboxes: Sandbox[];
}

/** POST /api/sandboxes. `cpus`/`mem_mb`: null/absent/0 = no limit
 * (normalized server-side to null). */
export interface CreateBody {
  name: string;
  env: SandboxEnv;
  cpus?: number | null;
  mem_mb?: number | null;
}

/** PUT /api/sandboxes/:name merge semantics (routes.rs put_sandbox):
 *   absent field  -> keep current value
 *   number > 0    -> set the limit
 *   0             -> CLEAR the limit (unlimited)
 */
export interface PutBody {
  env?: SandboxEnv;
  cpus?: number | null;
  mem_mb?: number | null;
}

/** Reply of POST /api/sandboxes, PUT /api/sandboxes/:name and DELETE. */
export interface JobReply {
  job: number;
  name: string;
}

// ── GET /api/images ────────────────────────────────────────────────

export interface Image {
  env_hash: string;
  tag: string;
  built_at: number | null;
  refcount: number;
  build_log: string;
}

export interface ImageList {
  images: Image[];
}

// ── GET /api/jobs/:id ──────────────────────────────────────────────

/** One job's status - mirrors mgr/src/state.rs JobShared EXACTLY. */
export type Job = {
  id: number;
  kind: "create" | "recreate" | "delete";
  sandbox: string | null;
  status: "running" | "ok" | "error";
  error: string | null;
  log: string;
};

/** Extract the error message from a non-2xx mgr API response body
 * ({error: "..."} JSON, see routes.rs ApiError). Falls back to the status. */
export async function apiError(r: Response): Promise<string> {
  try {
    const body = (await r.json()) as { error?: unknown };
    if (typeof body.error === "string" && body.error) return body.error;
  } catch {
    /* non-JSON body - fall through to the status line */
  }
  return `HTTP ${r.status}`;
}
