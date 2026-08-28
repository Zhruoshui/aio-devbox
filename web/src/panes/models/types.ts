// Shared types + decoders for the unified model config pane.
//
// Single boundary owner (cross-layer-thinking-guide): every /api/models/*
// response is decoded exactly once here, and all rendering consumes the typed
// CanonicalConfig / AgentsResponse / UsageResponse — never raw JSON.
//
// Mirrors app/src/routes/models/store.rs + mod.rs + usage.rs serde shapes.

import type { Lang } from "../../i18n";

// ── canonical config types (mirror store.rs) ──────────────────────

export interface CanonicalConfig {
  version: number;
  providers: Record<string, ProviderEntry>;
  agents: AgentsConfig;
}

export interface ProviderEntry {
  name: string;
  baseUrl: string;
  api: string;
  apiKey?: string;
  headers: Record<string, string>;
  compat: unknown;
  models: ModelEntry[];
}

export interface ModelEntry {
  id: string;
  name?: string;
  api?: string;
  reasoning?: boolean;
  contextWindow?: number;
  maxTokens?: number;
  cost?: CostEntry;
}

export interface CostEntry {
  input?: number;
  output?: number;
  cacheRead?: number;
  cacheWrite?: number;
}

/** claude is a switch-style agent: N presets, one `current` takes effect. */
export interface ClaudePreset {
  /** Backend-generated short id; "" on a freshly-created preset (backfilled on PUT). */
  id: string;
  name: string;
  provider: string;
  model: string;
  haikuModel?: string | null;
  sonnetModel?: string | null;
  opusModel?: string | null;
  authField: string;
}

export interface ClaudePresets {
  presets: ClaudePreset[];
  /** id of the active preset; unset or dangling => apply refuses. */
  current?: string | null;
}

/** codex is a switch-style agent (mirror of ClaudePresets). */
export interface CodexPreset {
  id: string;
  name: string;
  provider: string;
  model: string;
  reasoningEffort?: string | null;
  wireApi: string;
}

export interface CodexPresets {
  presets: CodexPreset[];
  current?: string | null;
}

/** pi/opencode keep a single assignment (incremental agents - design §5). */
export interface AgentAssignment {
  provider: string;
  model: string;
}

export interface AgentsConfig {
  pi?: AgentAssignment;
  opencode?: AgentAssignment;
  claude?: ClaudePresets;
  codex?: CodexPresets;
}

/** The preset-list agents (switch-style); narrows the AgentTab union. */
export type PresetAgent = "claude" | "codex";

/** Union of both preset shapes, keyed by agent for generic helpers. */
export type AnyPreset = ClaudePreset | CodexPreset;

export interface PutResponse {
  ok: boolean;
  warnings?: string[];
}

export interface ImportResponse {
  ok: boolean;
  imported: string[];
  skipped: string[];
}

// ── M2: discover + test types ─────────────────────────────────────

export interface DiscoveredModel {
  id: string;
  name?: string;
}

export interface DiscoverResponse {
  models: DiscoveredModel[];
  endpoint: string;
}

export interface TestResponse {
  ok: boolean;
  latencyMs?: number;
  status?: number;
  error?: string;
  responseText?: string;
}

// Per-(provider,model) test pill state. Keyed by `${providerId}:${modelId}`.
export interface TestStateEntry {
  status: "idle" | "testing" | "ok" | "fail";
  latencyMs?: number;
  statusHttp?: number;
  error?: string;
  responseText?: string;
}
export type TestStateMap = Record<string, TestStateEntry>;

// Discover modal state.
export interface DiscoverState {
  loading: boolean;
  error: string;
  endpoint: string;
  models: DiscoveredModel[];
  filter: string;
  selected: Set<string>;
}

// ── M3: agent status + apply types ────────────────────────────────

/** One provider configured in an agent's NATIVE config file (pi models.json /
 * opencode provider object) — the live list of 08-27-agent-tabs-live-config.
 * Fields are best-effort from the backend's tolerant extraction; name/api/
 * baseUrl may be absent (pi nodes carry no name; opencode npm may be missing). */
