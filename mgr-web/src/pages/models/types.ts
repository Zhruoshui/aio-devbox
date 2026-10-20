// Shared types + decoders for the mgr models config page (cc-switch refactor,
// task 10-04-model-config-cc-switch Phase 3).
//
// Single boundary owner (cross-layer-thinking-guide): every /api/models/* and
// /api/usage response is decoded exactly once here, and all rendering consumes
// the typed ProviderLibrary / UsageFanout — never raw JSON.
//
// Mirrors aio-models crate shapes:
//   - aio-models/src/template.rs — ProviderTemplate / TemplateModelEntry /
//     TemplateCost / ProviderLibrary (the mgr-side provider library)
//   - The mgr routes (mgr/src/models.rs) contract-lock the wire shape.
//
// Removed in the cc-switch refactor (prd "废弃的功能"):
//   - CanonicalConfig / AgentsConfig / ClaudePreset / CodexPreset (profile
//     system + agent assignments — each sandbox owns its agent config now)
//   - CatalogResponse / catalogRecommend (models.dev proxy — deleted)
//   - ImportResponse (pi import — deleted)
//   - rebindAgentProviders / bindingAgents / currentPreset / incompatibleReason
//     (agent-binding helpers — no more agent bindings at the mgr layer)
//   - AgentTab / PresetAgent / AnyPreset (agent-tab unions — deleted)
//
// What stays:
//   - ProviderLibrary / ProviderTemplate / TemplateModelEntry / TemplateCost
//   - Discover / Test request-response types (probes are still mgr-side)
//   - Usage types (UsageRow / UsageFanout / DayUsage / SandboxTotal) + the
//     usage fan-out decoder — the Usage page is unchanged by this refactor
//   - Small helpers used by the provider editor (genProviderId, slugify,
//     protocolLabel, isMaskedKey, safeStringify, API_PROTOCOLS)
//   - Usage formatting helpers (fmtTokens / fmtCost / fmtPct / cacheHit*)

// ── provider library types (mirror aio-models/template.rs) ────────

export interface TemplateCost {
  input?: number;
  output?: number;
  cacheRead?: number;
  cacheWrite?: number;
}

export interface TemplateModelEntry {
  id: string;
  name?: string;
  reasoning?: boolean;
  contextWindow?: number;
  maxTokens?: number;
  cost?: TemplateCost;
}


export interface ProviderTemplate {
  name: string;
  baseUrl: string;
  api: string;
  apiKey?: string;
  headers: Record<string, string>;
  models: TemplateModelEntry[];
  websiteUrl?: string;
  category?: string;
  notes?: string;
}

export interface ProviderLibrary {
  version: number;
  providers: Record<string, ProviderTemplate>;
}

/** One row of GET /api/models/templates — the template list for
 * sandbox-side pickers. Carries the REAL apiKey (10-13-models-config-ux R2)
 * so picking a template prefills the key too; panels fill only EMPTY key
 * fields, never overwriting a user-typed value. */
export interface ProviderTemplatePublic {
  id: string;
  name: string;
  baseUrl: string;
  api: string;
  /** Real stored key (same trust domain as the provider editor); absent
   *  when the library entry has no key configured. */
  apiKey?: string;
  models: TemplateModelEntry[];
  websiteUrl?: string;
  category?: string;
  notes?: string;
}

