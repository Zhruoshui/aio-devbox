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
  type UserToolsFile,
} from "./types";
import {
  decodeCatalog,
  decodeConfig,
  decodeUsageFanout,
  type CanonicalConfig,
  type CatalogResponse,
  type DiscoverResponse,
  type ImportResponse,
  type PutResponse,
  type TestResponse,
  type UsageFanout,
} from "./pages/models/types";
import { decodeManifest, type Manifest, type RegisterButtonInput } from "./pages/workspace/types";

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

// ── user-tools fragment (R2, 修正 2 simplified; mirrors mgr/src/usertools.rs) ──
//
// Single direct-edit view: the save stores `content` verbatim (comments and
// blank lines round-trip), then the backend re-syncs the per-tool scenario
// dirs. The save is followed by a re-GET so the page always renders the
// file's actual state (and the tool list reflects the just-written markers).

export function getUserTools(): Promise<UserToolsFile> {
  return get("/api/user-tools");
}

/** Direct-editor save. The backend 400s on the `/root` hard rule with a
 * line-numbered message; on success it also syncs the per-tool scenario
 * dirs under scenarios/. */
export function putUserTools(content: string): Promise<{ ok: boolean }> {
  return send("/api/user-tools", "PUT", { content });
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

// ── models / usage (Phase 4c; mirrors mgr/src/models.rs + usage.rs) ─
//
// Error bodies here are mgr's {"error": "..."} JSON (models.rs ApiError) -
// apiError() surfaces the message, so callers only ever handle exceptions.
// Payloads whose shape the workbench also decoded (config / catalog / usage)
// decode through pages/models/types.ts; the small literal replies (put /
// import / discover / test) are typed casts, same as web's ModelsPane.
//
// Multi-profile (unified Phase 4, D8): the config/import/discover/test calls
// take an OPTIONAL profile id -> ?profile= on the wire (models.rs
// ProfileQuery; absent = the first profile). Callers that always know the
// selection (ModelsPage) pass it explicitly once >1 profile exists.

export function getModelsConfig(profile?: string): Promise<CanonicalConfig> {
  return get<unknown>(`/api/models/config${profile ? `?profile=${enc(profile)}` : ""}`).then(
    decodeConfig,
  );
}

export function putModelsConfig(
  config: CanonicalConfig,
  profile?: string,
): Promise<PutResponse> {
  return send(
    `/api/models/config${profile ? `?profile=${enc(profile)}` : ""}`,
    "PUT",
    config,
  );
}

export function importPiModels(profile?: string): Promise<ImportResponse> {
  return send(`/api/models/import/pi${profile ? `?profile=${enc(profile)}` : ""}`, "POST");
}

/** Discover body: `{providerId}` resolves from the store; the literal form
 * probes a provider being edited (with a freshly typed key) before saving. */
export function discoverModels(
  body: { providerId: string } | { baseUrl: string; api: string; apiKey?: string },
  profile?: string,
): Promise<DiscoverResponse> {
  return send(
    `/api/models/discover${profile ? `?profile=${enc(profile)}` : ""}`,
    "POST",
    body,
  );
}

/** Test body: `{providerId, modelId}` resolves the provider from the store;
 * the literal form probes a provider being edited (with a freshly typed
 * key) before saving — same split as {@link discoverModels}. */
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
  profile?: string,
): Promise<TestResponse> {
  return send(
    `/api/models/test${profile ? `?profile=${enc(profile)}` : ""}`,
    "POST",
    body,
  );
}

export function getModelsCatalog(): Promise<CatalogResponse> {
  return get<unknown>("/api/models/catalog").then(decodeCatalog);
}

// ── model profiles (unified Phase 4, D8; mirrors mgr/src/models.rs) ─

/** One row of GET /api/models/profiles: identity + usage, NO config (the
 * page fetches the selected profile's config separately). `assigned` is the
 * list of sandbox names on this profile. */
export interface ModelProfile {
  id: string;
  name: string;
  version: number;
  assigned: string[];
}

export interface ModelProfileList {
  profiles: ModelProfile[];
}

export function listModelProfiles(): Promise<ModelProfileList> {
  return get("/api/models/profiles");
}

/** POST /api/models/profiles {name} — the backend owns the id. */
export function createModelProfile(name: string): Promise<{ id: string; name: string }> {
  return send("/api/models/profiles", "POST", { name });
}

/** PUT /api/models/profiles/:id — rename and/or replace the config (the
 * config branch is the same masked-echo merge pipeline as putModelsConfig;
 * ModelsPage uses the dedicated putModelsConfig with ?profile= instead, so
 * callers here only rename). */
export function renameModelProfile(id: string, name: string): Promise<PutResponse> {
  return send(`/api/models/profiles/${enc(id)}`, "PUT", { name });
}

/** DELETE refuses the last profile (backend 400s) and unassigns every
 * sandbox pointing at the deleted one. */
export function deleteModelProfile(id: string): Promise<{ ok: boolean; id: string }> {
  return send(`/api/models/profiles/${enc(id)}`, "DELETE");
}

/** PUT /api/sandboxes/:name/model_profile — assign (`{profile: id}`) or
 * UNassign (`{profile: null}`). A pure kv write on the backend: the
 * sandbox's 60s pull picks it up, no recreate job.
 *
 * S2 agent subset: `agents` narrows which agent configs the pull renders —
 * null/absent = all four (legacy wire shape), [] = zero agents (the sandbox
 * keeps its local configs untouched), an array = the explicit subset. */
export function putSandboxModelProfile(
  name: string,
  profile: string | null,
  agents?: string[] | null,
): Promise<{ ok: boolean; name: string; model_profile: string | null }> {
  return send(`/api/sandboxes/${enc(name)}/model_profile`, "PUT", {
    profile,
    ...(agents !== undefined ? { agents } : {}),
  });
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