export interface LiveProviderSummary {
  id: string;
  name?: string | null;
  api?: string | null;
  baseUrl?: string | null;
  models: string[];
}

/** Live readback for one agent (shape varies by agent; null fields absent).
 * pi/opencode additionally carry `providers` — every provider node living in
 * the agent's native config, not just the current default. */
export interface AgentLive {
  provider?: string | null;
  model?: string | null;
  baseUrl?: string | null;
  modelProvider?: string | null;
  providers?: LiveProviderSummary[];
}

export interface AgentStatus {
  installed: boolean;
  bin: string | null;
  live: AgentLive | null;
}

/** Mirrors the Rust `AgentsResponse` in routes/models/mod.rs. */
export interface AgentsResponse {
  pi: AgentStatus;
  opencode: AgentStatus;
  claude: AgentStatus;
  codex: AgentStatus;
}

export interface ApplyWrittenFile {
  path: string;
  backup: string | null;
}

export interface ApplyErrorFile {
  path: string;
  message: string;
}

/** Mirrors the Rust `ApplyResult` in routes/models/render/common.rs. */
export interface ApplyResponse {
  ok: boolean;
  written: ApplyWrittenFile[];
  errors: ApplyErrorFile[];
}

// ── M4: usage types ───────────────────────────────────────────────

export interface UsageRow {
  agent: string;
  provider?: string | null;
  model: string;
  in: number;
  out: number;
  cacheRead: number;
  cacheWrite: number;
  cost?: number;
}

export interface UsageResponse {
  rows: UsageRow[];
  generatedAt: string;
}

/** The four agent tabs that carry a provider/model assignment. */
export type AgentTab = "pi" | "opencode" | "claude" | "codex";

// ── R1: models.dev catalog types ──────────────────────────────────

export interface CatalogModel {
  id: string;
  name?: string;
  reasoning?: boolean;
  input?: string[];
  contextWindow?: number;
  maxTokens?: number;
  cost?: CostEntry;
}

export interface CatalogProvider {
  id: string;
  name: string;
  /** Official API base URL from models.dev (absent on ~1/4 of providers). */
  api?: string;
  models: CatalogModel[];
}

export interface CatalogResponse {
  providers: CatalogProvider[];
}

// ── decoders (single boundary owner) ──────────────────────────────

function isObj(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null;
}

function asU64(v: unknown): number {
  return typeof v === "number" && v >= 0 ? Math.floor(v) : 0;
}

/** Decode the GET /api/models/usage response (single boundary owner). */
export function decodeUsage(json: unknown): UsageResponse {
  const o = isObj(json) ? json : {};
  const rawRows = Array.isArray(o.rows) ? o.rows : [];
  const rows: UsageRow[] = [];
  for (const r of rawRows) {
    if (!isObj(r)) continue;
    rows.push({
      agent: typeof r.agent === "string" ? r.agent : "",
      provider:
        typeof r.provider === "string"
          ? r.provider
          : r.provider === null
            ? null
            : undefined,
      model: typeof r.model === "string" ? r.model : "",
      in: asU64(r.in),
      out: asU64(r.out),
      cacheRead: asU64(r.cacheRead),
      cacheWrite: asU64(r.cacheWrite),
      cost: typeof r.cost === "number" ? r.cost : undefined,
    });
  }
  return {
    rows,
    generatedAt: typeof o.generatedAt === "string" ? o.generatedAt : "",
  };
}

/** Format a non-negative token count human-friendly: 1.2M / 89k. */
export function fmtTokens(n: number): string {
  if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(1)}G`;
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${Math.floor(n / 1000)}k`;
  return String(n);
}

/** Format a USD cost: 4 decimals, plain otherwise. */
export function fmtCost(n: number): string {
  return `$${n.toFixed(4)}`;
}

/** Format a 0..1 ratio as a percentage with one decimal (0.724 -> "72.4%"). */
export function fmtPct(x: number): string {
  return `${(x * 100).toFixed(1)}%`;
}

