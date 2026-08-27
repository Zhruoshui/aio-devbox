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
      providers.push({ id: rp.id, name: rp.name, models });
    }
  }
  return { providers };
}

/** Common provider-baseUrl-hostname -> models.dev provider id mapping, used by
 * `catalogRecommend` to find the right catalog provider without the backend
 * having to guess (design 08-27-provider-form-piweb §4.2). Extend as needed;
 * an unmapped host just means models.dev fill-in falls back to unavailable. */
const CATALOG_HOST_HINTS: Record<string, string> = {
  "api.openai.com": "openai",
  "api.anthropic.com": "anthropic",
  "generativelanguage.googleapis.com": "google",
  "api.deepseek.com": "deepseek",
  "api.groq.com": "groq",
  "api.mistral.ai": "mistral",
  "api.moonshot.cn": "moonshot",
  "api.moonshot.ai": "moonshot",
  "openrouter.ai": "openrouter",
  "api.x.ai": "xai",
  "dashscope.aliyuncs.com": "alibaba",
  "open.bigmodel.cn": "zhipuai",
};

/** Find the models.dev catalog entry for a model, given the provider being
 * edited (its baseUrl decides which catalog provider to look in) and the
 * model id to fill (case-insensitive exact match — design §4.2). Returns
 * null when the host isn't recognized or the model isn't in that catalog
 * provider's list. */
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
  const catalogProviderId = Object.entries(CATALOG_HOST_HINTS).find(([h]) =>
    host === h || host.endsWith(`.${h}`),
  )?.[1];
  if (!catalogProviderId) return null;
  const provider = catalog.providers.find((p) => p.id === catalogProviderId);
  if (!provider) return null;
  const needle = modelId.toLowerCase();
  return provider.models.find((m) => m.id.toLowerCase() === needle) ?? null;
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
