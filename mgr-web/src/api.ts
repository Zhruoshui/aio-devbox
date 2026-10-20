// Typed API client - the single place that performs fetch/JSON decoding for
// mgr-web (web/src does its fetches in App.tsx; mgr-web has a bigger API
// surface, so the boundary owner is its own module). Every function checks
// r.ok and surfaces the backend's {error: "..."} message (routes.rs
// ApiError) as a thrown Error - callers only ever handle exceptions.

import {
  apiError,
  type AdoptBody,
  type AdoptReply,
  type CreateBody,
  type DeleteReply,
  type ImageList,
  type Job,
  type JobReply,
  type LogChunk,
  type PutBody,
  type Sandbox,
  type SandboxList,
  type ScenarioList,
  type Recipe,
  type StoreShelf,
} from "./types";
import {
  decodeLibrary,
  decodeTemplates,
  decodeUsageFanout,
  type DiscoverResponse,
  type ProviderLibrary,
  type ProviderTemplate,
  type TemplatesResponse,
  type TestResponse,
  type UsageFanout,
} from "./pages/models/types";
import { decodeManifest, type Manifest, type RegisterButtonInput } from "./pages/workspace/types";
import {
  decodeAgents,
  decodeApplyResult,
  decodeSandboxAgentConfig,
  type AgentsResponse,
  type ApplyResult,
  type SandboxAgentConfig,
} from "./pages/models/sandboxTypes";

async function get<T>(path: string): Promise<T> {
  const r = await fetch(path);
  if (!r.ok) throw new Error(await apiError(r));
  return (await r.json()) as T;
}

