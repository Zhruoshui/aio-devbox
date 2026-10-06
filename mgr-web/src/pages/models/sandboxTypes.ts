// Sandbox-side agent-config types + decoders (task 10-04-model-config-cc-switch
// Phase 4). These mirror the SANDBOX APP's shapes — aio-models/src/snapshot.rs
// (ProviderSnapshot / SwitchAgentConfig), aio-models/src/incremental.rs
// (IncrementalAgentConfig / ProviderNode), and the app's /api/models/agents +
// /api/models/apply/:agent responses (app/src/routes/models/mod.rs).
//
// Single boundary owner (cross-layer-thinking-guide): every response that
// crosses the mgr /api/sbx/:name/api/models/* proxy is decoded exactly once
// here; the sandbox-config panels consume these typed shapes, never raw JSON.
// The wire keys come from serde: SwitchAgentConfig/IncrementalAgentConfig are
// #[serde(rename_all = "camelCase")], ProviderSnapshot is snake_case
// (settings_config / created_at), ProviderNode is camelCase.

import { safeStringify } from "./types";

// ── switch-style agents (claude / codex) ───────────────────────────

export interface ProviderSnapshot {
  id: string;
  name: string;
  /** cc-switch style opaque config — for claude the full settings.json
   *  object; for codex `{ config: "<config.toml string>", auth: {...} }`. */
  settingsConfig: Record<string, unknown>;
  createdAt?: number;
  notes?: string;
}

export interface SwitchAgentConfig {
  providers: ProviderSnapshot[];
  current?: string;
}

// ── incremental-style agents (pi / opencode) ───────────────────────

export interface IncrementalModelEntry {
  id: string;
  name?: string;
  reasoning?: boolean;
  contextWindow?: number;
  maxTokens?: number;
}

export interface ProviderNode {
  name: string;
  baseUrl: string;
  api: string;
  apiKey?: string;
  headers: Record<string, string>;
  compat?: unknown;
  models: IncrementalModelEntry[];
}

export interface IncrementalAgentConfig {
  providers: Record<string, ProviderNode>;
  defaultProvider?: string;
  defaultModel?: string;
}

// ── the sandbox's full config (GET/PUT /api/sbx/:name/api/models/config) ──

export interface SandboxAgentConfig {
  version?: number;
  claude?: SwitchAgentConfig;
  codex?: SwitchAgentConfig;
  pi?: IncrementalAgentConfig;
  opencode?: IncrementalAgentConfig;
}

// ── agent status (GET /api/sbx/:name/api/models/agents) ────────────

export interface AgentStatus {
  installed: boolean;
  bin?: string | null;
  live?: unknown;
}

export interface AgentsResponse {
  pi: AgentStatus;
  opencode: AgentStatus;
  claude: AgentStatus;
  codex: AgentStatus;
}

// ── apply result (POST /api/sbx/:name/api/models/apply/:agent) ─────

export interface ApplyFileResult {
  path: string;
  backup?: string | null;
}

/** POST /api/models/apply/:agent replies `{ok, written, errors}` — the app's
 *  ApplyResult (app/src/routes/models/render/common.rs). */
export interface ApplyResult {
  ok: boolean;
  written: ApplyFileResult[];
  errors: { path: string; error: string }[];
}

// ── decoders ───────────────────────────────────────────────────────

function isObj(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null;
}

function asIncrementalModel(v: unknown): IncrementalModelEntry | null {
  if (!isObj(v) || typeof v.id !== "string") return null;
  return {
    id: v.id,
    name: typeof v.name === "string" ? v.name : undefined,
    reasoning: typeof v.reasoning === "boolean" ? v.reasoning : undefined,
    contextWindow: typeof v.contextWindow === "number" ? v.contextWindow : undefined,
    maxTokens: typeof v.maxTokens === "number" ? v.maxTokens : undefined,
  };
}