/** Total input tokens against which cache reads are measured, per agent.
 * The token conventions differ:
 *   - codex logs OpenAI-style usage where `cached_input_tokens` is a SUBSET
 *     of `input_tokens` -> denominator is `in` alone;
 *   - claude / pi / opencode log DISJOINT buckets where `in` excludes cache
 *     (verified: Anthropic usage fields are additive; pi-ai normalizes
 *     `input = promptTokens - cacheRead - cacheWrite`; opencode prices the
 *     four buckets separately against models.dev rates)
 *     -> denominator is `in + cacheRead + cacheWrite`. */
export function cacheHitDenom(r: UsageRow): number {
  return r.agent === "codex" ? r.in : r.in + r.cacheRead + r.cacheWrite;
}

/** One row's cache hit rate in 0..1, or null when nothing hit the cache. */
export function cacheHitRate(r: UsageRow): number | null {
  const denom = cacheHitDenom(r);
  return denom > 0 && r.cacheRead > 0 ? r.cacheRead / denom : null;
}

export function decodeConfig(json: unknown): CanonicalConfig {
  const o = isObj(json) ? json : {};
  const rawProviders = isObj(o.providers) ? o.providers : {};
  const providers: Record<string, ProviderEntry> = {};
  for (const [k, v] of Object.entries(rawProviders)) {
    if (isObj(v)) providers[k] = v as unknown as ProviderEntry;
  }
  return {
    version: typeof o.version === "number" ? o.version : 1,
    providers,
    agents: isObj(o.agents) ? (o.agents as AgentsConfig) : {},
  };
}

/** Decode the GET /api/models/agents response (single boundary owner). */
export function decodeAgents(json: unknown): AgentsResponse {
  const empty: AgentStatus = { installed: false, bin: null, live: null };
  if (!isObj(json)) return { pi: empty, opencode: empty, claude: empty, codex: empty };
  const decLive = (raw: unknown): AgentLive | null => {
    if (!isObj(raw)) return null;
    const live: AgentLive = raw as AgentLive;
    // Tolerant providers[] decode: malformed entries are skipped, a
    // non-array field is dropped (backend contract: absent when unreadable).
    if (Array.isArray(raw.providers)) {
      live.providers = raw.providers.filter(
        (p): p is LiveProviderSummary =>
          isObj(p) && typeof p.id === "string" && Array.isArray(p.models),
      );
    } else {
      delete live.providers;
    }
    return live;
  };
  const dec = (v: unknown): AgentStatus => {
    if (!isObj(v)) return empty;
    return {
      installed: v.installed === true,
      bin: typeof v.bin === "string" ? v.bin : null,
      live: decLive(v.live),
    };
  };
  return {
    pi: dec(json.pi),
    opencode: dec(json.opencode),
    claude: dec(json.claude),
    codex: dec(json.codex),
  };
}

/** Decode the GET /api/models/catalog response (single boundary owner). */
export function decodeCatalog(json: unknown): CatalogResponse {
  const o = isObj(json) ? json : {};
  const rawProviders = Array.isArray(o.providers) ? o.providers : [];
  const providers: CatalogProvider[] = [];
  for (const rp of rawProviders) {
    if (!isObj(rp)) continue;
    const rawModels = Array.isArray(rp.models) ? rp.models : [];
    const models: CatalogModel[] = [];
    for (const rm of rawModels) {
      if (!isObj(rm) || typeof rm.id !== "string") continue;
      const cost = isObj(rm.cost)
        ? ({
            input: typeof rm.cost.input === "number" ? rm.cost.input : undefined,
            output: typeof rm.cost.output === "number" ? rm.cost.output : undefined,
            cacheRead:
              typeof rm.cost.cacheRead === "number" ? rm.cost.cacheRead : undefined,
            cacheWrite:
              typeof rm.cost.cacheWrite === "number" ? rm.cost.cacheWrite : undefined,
          } as CostEntry)
        : undefined;
      models.push({
        id: rm.id,
        name: typeof rm.name === "string" ? rm.name : undefined,
        reasoning: typeof rm.reasoning === "boolean" ? rm.reasoning : undefined,
        input: Array.isArray(rm.input) ? rm.input.filter((x): x is string => typeof x === "string") : undefined,
        contextWindow: typeof rm.contextWindow === "number" ? rm.contextWindow : undefined,
        maxTokens: typeof rm.maxTokens === "number" ? rm.maxTokens : undefined,
        cost,
      });
    }
    if (typeof rp.id === "string" && typeof rp.name === "string") {
      providers.push({
        id: rp.id,
        name: rp.name,
        api: typeof rp.api === "string" ? rp.api : undefined,
        models,
      });
    }
  }
  return { providers };
}

