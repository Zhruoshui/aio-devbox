// Shared types + decoders for the models config page — ported from
// web/src/panes/models/types.ts (sandbox-mgr Phase 4c).
//
// Single boundary owner (cross-layer-thinking-guide): every /api/models/* and
// /api/usage response is decoded exactly once here, and all rendering consumes
// the typed CanonicalConfig / CatalogResponse / UsageFanout — never raw JSON.
//
// Mirrors aio-models crate shapes (mgr/src/models.rs contract-locks them to
// app/src/routes/models/*) plus the mgr-only /api/usage fan-out wrapper
// (mgr/src/usage.rs: {sandboxes: [{name, error, usage}]} where `usage` is the
// app's /api/models/usage payload passed through verbatim).
//
// Trimmed vs the workbench copy (mgr has no sandbox-local agent APIs):
// AgentsResponse/AgentStatus/AgentLive, ApplyResponse, live-match helpers.
// The canonical `agents` block (assignments + presets) stays fully editable.

// ── canonical config types (mirror aio-models/store.rs) ───────────

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
  /** id of the active preset; unset or dangling => the sandbox-side apply refuses. */
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

/** pi/opencode keep a single assignment (incremental agents). */
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

// ── discover + test types ──────────────────────────────────────────

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

// ── usage types (app row shape + mgr fan-out wrapper) ──────────────

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

/** One sandbox's app response, decoded (usage null when the probe errored). */
export interface SandboxUsageEntry {
  name: string;
  error: string | null;
  usage: UsageResponse | null;
}

/** One day's usage for one (agent, model) — the S4 byDay series item.
 * `date` is `YYYY-MM-DD` (UTC); `cost` present only when the source logged
 * it (pi/opencode); claude/codex days carry no cost (AC4 hides the series). */
export interface DayUsage {
  date: string;
  agent: string;
  model: string;
  in: number;
  out: number;
  cacheRead: number;
  cacheWrite: number;
  cost?: number;
}

/** Per-sandbox total derived by mgr from that sandbox's rows (S4 design §2). */
export interface SandboxTotal {
  name: string;
  in: number;
  out: number;
  cost: number;
}

/** GET /api/usage?window= — the mgr-only multi-sandbox wrapper. */
export interface UsageFanout {
  sandboxes: SandboxUsageEntry[];
  /** S4: per-sandbox totals ({name, in, out, cost}) — absent on old mgr. */
  totals?: SandboxTotal[];
}

export interface UsageResponse {
  rows: UsageRow[];
  /** S4: last-14-days series, independent of the window param. Absent on
   * old apps — the frontend hides the trend when it's missing. */
  byDay?: DayUsage[];
  generatedAt: string;
}

/** The four agent tabs that carry a provider/model assignment. */
export type AgentTab = "pi" | "opencode" | "claude" | "codex";

// ── models.dev catalog types ───────────────────────────────────────

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

// ── decoders (single boundary owner) ───────────────────────────────

function isObj(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null;
}

function asU64(v: unknown): number {
  return typeof v === "number" && v >= 0 ? Math.floor(v) : 0;
}

/** Decode one app /api/models/usage payload (rows + generatedAt). */
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
  // S4: decode byDay (absent on old apps -> undefined, trend hidden).
  const rawDays = Array.isArray(o.byDay) ? o.byDay : null;
  const byDay: DayUsage[] | undefined = rawDays
    ? rawDays
        .map((d): DayUsage | null => {
          if (!isObj(d) || typeof d.date !== "string") return null;
          return {
            date: d.date,
            agent: typeof d.agent === "string" ? d.agent : "",
            model: typeof d.model === "string" ? d.model : "",
            in: asU64(d.in),
            out: asU64(d.out),
            cacheRead: asU64(d.cacheRead),
            cacheWrite: asU64(d.cacheWrite),
            cost: typeof d.cost === "number" ? d.cost : undefined,
          };
        })
        .filter((d): d is DayUsage => d !== null)
    : undefined;
  return {
    rows,
    byDay,
    generatedAt: typeof o.generatedAt === "string" ? o.generatedAt : "",
  };
}

/** Decode GET /api/usage — the mgr fan-out: one entry per running sandbox,
 * `usage` passed through verbatim from that sandbox's app. Malformed entries
 * are skipped (never fail the whole page for one bad sandbox). */
export function decodeUsageFanout(json: unknown): UsageFanout {
  const o = isObj(json) ? json : {};
  const raw = Array.isArray(o.sandboxes) ? o.sandboxes : [];
  const sandboxes: SandboxUsageEntry[] = [];
  for (const e of raw) {
    if (!isObj(e) || typeof e.name !== "string") continue;
    sandboxes.push({
      name: e.name,
      error: typeof e.error === "string" ? e.error : null,
      usage: e.usage == null ? null : decodeUsage(e.usage),
    });
  }
  // S4: decode totals (absent on old mgr -> totals falls back undefined and
  // the frontend derives per-sandbox bars from entries).
  const rawTotals = Array.isArray(o.totals) ? o.totals : null;
  const totals: SandboxTotal[] | undefined = rawTotals
    ? rawTotals
        .map((e): SandboxTotal | null => {
          if (!isObj(e) || typeof e.name !== "string") return null;
          return {
            name: e.name,
            in: asU64(e.in),
            out: asU64(e.out),
            cost: typeof e.cost === "number" ? e.cost : 0,
          };
        })
        .filter((e): e is SandboxTotal => e !== null)
    : undefined;
  return { sandboxes, totals };
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
 * from each catalog provider's own `api` base URL; this table only covers
 * providers whose catalog entry has NO `api` (openai, anthropic, google, …)
 * and regional host variants models.dev doesn't list.
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
 *  edited and the model id to fill (see the workbench copy for the full
 *  matching-order rationale — identical here). */
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

// ── helpers ────────────────────────────────────────────────────────

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
 *  placeholder, derive a meaningful id from its display name (pi's TUI and
 *  pi-web display the provider ID everywhere — the node `name` never reaches
 *  those UIs). Returns "" when the id was user-customized (not a placeholder)
 *  or the name has no slug; `-2`/`-3`… suffixes resolve collisions.
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
 * single assignments + claude/codex preset references). */
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
 * Why a provider can't be assigned to an agent (compat matrix). Returns null
 * when compatible. claude requires anthropic-messages; codex rejects it.
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

/** Badge label for a provider protocol (prototype models.html pv card):
 * the short protocol family, language-neutral — no i18n needed. */
export function protocolBadge(p: string): string {
  switch (p) {
    case "anthropic-messages":
      return "Anthropic";
    case "openai-responses":
      return "Responses";
    case "openai-completions":
      return "OpenAI compat";
    default:
      return p;
  }
}

/** Whether a string looks like a server-side masked apiKey (has "****"). */
export function isMaskedKey(k: string | undefined): boolean {
  return !!k && k.includes("****");
}

// ── usage formatting helpers (shared by the usage page) ────────────

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
 *  codex logs `cached_input_tokens` as a SUBSET of `input_tokens` (denominator
 *  `in` alone); the other agents log DISJOINT buckets (denominator
 *  `in + cacheRead + cacheWrite`) — see the workbench copy for provenance. */
export function cacheHitDenom(r: UsageRow): number {
  return r.agent === "codex" ? r.in : r.in + r.cacheRead + r.cacheWrite;
}

/** One row's cache hit rate in 0..1, or null when nothing hit the cache. */
export function cacheHitRate(r: UsageRow): number | null {
  const denom = cacheHitDenom(r);
  return denom > 0 && r.cacheRead > 0 ? r.cacheRead / denom : null;
}
