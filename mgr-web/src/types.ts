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
//   POST /api/sandboxes/adopt  routes.rs adopt_sandbox (sync) -> {name,
//                            entry_url, piweb_url}
//   DELETE /api/sandboxes/:n jobs.rs (async) -> {job, ...} for native rows,
//                            routes.rs unadopt_sandbox (sync) -> {ok} for
//                            adopted rows (DeleteReply below)
//   GET  /api/images         routes.rs list_images    (db images table;
//                            R3: + env/services structured combo detail)
//   GET  /api/jobs/:id       routes.rs get_job        (state::JobShared)

// ── GET /api/scenarios ─────────────────────────────────────────────

export interface Scenario {
  id: string;
  name: string;
  description: string;
  category: string;
  /** How the tool is installed: "mise" | "apt" | "npm" | "tarball". EnvPicker
   * groups a layer's rows by this and renders a sub-heading per rung, but
   * ONLY when the layer mixes installers (L3 = mise tools + the apt system
   * toolchain). Mirrors the config TUI's scenario::installer_rank. */
  installer: string;
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
 * in versions). user_tools (修正 3, R4): the selected custom-tool ids (L5) —
 * not scenario-catalog entries; they ride env.user_tools into the env_hash. */
export interface SandboxEnv {
  scenarios: string[];
  versions: Record<string, string>;
  /** 修正 3: selected custom-tool ids (the L5 layer). Absent/undefined on old
   * payloads = no custom tools; normalize to [] at the use site. */
  user_tools?: string[];
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
 * the create page always sends it). `user_tools` (修正 3): selected custom-tool
 * ids; absent = none. */
export interface CreateBody {
  name: string;
  env: SandboxEnv;
  services?: ServicesInput;
  user_tools?: string[];
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
  /** S3: readable combo description; null on pre-S3 rows (page falls back
   * to the env_hash). */
  combo: string | null;
  /** R3: structured build-time env (scenarios + versions) powering the
   * combo-detail panel; null on rows recorded before R3 (the panel then
   * shows a fallback note). */
  env: SandboxEnv | null;
  /** R3: build-time code-server/vnc switches; pi/pi-web derive from
   * env.scenarios (S1 single-owner). null on pre-R3 rows. */
  services: { code_server: boolean; vnc: boolean } | null;
  /** S3: live image size in bytes (docker inspect); null = failed / gone. */
  size_bytes: number | null;
  /** R3: the create/recreate job whose FULL log built this image (GET
   * /api/images/:env_hash/log). null on pre-R3 rows - the log view falls
   * back to the build_log column tail. */
  job_id: number | null;
}

export interface ImageList {
  images: Image[];
}

// ── GET/PUT /api/user-tools (R2, 修正 2 simplified) ────────────────
//
// The user-tools fragment (`scenarios/user-tools/fragment.Dockerfile`) — the
// escape hatch for tools mise cannot manage. mgr owns the file's FORMAT
// (mgr/src/usertools.rs); these types mirror it. 修正 2 removed the
// structured form: the page is a single direct-edit text editor, so the
// wire shape dropped `entries` / `parse_note` and gained `tools` (the
// display-only list of tool names parsed out of the `# --- tool:` marker
// blocks).

// ── usertools store (10-20; mirrors mgr/src/usertools/) ────────────

/** How a downloaded artifact lands (recipe.rs UrlKind). */
export type UrlKind = "binary" | "tarball";

/** 安装配方 — tagged union mirroring recipe.rs `Install` (kebab-case
 * `type`, snake_case wire). One variant per form type; adding a kind is a
 * backend variant + one entry here. */
export type Install =
  | { type: "apt"; packages: string[] }
  | { type: "url"; url: string; kind: UrlKind; bins: string[] }
  | {
      type: "github-release";
      repo: string;
      tag: string;
      asset: string;
      kind: UrlKind;
      bins: string[];
    }
  | { type: "script"; script: string; check?: string | null }
  | { type: "raw"; dockerfile: string };

/** The shelf categories (recipe.rs CATEGORIES; unknown values degrade to
 * the raw string — never break on the unknown). */
export const TOOL_CATEGORIES = ["shell", "document", "media", "network", "dev", "other"] as const;
export type ToolCategory = (typeof TOOL_CATEGORIES)[number];

/** One tool recipe — recipe.toml on disk, POST/PUT body on the wire. */
export interface Recipe {
  schema: number;
  name: string;
  description: string;
  category: string;
  version: string;
  homepage: string;
  install: Install;
}

/** install_type vocabulary: Install::label() + "custom" for recipe-less
 * (legacy/hand-written) dirs. */
export type InstallType = "apt" | "url-binary" | "url-tarball" | "script" | "raw" | "custom";

/** One shelf card — GET /api/usertools/store `mine` and `builtin` rows in
 * the same shape so the SPA renders one grid for both. */
export interface ToolCard {
  id: string;
  name: string;
  description: string;
  category: string;
  version: string;
  homepage: string;
  install_type: InstallType;
  has_recipe: boolean;
  /** Sandboxes whose env selects this tool (sorted; empty = unreferenced). */
  referenced_by: string[];
}

export interface StoreShelf {
  mine: ToolCard[];
  builtin: ToolCard[];
}

// ── GET /api/jobs/:id ──────────────────────────────────────────────

/** One job's status - mirrors mgr/src/state.rs JobShared EXACTLY. */
export type Job = {
  id: number;
  kind: "create" | "recreate" | "delete" | "image-delete" | "image-cleanup";
  sandbox: string | null;
  status: "running" | "ok" | "error";
  error: string | null;
  /** The in-memory tail (mgr caps it at 64KB, jobs.rs LOG_TAIL): the live
   * view while the job runs. The full output is the R3 log file below. */
  log: string;
  /** R3: on-disk FULL log (`<data>/jobs/<id>.log`) for create/recreate jobs;
   * null for short lifecycle jobs (the log endpoint then serves `log`). */
  log_path?: string | null;
};

// ── GET /api/jobs/:id/log, /api/images/:env_hash/log (R3) ──────────

/** One page of a log. `data` is BYTE-offset-addressed (the backend walks
 * both ends onto char boundaries), so a pager must advance with
 * `next_offset` - never with `data.length` (JS string length is UTF-16
 * units, not bytes). `next_offset === total_size` means the end. */
export interface LogChunk {
  data: string;
  total_size: number;
  next_offset: number;
  /** The file this page came from; null on the memory/column fallback. */
  log_path: string | null;
  /** false = capped fallback (memory tail / build_log column), not the full
   * on-disk log - the UI labels it accordingly. */
  complete: boolean;
}

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