/** Fallback provider-baseUrl-hostname -> models.dev provider id mapping for
 * `catalogRecommend`. The primary host→provider index is built data-driven
 * from each catalog provider's own `api` base URL (177/204 carry one); this
 * table only covers providers whose catalog entry has NO `api` (openai,
 * anthropic, google, …) and regional host variants models.dev doesn't list.
 * NB: ids must match models.dev's provider keys (e.g. `moonshotai`, NOT
 * `moonshot`). */
const CATALOG_HOST_HINTS: Record<string, string> = {
  "api.openai.com": "openai",
  "api.anthropic.com": "anthropic",
  "generativelanguage.googleapis.com": "google",
  "api.x.ai": "xai",
  "api.groq.com": "groq",
  "api.mistral.ai": "mistral",
  "api.cerebras.ai": "cerebras",
  // regional variants (catalog `api` carries only the .com/.intl host)
  "api.moonshot.cn": "moonshotai",
  "dashscope.aliyuncs.com": "alibaba",
};

/** Find the models.dev catalog entry for a model, given the provider being
 * edited and the model id to fill.
 *
 * Matching order (design 08-27-provider-form-piweb §4.2, relaxed 08-28):
 *   1. resolve the baseUrl host to a catalog provider — data-driven via the
 *      catalog's own `api` URLs, CATALOG_HOST_HINTS as fallback (exact host
 *      first, then subdomain suffix) — and require an exact model-id match;
 *   2. unmapped/missed host (relays, proxies): exact model id across ALL
 *      catalog providers. Unique hit wins; an ambiguous id prefers the
 *      hinted provider, else the first in catalog order (backend sorts by
 *      provider id, so this is deterministic);
 *   3. same disambiguation on the model DISPLAY name (ids sometimes diverge).
 * Returns null when nothing matches (catalog fill shows "not found"). */
export function catalogRecommend(
  catalog: CatalogResponse,
  baseUrl: string,
  modelId: string,
): CatalogModel | null {
  let host = "";
  try {
    host = new URL(baseUrl).hostname.toLowerCase();
  } catch {
    return null;
  }
  const needle = modelId.toLowerCase();

  // 1. host → catalog provider (data-driven index, then static fallback)
  const derived: [string, string][] = [];
  for (const p of catalog.providers) {
    if (!p.api) continue;
    try {
      derived.push([new URL(p.api).hostname.toLowerCase(), p.id]);
    } catch {
      /* malformed api URL upstream — skip that entry */
    }
  }
  const staticHints = Object.entries(CATALOG_HOST_HINTS);
  let hintedProviderId: string | null = null;
  for (const entries of [derived, staticHints]) {
    hintedProviderId =
      entries.find(([h]) => host === h)?.[1] ??
      entries.find(([h]) => host.endsWith(`.${h}`))?.[1] ??
      null;
    if (hintedProviderId) break;
  }
  const byProviderId = (pid: string): CatalogModel | null =>
    catalog.providers
      .find((p) => p.id === pid)
      ?.models.find((m) => m.id.toLowerCase() === needle) ?? null;
  if (hintedProviderId) {
    const hit = byProviderId(hintedProviderId);
    if (hit) return hit;
  }

  // Shared disambiguation for the cross-provider sweeps below.
  const pick = (
    hits: { m: CatalogModel; pid: string }[],
  ): CatalogModel | null => {
    if (hits.length === 0) return null;
    if (hits.length === 1) return hits[0].m;
    if (hintedProviderId) {
      const hinted = hits.find((h) => h.pid === hintedProviderId);
      if (hinted) return hinted.m;
    }
    return hits[0].m; // catalog order = provider-id order (deterministic)
  };

  // 2. exact model id across all providers
  const idHits = catalog.providers.flatMap((p) =>
    p.models
      .filter((m) => m.id.toLowerCase() === needle)
      .map((m) => ({ m, pid: p.id })),
  );
  const byId = pick(idHits);
  if (byId) return byId;

  // 3. model display-name fallback
  const nameHits = catalog.providers.flatMap((p) =>
    p.models
      .filter((m) => (m.name ?? "").toLowerCase() === needle)
      .map((m) => ({ m, pid: p.id })),
  );
  return pick(nameHits);
}