export interface TemplatesResponse {
  version: number;
  templates: ProviderTemplatePublic[];
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

// ── decoders (single boundary owner) ───────────────────────────────

function isObj(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null;
}

function asU64(v: unknown): number {
  return typeof v === "number" && v >= 0 ? Math.floor(v) : 0;
}

function asCost(v: unknown): TemplateCost | undefined {
  if (!isObj(v)) return undefined;
  return {
    input: typeof v.input === "number" ? v.input : undefined,
    output: typeof v.output === "number" ? v.output : undefined,
    cacheRead: typeof v.cacheRead === "number" ? v.cacheRead : undefined,
    cacheWrite: typeof v.cacheWrite === "number" ? v.cacheWrite : undefined,
  };
}

function asTemplateModel(v: unknown): TemplateModelEntry | null {
  if (!isObj(v) || typeof v.id !== "string") return null;
  return {
    id: v.id,
    name: typeof v.name === "string" ? v.name : undefined,
    reasoning: typeof v.reasoning === "boolean" ? v.reasoning : undefined,
    contextWindow: typeof v.contextWindow === "number" ? v.contextWindow : undefined,
    maxTokens: typeof v.maxTokens === "number" ? v.maxTokens : undefined,
    cost: asCost(v.cost),
  };
}

function asTemplate(v: unknown): ProviderTemplate | null {
  if (!isObj(v)) return null;
  if (typeof v.name !== "string") return null;
  if (typeof v.baseUrl !== "string") return null;
  if (typeof v.api !== "string") return null;
  const headers: Record<string, string> = {};
  if (isObj(v.headers)) {
    for (const [k, val] of Object.entries(v.headers)) {
      if (typeof val === "string") headers[k] = val;
    }
  }
  const models: TemplateModelEntry[] = [];
  if (Array.isArray(v.models)) {
    for (const m of v.models) {
      const parsed = asTemplateModel(m);
      if (parsed) models.push(parsed);
    }
  }
  return {
    name: v.name,
    baseUrl: v.baseUrl,
    api: v.api,
    apiKey: typeof v.apiKey === "string" ? v.apiKey : undefined,
    headers,
    models,
    websiteUrl: typeof v.websiteUrl === "string" ? v.websiteUrl : undefined,
    category: typeof v.category === "string" ? v.category : undefined,
    notes: typeof v.notes === "string" ? v.notes : undefined,
  };
}

/** Decode GET /api/models/providers — the masked provider library. */
export function decodeLibrary(json: unknown): ProviderLibrary {
  const o = isObj(json) ? json : {};
  const providers: Record<string, ProviderTemplate> = {};
  if (isObj(o.providers)) {
    for (const [k, v] of Object.entries(o.providers)) {
      const parsed = asTemplate(v);
      if (parsed) providers[k] = parsed;
    }
  }
  return {
    version: typeof o.version === "number" ? o.version : 1,
    providers,
  };
}

/** Decode GET /api/models/templates — the template list (real apiKey). */
export function decodeTemplates(json: unknown): TemplatesResponse {
  const o = isObj(json) ? json : {};
  const templates: ProviderTemplatePublic[] = [];
  if (Array.isArray(o.templates)) {
    for (const t of o.templates) {
      if (!isObj(t) || typeof t.id !== "string") continue;
      const parsed = asTemplate(t);
      if (!parsed) continue;
      templates.push({
        id: t.id,
        name: parsed.name,
        baseUrl: parsed.baseUrl,
        api: parsed.api,
        apiKey: typeof t.apiKey === "string" && t.apiKey !== "" ? t.apiKey : undefined,
        models: parsed.models,
        websiteUrl: parsed.websiteUrl,
        category: parsed.category,
        notes: parsed.notes,
      });
    }
  }
  return {
    version: typeof o.version === "number" ? o.version : 1,
    templates,
  };
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
 *  placeholder, derive a meaningful id from its display name. Returns "" when
 *  the id was user-customized (not a placeholder) or the name has no slug;
 *  `-2`/`-3`… suffixes resolve collisions. */
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

/** A blank provider template for the editor's "new" flow. */
export function emptyProvider(): ProviderTemplate {
  return {
    name: "",
    baseUrl: "",
    api: "openai-completions",
    apiKey: undefined,
    headers: {},
    models: [],
  };
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
  // Adaptive precision: agent-level daily costs are routinely fractions of a
  // cent ($0.0002/day is real), so a fixed 2-decimal format renders them all
  // as "$0.00". >= $1 shows cents; >= $0.0001 keeps 4 decimals; smaller
  // values extend to 6 so a real cost never displays as all-zeros.
  if (n >= 1) return `$${n.toFixed(2)}`;
  if (n >= 0.0001) return `$${n.toFixed(4)}`;
  return `$${n.toFixed(6)}`;
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
