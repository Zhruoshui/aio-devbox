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
//   PUT  /api/sandboxes/:n/model_profile  routes.rs put_model_profile (sync
//                            kv write, never a job - unified Phase 4, D8)
//   POST /api/sandboxes      same -> {job, name}
//   POST /api/sandboxes/adopt  routes.rs adopt_sandbox (sync) -> {name,
//                            entry_url, piweb_url}
//   DELETE /api/sandboxes/:n jobs.rs (async) -> {job, ...} for native rows,
//                            routes.rs unadopt_sandbox (sync) -> {ok} for
//                            adopted rows (DeleteReply below)
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
  /** Assigned model profile id, null = unassigned (the sandbox keeps its
   * local models.json; mgr never overwrites it - unified Phase 4, D8). */
  model_profile: string | null;
  /** Runtime container states (compose ps per service). */
  services: SandboxService[];
  /** S1: services installed at create time (read-only — fixed by the image
   * content). pi/pi_web are derived from env.scenarios by the backend. */
  installed_services: {
    code_server: boolean;
    vnc: boolean;
    pi: boolean;
    pi_web: boolean;
  };
}

/** S1: the four-switch services request shape. All `true` by default (the
 * UI sends the full set; absent = server-side default all-on for old
 * clients). pi_web forces pi + vnc on the backend. */
export interface ServicesInput {
  code_server: boolean;
  vnc: boolean;
  pi: boolean;
  pi_web: boolean;
}

export interface SandboxList {
  sandboxes: Sandbox[];
}

/** POST /api/sandboxes. `cpus`/`mem_mb`: null/absent/0 = no limit
 * (normalized server-side to null). `services` absent = all-on (old clients;
 * the create page always sends it). */
export interface CreateBody {
  name: string;
  env: SandboxEnv;
  services?: ServicesInput;
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

// ── POST /api/sandboxes/adopt (Phase 5: register an external stack) ─

/** Adopt body: register an existing RUNNING compose stack under mgr
 * (design §3.8). Service names default to the repo stack's gateway / app
 * (backend routes.rs DEFAULT_*_SERVICE). */
export interface AdoptBody {
  name: string;
  /** Compose file of the external stack; relative paths resolve against
   * the mgr repo root (routes.rs resolve_compose_path). */
  compose_path: string;
  gateway_service?: string;
  app_service?: string;
}

/** Synchronous adopt reply - no job: aliasing two containers and regenerating
 * the gateway Caddyfile is seconds at most. */
export interface AdoptReply {
  name: string;
  entry_url: string;
  piweb_url: string;
}

/** DELETE /api/sandboxes/:name reply is shape-polymorphic: native rows run a
 * job (compose down -v can take a while on big volumes); adopted rows
 * un-register synchronously (nothing of theirs is torn down) and reply {ok}.
 * Branch with isJobReply - the UI stays on the list for the adopted case. */
export type DeleteReply = JobReply | { ok: boolean; name: string };

export function isJobReply(r: DeleteReply): r is JobReply {
  return typeof (r as JobReply).job === "number";
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