// ── helpers ───────────────────────────────────────────────────────

export const API_PROTOCOLS = [
  "openai-completions",
  "openai-responses",
  "anthropic-messages",
] as const;

export function safeStringify(v: unknown, fallback = "{}"): string {
  try {
    return v == null ? fallback : JSON.stringify(v, null, 2);
  } catch {
    return fallback;
  }
}

export function genProviderId(existing: Record<string, unknown>): string {
  let n = 1;
  while (`provider-${n}` in existing) n++;
  return `provider-${n}`;
}

/** Slug a provider display name into a valid provider id: lowercase ascii
 *  alnum kept, every other run collapsed to `-`, edges trimmed. "" when
 *  nothing survives (e.g. a pure-CJK name) — caller keeps the placeholder. */
export function slugifyProviderName(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

/** While a provider's key is still the auto-generated `provider-N`
 *  placeholder, derive a meaningful id from its display name. pi's TUI
 *  (model-selector `[id]` badge, footer `(id)`) and pi-web (group header)
 *  display the provider ID everywhere — the node `name` never reaches those
 *  UIs — so the id itself must carry the name for it to show up. Returns ""
 *  when the id was user-customized (not a placeholder) or the name has no
 *  slug; `-2`/`-3`… suffixes resolve collisions against `existing`.
 *  (08-28-provider-id-from-name) */
export function deriveProviderIdFromName(
  name: string,
  existing: Record<string, unknown>,
  selfId: string,
): string {
  if (!/^provider-\d+$/.test(selfId)) return "";
  const slug = slugifyProviderName(name);
  if (!slug || slug === selfId) return "";
  let id = slug;
  for (let n = 2; id in existing; n++) id = `${slug}-${n}`;
  return id;
}

/** Re-point every agent reference at a renamed provider id (pi/opencode
 *  single assignments + claude/codex preset references). */
export function rebindAgentProviders(
  agents: AgentsConfig,
  from: string,
  to: string,
): AgentsConfig {
  const next: AgentsConfig = { ...agents };
  if (next.pi?.provider === from) next.pi = { ...next.pi, provider: to };
  if (next.opencode?.provider === from)
    next.opencode = { ...next.opencode, provider: to };
  if (next.claude) {
    next.claude = {
      ...next.claude,
      presets: next.claude.presets.map((p) =>
        p.provider === from ? { ...p, provider: to } : p,
      ),
    };
  }
  if (next.codex) {
    next.codex = {
      ...next.codex,
      presets: next.codex.presets.map((p) =>
        p.provider === from ? { ...p, provider: to } : p,
      ),
    };
  }
  return next;
}

export function emptyProvider(): ProviderEntry {
  return {
    name: "",
    baseUrl: "",
    api: "openai-completions",
    apiKey: undefined,
    headers: {},
    compat: {},
    models: [],
  };
}

/**
 * Why a provider can't be assigned to an agent (design §2 compat matrix).
 * Returns null when compatible. R1: claude requires anthropic-messages
 * (no separate anthropic block escape hatch); codex rejects it.
 */
export function incompatibleReason(
  agent: AgentTab,
  provider: ProviderEntry,
): string | null {
  if (agent === "claude" && provider.api !== "anthropic-messages") {
    return "incompatible-claude";
  }
  if (agent === "codex" && provider.api === "anthropic-messages") {
    return "incompatible-codex";
  }
  return null;
}

/** Format the live readback line for an agent tab. */
export function liveReadbackText(
  agent: AgentTab,
  status: AgentStatus | undefined,
): string {
  const l = status?.live;
  if (!l) return "—";
  switch (agent) {
    case "pi":
      return `${l.provider ?? "—"} / ${l.model ?? "—"}`;
    case "opencode":
      return l.model ?? "—";
    case "claude":
      return `${l.baseUrl ?? "—"} · ${l.model ?? "—"}`;
    case "codex":
      return `${l.modelProvider ?? "—"} / ${l.model ?? "—"}`;
  }
}

/** Live-vs-assignment match state for an incremental agent tab (pi/opencode),
 * mirroring PresetList's tri-state badge semantics: "unknown" when the live
 * default is unreadable, "match" when the native default equals the canonical
 * assignment, "mismatch" otherwise. */
export function liveMatchState(
  agent: "pi" | "opencode",
  live: AgentLive | null,
  assignment: { provider?: string; model?: string } | undefined,
): "match" | "mismatch" | "unknown" {
  if (!live) return "unknown";
  if (agent === "pi") {
    if (!live.provider && !live.model) return "unknown";
    return live.provider === (assignment?.provider ?? null) &&
      live.model === (assignment?.model ?? null)
      ? "match"
      : "mismatch";
  }
  // opencode: the native default is top-level model = "<providerId>/<modelId>".
  if (!live.model) return "unknown";
  const expected =
    assignment?.provider && assignment?.model
      ? `${assignment.provider}/${assignment.model}`
      : null;
  return live.model === expected ? "match" : "mismatch";
}

/** Whether a live provider node is the agent's native default: pi =
 * settings defaultProvider; opencode = top-level model "<id>/…" prefix. */
export function isLiveDefault(
  agent: "pi" | "opencode",
  live: AgentLive | null,
  providerId: string,
): boolean {
  if (!live) return false;
  if (agent === "pi") return live.provider === providerId;
  return !!live.model && live.model.startsWith(`${providerId}/`);
}

/** Which agents bind a given provider id (for card chips + editor overview).
 *  For switch-style agents, a provider is "bound" when ANY preset references it. */
export function bindingAgents(
  config: CanonicalConfig,
  providerId: string,
): AgentTab[] {
  const out: AgentTab[] = [];
  const a = config.agents;
  if (a.pi?.provider === providerId) out.push("pi");
  if (a.opencode?.provider === providerId) out.push("opencode");
  if (a.claude?.presets.some((p) => p.provider === providerId)) out.push("claude");
  if (a.codex?.presets.some((p) => p.provider === providerId)) out.push("codex");
  return out;
}

/** The currently-effective preset for a switch-style agent (null when unset/dangling). */
export function currentPreset(
  config: CanonicalConfig,
  agent: PresetAgent,
): AnyPreset | null {
  const block =
    agent === "claude" ? config.agents.claude : config.agents.codex;
  if (!block || !block.current) return null;
  return block.presets.find((p) => p.id === block.current) ?? null;
}

/** A blank claude preset (id empty - backend backfills on PUT). */
export function emptyClaudePreset(): ClaudePreset {
  return {
    id: "",
    name: "",
    provider: "",
    model: "",
    haikuModel: null,
    sonnetModel: null,
    opusModel: null,
    authField: "AUTH_TOKEN",
  };
}

/** A blank codex preset (id empty - backend backfills on PUT). */
export function emptyCodexPreset(): CodexPreset {
  return {
    id: "",
    name: "",
    provider: "",
    model: "",
    reasoningEffort: null,
    wireApi: "responses",
  };
}

/** Short human label for a provider protocol. */
export function protocolLabel(p: string): string {
  switch (p) {
    case "openai-completions":
      return "openai chat";
    case "openai-responses":
      return "openai resp";
    case "anthropic-messages":
      return "anthropic";
    default:
      return p;
  }
}

/** Whether a string looks like a server-side masked apiKey (has "****"). */
export function isMaskedKey(k: string | undefined): boolean {
  return !!k && k.includes("****");
}

export type { Lang };