async function send<T>(path: string, method: string, body?: unknown): Promise<T> {
  const r = await fetch(path, {
    method,
    headers: body === undefined ? undefined : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!r.ok) throw new Error(await apiError(r));
  return (await r.json()) as T;
}

const enc = encodeURIComponent;

// ── scenarios ──────────────────────────────────────────────────────

export function listScenarios(): Promise<ScenarioList> {
  return get("/api/scenarios");
}

// ── sandboxes ──────────────────────────────────────────────────────

export function listSandboxes(): Promise<SandboxList> {
  return get("/api/sandboxes");
}

export function getSandbox(name: string): Promise<Sandbox> {
  return get(`/api/sandboxes/${enc(name)}`);
}

export function createSandbox(body: CreateBody): Promise<JobReply> {
  return send("/api/sandboxes", "POST", body);
}

/** Adopt is synchronous on the backend (ps + two network alias connects +
 * Caddyfile regen); no job to poll. */
export function adoptSandbox(body: AdoptBody): Promise<AdoptReply> {
  return send("/api/sandboxes/adopt", "POST", body);
}

export function putSandbox(name: string, body: PutBody): Promise<JobReply> {
  return send(`/api/sandboxes/${enc(name)}`, "PUT", body);
}

/** Reply is shape-polymorphic (types.ts DeleteReply): native rows -> {job};
 * adopted rows unregister synchronously -> {ok}. */
export function deleteSandbox(name: string, volumes: boolean): Promise<DeleteReply> {
  return send(`/api/sandboxes/${enc(name)}?volumes=${volumes ? "1" : "0"}`, "DELETE");
}

/** Start/stop/restart are synchronous on the backend (compose up/stop/restart
 * inline in the handler); the reply is {ok, output}. */
export function sandboxAction(
  name: string,
  action: "start" | "stop" | "restart",
): Promise<{ ok: boolean; output: string }> {
  return send(`/api/sandboxes/${enc(name)}/${action}`, "POST");
}

/** Bring up ONE on-demand service (D4: code-server) of a running sandbox -
 * synchronous on the backend (container create+start, seconds). Backend
 * 404s on an unknown sandbox/service and 400s when the sandbox is not
 * running (a stopped stack must not come back up half-started). */
export function startSandboxService(
  name: string,
  service: "code-server",
): Promise<{ ok: boolean; service: string; output: string }> {
  return send(`/api/sandboxes/${enc(name)}/service/${service}/start`, "POST");
}

// ── images / jobs ──────────────────────────────────────────────────

export function listImages(): Promise<ImageList> {
  return get("/api/images");
}

/** POST /api/images/:env_hash/delete — one image's tag group (base/app/cs),
 * 202 {job} (S3 R3: async, poll getJob for progress). The backend 409s a
 * referenced image; the page disables those rows anyway. */
export function deleteImage(envHash: string): Promise<{ ok: boolean; job: number }> {
  return send(`/api/images/${enc(envHash)}/delete`, "POST");
}

/** POST /api/images/cleanup — every refcount=0 image group + builder cache,
 * 202 {job} (S3 R4). */
export function cleanupImages(): Promise<{ ok: boolean; job: number }> {
  return send("/api/images/cleanup", "POST");
}

export function getJob(id: number): Promise<Job> {
  return get(`/api/jobs/${id}`);
}

// ── usertools store (10-20; mirrors mgr/src/usertools/ routes) ──────
//
// The shelf is a directory scan on the mgr side — no cache layer, so a
// save/add/delete is visible on the next getToolStore() without any
// invalidation protocol.

/** The shelf view: materialized tools (with reference counts) + builtin
 * recommendations not yet materialized. */
export function getToolStore(): Promise<StoreShelf> {
  return get("/api/usertools/store");
}

/** 上架 — form recipe. id is derived (slug of the name) server-side; a
 * collision 409s, field errors 400 with a zh message. */
export function createToolFromRecipe(
  recipe: Recipe,
): Promise<{ ok: boolean; id: string }> {
  return send("/api/usertools/tools", "POST", { recipe });
}

/** 上架 — one click from the builtin shelf (materializes the seed recipe). */
export function createToolFromBuiltin(id: string): Promise<{ ok: boolean; id: string }> {
  return send("/api/usertools/tools", "POST", { from_builtin: id });
}

/** Edit a recipe tool: full replace, the backend regenerates the fragment. */
export function updateTool(id: string, recipe: Recipe): Promise<{ ok: boolean; id: string }> {
  return send(`/api/usertools/tools/${enc(id)}`, "PUT", recipe);
}

/** 下架. Referenced tools 400 with the sandbox names in the message unless
 * `force` — the UI surfaces the list and asks before forcing. */
export function deleteTool(id: string, force = false): Promise<{ ok: boolean; id: string }> {
  return send(`/api/usertools/tools/${enc(id)}${force ? "?force=true" : ""}`, "DELETE");
}

/** One tool's detail: the fragment verbatim + its recipe (null for legacy
 * fragment-only tools — the SPA routes those to the raw editor). */
export function getToolFragment(
  id: string,
): Promise<{ id: string; content: string; recipe: Recipe | null }> {
  return get(`/api/usertools/tools/${enc(id)}/fragment`);
}

/** Raw-channel save (legacy tools only; recipe tools 409 — edit the recipe). */
export function putToolFragment(id: string, content: string): Promise<{ ok: boolean }> {
  return send(`/api/usertools/tools/${enc(id)}/fragment`, "PUT", { content });
}

/** Recipe → fragment WITHOUT writing (the form's live preview; the backend
 * generator is the single owner of the templates). `id` may be null for a
 * create-preview — the backend derives it from the name. */
export function previewTool(
  id: string | null,
  recipe: Recipe,
): Promise<{ id: string; fragment: string }> {
  return send("/api/usertools/preview", "POST", { id, recipe });
}

// R3: full build logs. The backend keeps the COMPLETE output on disk
// (`<data>/jobs/<id>.log`) and serves it through two byte-addressed
// endpoints; the job view's live tail and the images page's column are just
// the fallbacks.

/** One page of a job's on-disk log (?offset/&limit are BYTES). */
export function getJobLog(id: number, offset = 0, limit = LOG_PAGE): Promise<LogChunk> {
  return get(`/api/jobs/${id}/log?offset=${offset}&limit=${limit}`);
}

/** One page of an image's build log (the file behind images.job_id, or the
 * build_log column when the row predates R3). */
export function getImageLogPage(
  envHash: string,
  offset = 0,
  limit = LOG_PAGE,
): Promise<LogChunk> {
  return get(`/api/images/${enc(envHash)}/log?offset=${offset}&limit=${limit}`);
}

/** Bytes per request. The server's own cap is 4MB (routes.rs LOG_PAGE_MAX);
 * 1MB keeps a single response snappy while making a multi-MB log a handful
 * of round trips. */
const LOG_PAGE = 1024 * 1024;
/** Page ceiling: 64MB of log is far past anything a browser <pre> should
 * hold, and bounds the loop against a server that never reports the end. */
const LOG_MAX_PAGES = 64;

/** Follow a paged log endpoint to the end, concatenating the pages. Returns
 * the SAME LogChunk shape with the whole (or page-capped) content; `complete`
 * stays false when the source itself is a fallback, and is downgraded to
 * false when the page ceiling truncated the result. */
async function readAllLog(fetchPage: (offset: number) => Promise<LogChunk>): Promise<LogChunk> {
  const parts: string[] = [];
  let offset = 0;
  let last: LogChunk | null = null;
  for (let i = 0; i < LOG_MAX_PAGES; i++) {
    const page = await fetchPage(offset);
    last = page;
    parts.push(page.data);
    if (page.next_offset >= page.total_size) {
      return { ...page, data: parts.join("") };
    }
    if (page.next_offset <= offset) break; // no progress: bail, never spin
    offset = page.next_offset;
  }
  const fallback: LogChunk = last ?? {
    data: "",
    total_size: 0,
    next_offset: 0,
    log_path: null,
    complete: false,
  };
  return { ...fallback, data: parts.join(""), complete: false };
}

/** The FULL build log of a job (R3), paged to the end. */
export function getFullJobLog(id: number): Promise<LogChunk> {
  return readAllLog((offset) => getJobLog(id, offset));
}

/** The FULL build log behind an image row (R3): the create job's on-disk log,
 * or the images.build_log column tail for pre-R3 rows. */
export function getFullImageLog(envHash: string): Promise<LogChunk> {
  return readAllLog((offset) => getImageLogPage(envHash, offset));
}

// ── models / usage (cc-switch refactor; mirrors mgr/src/models.rs) ─
//
// Error bodies here are mgr's {"error": "..."} JSON (models.rs ApiError) -
// apiError() surfaces the message, so callers only ever handle exceptions.
// Payloads decode through pages/models/types.ts (single boundary owner).
//
// Phase 1.2/3 boundary: mgr keeps ONLY the provider library. The profile
// system, the mgr-side canonical config, the pi import, the models.dev
// catalog, and the sandbox assignment routes are all gone (prd D1/D6).
// Each sandbox owns its agent configs end-to-end; the mgr library is a
// TEMPLATE source that sandbox-side pickers read via /api/models/templates.

/** GET /api/models/providers — the masked provider library. The plaintext
 * apiKey never crosses this route; PUT/POST echo the mask back. */
export function getProviderLibrary(): Promise<ProviderLibrary> {
  return get<unknown>("/api/models/providers").then(decodeLibrary);
}

/** PUT /api/models/providers — masked-echo merge + validate + whole-library
 * replace (the page saves the edited grid in one shot). */
export function putProviderLibrary(library: ProviderLibrary): Promise<{ ok: boolean }> {
  return send("/api/models/providers", "PUT", library);
}

/** POST /api/models/providers — add one provider ({id, ...template}). The
 * id comes from the caller (the frontend generates a slug); a duplicate id
 * is a 400. */
export function createProvider(
  id: string,
  template: ProviderTemplate,
): Promise<{ ok: boolean; id: string }> {
  return send("/api/models/providers", "POST", { id, ...template });
}

/** PUT /api/models/providers/:id — masked-echo merge on the apiKey
 * (empty clears, mask restores plaintext, absent keeps, other replaces). */
export function updateProvider(
  id: string,
  template: ProviderTemplate,
): Promise<{ ok: boolean; id: string }> {
  return send(`/api/models/providers/${enc(id)}`, "PUT", template);
}

/** DELETE /api/models/providers/:id. */
export function deleteProvider(id: string): Promise<{ ok: boolean; id: string }> {
  return send(`/api/models/providers/${enc(id)}`, "DELETE");
}

/** GET /api/models/templates — the template list for sandbox-side pickers,
 *  carrying the REAL apiKey (10-13 R2) so a pick prefills the key too. */
export function getProviderTemplates(): Promise<TemplatesResponse> {
  return get<unknown>("/api/models/templates").then(decodeTemplates);
}

/** Discover body: `{providerId}` resolves from the library; the literal form
 * probes a provider being edited (with a freshly typed key) before saving. */
export function discoverModels(
  body: { providerId: string } | { baseUrl: string; api: string; apiKey?: string },
): Promise<DiscoverResponse> {
  return send("/api/models/discover", "POST", body);
}

/** Test body: `{providerId, modelId}` resolves the provider from the
 * library; the literal form probes a provider being edited (with a freshly
 * typed key) before saving — same split as {@link discoverModels}. */
export function testModel(
  body:
    | { providerId: string; modelId: string; protocol?: string }
    | {
        baseUrl: string;
        api: string;
        apiKey?: string;
        modelId: string;
        protocol?: string;
      },
): Promise<TestResponse> {
  return send("/api/models/test", "POST", body);
}

export function getUsage(window: "today" | "7d" | "all"): Promise<UsageFanout> {
  return get<unknown>(`/api/usage?window=${window}`).then(decodeUsageFanout);
}

// ── per-sandbox proxied endpoints (workspace, Phase 2) ─────────────
//
// These hit the SANDBOX APP's own handlers through mgr's /api/sbx/:name
// proxy (mgr/src/proxy.rs, Phase 1) - NOT mgr routes. Payloads and status
// codes pass through verbatim (the app's shapes, see
// pages/workspace/types.ts): POST /api/buttons replies 201 + ButtonOut,
// DELETE replies 204 with an EMPTY body (unlike mgr's JSON error shape -
// hence the bespoke fetch below), and probe replies {listening}.
//
// Sandbox names are validated slugs on the backend; the proxy matches on
// the raw path, so no escaping is needed (enc() would be an identity).

export function getSandboxManifest(name: string): Promise<Manifest> {
  return get<unknown>(`/api/sbx/${name}/api/manifest`).then(decodeManifest);
}

export function registerSandboxButton(name: string, body: RegisterButtonInput): Promise<unknown> {
  return send(`/api/sbx/${name}/api/buttons`, "POST", body);
}

/** DELETE /api/buttons/:id replies 204 (empty body), so this bypasses
 * send()'s r.json(); 404 is tolerated like the workbench did (the sandbox
 * may have been deleted since the tree last refreshed). */
export async function deleteSandboxButton(name: string, id: string): Promise<void> {
  const r = await fetch(`/api/sbx/${name}/api/buttons/${encodeURIComponent(id)}`, {
    method: "DELETE",
  });
  if (!r.ok && r.status !== 404) throw new Error(await apiError(r));
}

/** TCP probe of a port on the sandbox's app netns (app's own
 * /api/buttons/probe; 1-65535, 0/8088/non-numeric -> 400). */
export function probeSandboxPort(name: string, port: number): Promise<{ listening: boolean }> {
  return get(`/api/sbx/${name}/api/buttons/probe?port=${port}`);
}

// ── per-sandbox model config (cc-switch refactor Phase 4) ──────────
//
// These hit the SANDBOX APP's own /api/models/* handlers through mgr's
// /api/sbx/:name proxy — each sandbox owns its agent configs end-to-end
// (prd D1). The wire shapes are the app's (aio-models snapshot/incremental
// types); decoding happens in pages/models/sandboxTypes.ts. A stopped
// sandbox fails at the proxy with 502 (proxy.rs never double-guesses).

/** GET /api/sbx/:name/api/models/config — the sandbox's full agent config.
 *  The app returns `{}` for a sandbox that was never configured. */
export function getSandboxModelConfig(name: string): Promise<SandboxAgentConfig> {
  return get<unknown>(`/api/sbx/${name}/api/models/config`).then(decodeSandboxAgentConfig);
}

/** PUT /api/sbx/:name/api/models/config — save the full agent config. */
export function putSandboxModelConfig(
  name: string,
  config: SandboxAgentConfig,
): Promise<{ ok: boolean }> {
  return send(`/api/sbx/${name}/api/models/config`, "PUT", config);
}

/** GET /api/sbx/:name/api/models/agents — per-agent install + live readback. */
export function getSandboxAgents(name: string): Promise<AgentsResponse> {
  return get<unknown>(`/api/sbx/${name}/api/models/agents`).then(decodeAgents);
}

/** POST /api/sbx/:name/api/models/apply/:agent — render one agent's config
 *  to its native files (claude | codex | pi | opencode). */
export function applySandboxAgent(name: string, agent: string): Promise<ApplyResult> {
  return send(`/api/sbx/${name}/api/models/apply/${enc(agent)}`, "POST").then(decodeApplyResult);
}
