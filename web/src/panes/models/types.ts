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

export interface AgentsConfig {
  pi?: { provider: string; model: string };
  opencode?: { provider: string; model: string };
  claude?: {
    provider: string;
    model: string;
    haikuModel?: string | null;
    sonnetModel?: string | null;
    opusModel?: string | null;
    authField: string;
  };
  codex?: {
    provider: string;
    model: string;
    reasoningEffort?: string | null;
    wireApi: string;
  };
}

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

/** Live readback for one agent (shape varies by agent; null fields absent). */
export interface AgentLive {
  provider?: string | null;
  model?: string | null;
  baseUrl?: string | null;
  modelProvider?: string | null;
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
  const dec = (v: unknown): AgentStatus => {
    if (!isObj(v)) return empty;
    return {
      installed: v.installed === true,
      bin: typeof v.bin === "string" ? v.bin : null,
      live: isObj(v.live) ? (v.live as AgentLive) : null,
    };
  };
  return {
    pi: dec(json.pi),
    opencode: dec(json.opencode),
    claude: dec(json.claude),
    codex: dec(json.codex),
  };
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

/** Which agents bind a given provider id (for card chips + editor overview). */
export function bindingAgents(
  config: CanonicalConfig,
  providerId: string,
): AgentTab[] {
  const out: AgentTab[] = [];
  const a = config.agents;
  if (a.pi?.provider === providerId) out.push("pi");
  if (a.opencode?.provider === providerId) out.push("opencode");
  if (a.claude?.provider === providerId) out.push("claude");
  if (a.codex?.provider === providerId) out.push("codex");
  return out;
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