function asProviderNode(v: unknown): ProviderNode | null {
  if (!isObj(v) || typeof v.baseUrl !== "string" || typeof v.api !== "string") {
    return null;
  }
  const headers: Record<string, string> = {};
  if (isObj(v.headers)) {
    for (const [k, val] of Object.entries(v.headers)) {
      if (typeof val === "string") headers[k] = val;
    }
  }
  const models: IncrementalModelEntry[] = [];
  if (Array.isArray(v.models)) {
    for (const m of v.models) {
      const parsed = asIncrementalModel(m);
      if (parsed) models.push(parsed);
    }
  }
  return {
    name: typeof v.name === "string" ? v.name : "",
    baseUrl: v.baseUrl,
    api: v.api,
    apiKey: typeof v.apiKey === "string" ? v.apiKey : undefined,
    headers,
    compat: v.compat,
    models,
  };
}

function asSnapshot(v: unknown): ProviderSnapshot | null {
  if (!isObj(v) || typeof v.id !== "string" || typeof v.name !== "string") return null;
  const settings = isObj(v.settings_config) ? v.settings_config : {};
  return {
    id: v.id,
    name: v.name,
    settingsConfig: settings,
    createdAt: typeof v.created_at === "number" ? v.created_at : undefined,
    notes: typeof v.notes === "string" ? v.notes : undefined,
  };
}

function asSwitch(v: unknown): SwitchAgentConfig | undefined {
  if (!isObj(v)) return undefined;
  const providers: ProviderSnapshot[] = [];
  if (Array.isArray(v.providers)) {
    for (const s of v.providers) {
      const parsed = asSnapshot(s);
      if (parsed) providers.push(parsed);
    }
  }
  return {
    providers,
    current: typeof v.current === "string" ? v.current : undefined,
  };
}

function asIncremental(v: unknown): IncrementalAgentConfig | undefined {
  if (!isObj(v)) return undefined;
  const providers: Record<string, ProviderNode> = {};
  if (isObj(v.providers)) {
    for (const [id, node] of Object.entries(v.providers)) {
      const parsed = asProviderNode(node);
      if (parsed) providers[id] = parsed;
    }
  }
  return {
    providers,
    defaultProvider: typeof v.defaultProvider === "string" ? v.defaultProvider : undefined,
    defaultModel: typeof v.defaultModel === "string" ? v.defaultModel : undefined,
  };
}

/** Decode GET /api/models/config (or the PUT echo) — the sandbox's full
 *  agent config. Absent agent blocks stay undefined (agent not configured). */
export function decodeSandboxAgentConfig(json: unknown): SandboxAgentConfig {
  const o = isObj(json) ? json : {};
  return {
    version: typeof o.version === "number" ? o.version : undefined,
    claude: asSwitch(o.claude),
    codex: asSwitch(o.codex),
    pi: asIncremental(o.pi),
    opencode: asIncremental(o.opencode),
  };
}

function asAgentStatus(v: unknown): AgentStatus {
  const o = isObj(v) ? v : {};
  return {
    installed: typeof o.installed === "boolean" ? o.installed : false,
    bin: typeof o.bin === "string" ? o.bin : null,
    live: o.live,
  };
}

/** Decode GET /api/models/agents — per-agent install + live readback. */
export function decodeAgents(json: unknown): AgentsResponse {
  const o = isObj(json) ? json : {};
  return {
    pi: asAgentStatus(o.pi),
    opencode: asAgentStatus(o.opencode),
    claude: asAgentStatus(o.claude),
    codex: asAgentStatus(o.codex),
  };
}

function asApplyFile(v: unknown): ApplyFileResult | null {
  if (!isObj(v) || typeof v.path !== "string") return null;
  return {
    path: v.path,
    backup: typeof v.backup === "string" ? v.backup : null,
  };
}

/** Decode POST /api/models/apply/:agent — files written (with backups) +
 *  per-file errors. A totally foreign body decodes to a failed apply. */
export function decodeApplyResult(json: unknown): ApplyResult {
  const o = isObj(json) ? json : {};
  const written: ApplyFileResult[] = [];
  if (Array.isArray(o.written)) {
    for (const w of o.written) {
      const parsed = asApplyFile(w);
      if (parsed) written.push(parsed);
    }
  }
  const errors: { path: string; error: string }[] = [];
  if (Array.isArray(o.errors)) {
    for (const e of o.errors) {
      if (!isObj(e)) continue;
      errors.push({
        path: typeof e.path === "string" ? e.path : "",
        error: typeof e.message === "string" ? e.message : String(e.message ?? ""),
      });
    }
  }
  return {
    ok: typeof o.ok === "boolean" ? o.ok : errors.length === 0,
    written,
    errors,
  };
}

// ── empty factories (editor "new" flows) ───────────────────────────

/** An empty claude snapshot: the settings.json skeleton with the standard
 *  env hook that points claude at the provider's base URL + key. */
export function emptyClaudeSnapshot(id: string): ProviderSnapshot {
  return {
    id,
    name: "",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "",
      },
    },
  };
}

/** An empty codex snapshot: `{ config: "<toml>", auth: {} }`. */
export function emptyCodexSnapshot(id: string): ProviderSnapshot {
  return {
    id,
    name: "",
    settingsConfig: {
      config: 'model = ""\nmodel_provider = ""\n',
      auth: {},
    },
  };
}

/** An empty pi/opencode provider node. */
export function emptyProviderNode(): ProviderNode {
  return { name: "", baseUrl: "", api: "openai-completions", headers: {}, models: [] };
}

// ── helpers ────────────────────────────────────────────────────────

/** The four agents in their UI order. */
export const AGENT_IDS = ["claude", "codex", "pi", "opencode"] as const;
export type AgentId = (typeof AGENT_IDS)[number];

export function isSwitchAgent(a: AgentId): boolean {
  return a === "claude" || a === "codex";
}

/** Pick one agent block out of the full config. */
export function pickAgent(
  config: SandboxAgentConfig,
  agent: AgentId,
): SwitchAgentConfig | IncrementalAgentConfig | undefined {
  switch (agent) {
    case "claude":
      return config.claude;
    case "codex":
      return config.codex;
    case "pi":
      return config.pi;
    case "opencode":
      return config.opencode;
  }
}

/** Replace one agent block, returning a NEW config object (immutability). */
export function setAgent(
  config: SandboxAgentConfig,
  agent: AgentId,
  value: SwitchAgentConfig | IncrementalAgentConfig | undefined,
): SandboxAgentConfig {
  const next: SandboxAgentConfig = { ...config };
  if (value === undefined) {
    // The two block kinds are disjoint in practice — deleting the right key
    // is safe because the agent id picks which one could ever live there.
    delete (next as Record<string, unknown>)[agent];
  } else {
    (next as Record<string, unknown>)[agent] = value;
  }
  return next;
}

/** Pretty-print the snapshot's settings_config for the source-code view.
 *  For codex the config.toml string stays raw (the `config` key), the rest is
 *  JSON-pretty — render.rs stores `{config: "<toml>", auth: {...}}`. */
export function snapshotToSource(snapshot: ProviderSnapshot, agent: AgentId): string {
  if (agent === "codex") {
    const cfg = snapshot.settingsConfig.config;
    return typeof cfg === "string" ? cfg : "";
  }
  return safeStringify(snapshot.settingsConfig, "{}");
}

/** Parse a source string back into a settings_config object (best effort —
 *  codex: `{config: text, auth: keep}`; others: JSON.parse). Returns null on
 *  unparseable input. `prev` is the snapshot being edited (auth preserved). */
export function sourceToSnapshotSettings(
  text: string,
  agent: AgentId,
  prev: ProviderSnapshot,
): Record<string, unknown> | null {
  if (agent === "codex") {
    if (text.trim() === "") return null;
    const prevAuth = isObj(prev.settingsConfig.auth) ? prev.settingsConfig.auth : {};
    return { config: text, auth: prevAuth };
  }
  try {
    const v: unknown = JSON.parse(text);
    return isObj(v) ? v : null;
  } catch {
    return null;
  }
}
