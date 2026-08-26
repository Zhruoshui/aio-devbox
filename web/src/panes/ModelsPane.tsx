// ModelsPane - native page pane (type="page") for the unified model config.
//
// M0/M1: provider library (CRUD + save + pi import). M2: discover + test.
// M3: agent tabs (pi/opencode/claude/codex) with assignment + apply. The
// usage tab is a placeholder for M4.
//
// API contract: GET/PUT /api/models/config + POST /api/models/import/pi +
// GET /api/models/agents + POST /api/models/apply/:agent.
// The response is decoded once here (decodeConfig / decodeAgents) — all
// rendering uses the typed CanonicalConfig, never raw JSON
// (cross-layer-thinking-guide).

import { useCallback, useEffect, useState } from "react";
import type { ServiceEntry } from "../types";
import { t, type Lang } from "../i18n";
import { Icon } from "../icons";

// ── API types (mirror app/src/routes/models/store.rs) ──────────────

interface CanonicalConfig {
  version: number;
  providers: Record<string, ProviderEntry>;
  agents: AgentsConfig;
}

interface ProviderEntry {
  name: string;
  baseUrl: string;
  api: string;
  apiKey?: string;
  headers: Record<string, string>;
  compat: unknown;
  anthropic?: AnthropicBlock | null;
  models: ModelEntry[];
}

interface AnthropicBlock {
  baseUrl?: string;
}

interface ModelEntry {
  id: string;
  name?: string;
  reasoning?: boolean;
  contextWindow?: number;
  maxTokens?: number;
  cost?: CostEntry;
}

interface CostEntry {
  input?: number;
  output?: number;
  cacheRead?: number;
  cacheWrite?: number;
}

interface AgentsConfig {
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

interface PutResponse {
  ok: boolean;
  warnings?: string[];
}

interface ImportResponse {
  ok: boolean;
  imported: string[];
  skipped: string[];
}

// ── M2: discover + test API types ──────────────────────────────────

interface DiscoveredModel {
  id: string;
  name?: string;
}

interface DiscoverResponse {
  models: DiscoveredModel[];
  endpoint: string;
}

interface TestResponse {
  ok: boolean;
  latencyMs?: number;
  status?: number;
  error?: string;
  responseText?: string;
}

// Per-(provider,model) test pill state. Keyed by `${providerId}:${modelId}`.
interface TestStateEntry {
  status: "idle" | "testing" | "ok" | "fail";
  latencyMs?: number;
  statusHttp?: number;
  error?: string;
  responseText?: string;
}
type TestStateMap = Record<string, TestStateEntry>;

// Discover modal state.
interface DiscoverState {
  loading: boolean;
  error: string;
  endpoint: string;
  models: DiscoveredModel[];
  filter: string;
  selected: Set<string>;
}

// ── M3: agent status + apply API types ────────────────────────────

/** Live readback for one agent (shape varies by agent; null fields absent). */
interface AgentLive {
  provider?: string | null;
  model?: string | null;
  baseUrl?: string | null;
  modelProvider?: string | null;
}

interface AgentStatus {
  installed: boolean;
  bin: string | null;
  live: AgentLive | null;
}

/** Mirrors the Rust `AgentsResponse` in routes/models/mod.rs. */
interface AgentsResponse {
  pi: AgentStatus;
  opencode: AgentStatus;
  claude: AgentStatus;
  codex: AgentStatus;
}

interface ApplyWrittenFile {
  path: string;
  backup: string | null;
}

interface ApplyErrorFile {
  path: string;
  message: string;
}

/** Mirrors the Rust `ApplyResult` in routes/models/render/common.rs. */
interface ApplyResponse {
  ok: boolean;
  written: ApplyWrittenFile[];
  errors: ApplyErrorFile[];
}

// ── decoder (single boundary owner) ───────────────────────────────

function isObj(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null;
}

function asU64(v: unknown): number {
  return typeof v === "number" && v >= 0 ? Math.floor(v) : 0;
}

// ── M4: usage API types ───────────────────────────────────────────

interface UsageRow {
  agent: string;
  provider?: string | null;
  model: string;
  in: number;
  out: number;
  cacheRead: number;
  cacheWrite: number;
  cost?: number;
}

interface UsageResponse {
  rows: UsageRow[];
  generatedAt: string;
}

/** Decode the GET /api/models/usage response (single boundary owner). */
function decodeUsage(json: unknown): UsageResponse {
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
function fmtTokens(n: number): string {
  if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(1)}G`;
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${Math.floor(n / 1000)}k`;
  return String(n);
}

/** Format a USD cost: 4 decimals, plain otherwise. */
function fmtCost(n: number): string {
  return `$${n.toFixed(4)}`;
}

function decodeConfig(json: unknown): CanonicalConfig {
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
function decodeAgents(json: unknown): AgentsResponse {
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

const API_PROTOCOLS = [
  "openai-completions",
  "openai-responses",
  "anthropic-messages",
];

function safeStringify(v: unknown, fallback = "{}"): string {
  try {
    return v == null ? fallback : JSON.stringify(v, null, 2);
  } catch {
    return fallback;
  }
}

function genProviderId(existing: Record<string, unknown>): string {
  let n = 1;
  while (`provider-${n}` in existing) n++;
  return `provider-${n}`;
}

function emptyProvider(): ProviderEntry {
  return {
    name: "",
    baseUrl: "",
    api: "openai-completions",
    apiKey: undefined,
    headers: {},
    compat: {},
    anthropic: null,
    models: [],
  };
}

/** The four agent tabs that carry a provider/model assignment. */
type AgentTab = "pi" | "opencode" | "claude" | "codex";

/**
 * Why a provider can't be assigned to an agent (design §2 compat matrix).
 * Returns null when compatible. claude requires anthropic-messages OR an
 * anthropic block; codex rejects anthropic-messages.
 */
function incompatibleReason(
  agent: AgentTab,
  provider: ProviderEntry,
): string | null {
  if (agent === "claude") {
    if (provider.api !== "anthropic-messages" && provider.anthropic == null) {
      return "incompatible-claude";
    }
  } else if (agent === "codex") {
    if (provider.api === "anthropic-messages") {
      return "incompatible-codex";
    }
  }
  return null;
}

/** Format the live readback line for an agent tab. */
function liveReadbackText(
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

// ── component ─────────────────────────────────────────────────────

type TabKey = "providers" | "pi" | "opencode" | "claude" | "codex" | "usage";

const TAB_KEYS: TabKey[] = [
  "providers",
  "pi",
  "opencode",
  "claude",
  "codex",
  "usage",
];

function tabLabel(lang: Lang, key: TabKey): string {
  switch (key) {
    case "providers":
      return t(lang, "mcProviders");
    case "usage":
      return t(lang, "mcUsage");
    case "pi":
      return "pi";
    case "opencode":
      return "opencode";
    case "claude":
      return "Claude";
    case "codex":
      return "Codex";
  }
}

export function ModelsPane(_: { service: ServiceEntry }): JSX.Element {
  const [lang] = useState<Lang>(
    () => (localStorage.getItem("aio.lang") === "en" ? "en" : "zh-CN"),
  );
  const [tab, setTab] = useState<TabKey>("providers");
  const [config, setConfig] = useState<CanonicalConfig | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saveMsg, setSaveMsg] = useState<{ ok: boolean; text: string } | null>(
    null,
  );
  const [headersText, setHeadersText] = useState("");
  const [compatText, setCompatText] = useState("");
  const [showAdvanced, setShowAdvanced] = useState(false);

  // M2: per-(provider,model) test state + discover modal state.
  // Test state is keyed by `${providerId}:${modelId}` so rows keep their
  // pill across re-renders; a row's pill resets when its provider fields or
  // model id change (tracked via the `version` counter).
  const [testState, setTestState] = useState<TestStateMap>({});
  // Discover modal state.
  const [discover, setDiscover] = useState<DiscoverState | null>(null);

  // M3: agent tabs state.
  const [agentsStatus, setAgentsStatus] = useState<AgentsResponse | null>(null);
  const [agentDirty, setAgentDirty] = useState<Set<string>>(new Set());
  const [applying, setApplying] = useState(false);
  const [applyResult, setApplyResult] = useState<ApplyResponse | null>(null);
  const [agentSaveMsg, setAgentSaveMsg] = useState<{
    ok: boolean;
    text: string;
  } | null>(null);

  // M4: usage tab state. window=today default; refresh bypasses the
  // 30s server cache with ?refresh=1.
  const [usageRows, setUsageRows] = useState<UsageRow[] | null>(null);
  const [usageGeneratedAt, setUsageGeneratedAt] = useState("");
  const [usageWindow, setUsageWindow] = useState<"today" | "7d" | "all">(
    "today",
  );
  const [usageLoading, setUsageLoading] = useState(false);
  const [usageError, setUsageError] = useState("");

  const fetchConfig = useCallback(async (): Promise<void> => {
    try {
      const r = await fetch("/api/models/config");
      if (!r.ok) throw new Error(`HTTP ${r.status}`);
      const cfg = decodeConfig(await r.json());
      setConfig(cfg);
      setSelectedId((prev) => {
        const ids = Object.keys(cfg.providers);
        if (prev && prev in cfg.providers) return prev;
        return ids[0] ?? null;
      });
      setDirty(false);
    } catch (e) {
      setLoadError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void fetchConfig();
  }, [fetchConfig]);

  // Sync textarea text when the selected provider changes.
  useEffect(() => {
    if (!config || !selectedId) return;
    const p = config.providers[selectedId];
    if (!p) return;
    setHeadersText(safeStringify(p.headers));
    setCompatText(safeStringify(p.compat));
  }, [selectedId, config]);

  // M2: reset all test pills for the selected provider when its identifying
  // fields change (a stale pill would mislead — design §5 "Reset pill when
  // provider fields/model id change"). Deps read from `config` directly
  // because `selected` is derived later in render.
  useEffect(() => {
    if (!selectedId) return;
    const p = config?.providers[selectedId];
    const prefix = `${selectedId}:`;
    setTestState((prev) => {
      const has = Object.keys(prev).some((k) => k.startsWith(prefix));
      if (!has) return prev;
      const next: typeof prev = {};
      for (const [k, v] of Object.entries(prev)) {
        if (!k.startsWith(prefix)) next[k] = v;
      }
      return next;
    });
    void p; // p read for deps below
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    selectedId,
    config?.providers[selectedId ?? ""]?.baseUrl,
    config?.providers[selectedId ?? ""]?.api,
    config?.providers[selectedId ?? ""]?.apiKey,
    config?.providers[selectedId ?? ""]?.anthropic,
  ]);

  const updateProvider = useCallback(
    (id: string, patch: Partial<ProviderEntry>): void => {
      setConfig((prev) => {
        if (!prev) return prev;
        const old = prev.providers[id];
        if (!old) return prev;
        return {
          ...prev,
          providers: {
            ...prev.providers,
            [id]: { ...old, ...patch },
          },
        };
      });
      setDirty(true);
    },
    [],
  );

  const updateModel = useCallback(
    (
      providerId: string,
      idx: number,
      patch: Partial<ModelEntry>,
    ): void => {
      setConfig((prev) => {
        if (!prev) return prev;
        const p = prev.providers[providerId];
        if (!p) return prev;
        const models = [...p.models];
        models[idx] = { ...models[idx], ...patch };
        return {
          ...prev,
          providers: {
            ...prev.providers,
            [providerId]: { ...p, models },
          },
        };
      });
      setDirty(true);
    },
    [],
  );

  const addModel = useCallback((providerId: string): void => {
    setConfig((prev) => {
      if (!prev) return prev;
      const p = prev.providers[providerId];
      if (!p) return prev;
      return {
        ...prev,
        providers: {
          ...prev.providers,
          [providerId]: {
            ...p,
            models: [...p.models, { id: "", reasoning: false }],
          },
        },
      };
    });
    setDirty(true);
  }, []);

  const deleteModel = useCallback(
    (providerId: string, idx: number): void => {
      setConfig((prev) => {
        if (!prev) return prev;
        const p = prev.providers[providerId];
        if (!p) return prev;
        const models = p.models.filter((_, i) => i !== idx);
        return {
          ...prev,
          providers: {
            ...prev.providers,
            [providerId]: { ...p, models },
          },
        };
      });
      setDirty(true);
    },
    [],
  );

  const addProvider = useCallback((): void => {
    const base = config ?? { version: 1, providers: {}, agents: {} };
    const id = genProviderId(base.providers);
    setConfig({
      ...base,
      providers: { ...base.providers, [id]: emptyProvider() },
    });
    setSelectedId(id);
    setDirty(true);
  }, [config]);

  const deleteProvider = useCallback(
    (id: string): void => {
      if (!config) return;
      const { [id]: _, ...rest } = config.providers;
      void _;
      setConfig({ ...config, providers: rest });
      setSelectedId((prev) => {
        const remaining = Object.keys(rest);
        return prev === id ? (remaining[0] ?? null) : prev;
      });
      setDirty(true);
    },
    [config],
  );

  const updateCost = useCallback(
    (
      providerId: string,
      idx: number,
      field: keyof CostEntry,
      val: string,
    ): void => {
      setConfig((prev) => {
        if (!prev) return prev;
        const p = prev.providers[providerId];
        if (!p) return prev;
        const models = [...p.models];
        const m = { ...models[idx] };
        const cost = { ...(m.cost ?? {}) };
        if (val === "") {
          delete cost[field];
        } else {
          const n = parseFloat(val);
          if (!isNaN(n)) cost[field] = n;
        }
        m.cost = Object.keys(cost).length > 0 ? cost : undefined;
        models[idx] = m;
        return {
          ...prev,
          providers: { ...prev.providers, [providerId]: { ...p, models } },
        };
      });
      setDirty(true);
    },
    [],
  );

  const handleSave = useCallback(async (): Promise<void> => {
    if (!config) return;
    setSaving(true);
    setSaveMsg(null);
    try {
      // Parse headers/compat text for the selected provider before sending.
      let body: CanonicalConfig = config;
      if (selectedId) {
        let headers: Record<string, string>;
        let compat: unknown;
        try {
          headers = headersText.trim()
            ? (JSON.parse(headersText) as Record<string, string>)
            : {};
        } catch {
          setSaveMsg({ ok: false, text: t(lang, "mcInvalidJson") + ": headers" });
          setSaving(false);
          return;
        }
        try {
          compat = compatText.trim() ? JSON.parse(compatText) : {};
        } catch {
          setSaveMsg({ ok: false, text: t(lang, "mcInvalidJson") + ": compat" });
          setSaving(false);
          return;
        }
        body = {
          ...config,
          providers: {
            ...config.providers,
            [selectedId]: {
              ...config.providers[selectedId],
              headers,
              compat,
            },
          },
        };
      }

      const r = await fetch("/api/models/config", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      if (!r.ok) {
        const text = await r.text();
        setSaveMsg({ ok: false, text });
        return;
      }
      const resp = (await r.json()) as PutResponse;
      const warnings = resp.warnings?.length
        ? resp.warnings.join("; ")
        : "";
      setSaveMsg({ ok: true, text: warnings || t(lang, "mcSaved") });
      // Refresh to pick up masked keys after write.
      await fetchConfig();
    } catch (e) {
      setSaveMsg({ ok: false, text: e instanceof Error ? e.message : String(e) });
    } finally {
      setSaving(false);
      window.setTimeout(() => setSaveMsg(null), 3000);
    }
  }, [config, selectedId, headersText, compatText, lang, fetchConfig]);

  const handleImport = useCallback(async (): Promise<void> => {
    if (!confirm(t(lang, "mcImportConfirm"))) return;
    try {
      const r = await fetch("/api/models/import/pi", { method: "POST" });
      if (!r.ok) {
        const text = await r.text();
        setSaveMsg({ ok: false, text });
        return;
      }
      const resp = (await r.json()) as ImportResponse;
      setSaveMsg({
        ok: true,
        text: t(lang, "mcImportResult")
          .replace("{imported}", String(resp.imported.length))
          .replace("{skipped}", String(resp.skipped.length)),
      });
      await fetchConfig();
    } catch (e) {
      setSaveMsg({ ok: false, text: e instanceof Error ? e.message : String(e) });
    } finally {
      window.setTimeout(() => setSaveMsg(null), 5000);
    }
  }, [lang, fetchConfig]);

  // ── M2 handlers ─────────────────────────────────────────────────

  // Run a single model availability test. Pill state is keyed by
  // `${providerId}:${modelId}`; reset to "testing" immediately, then ok/fail.
  const handleTest = useCallback(
    async (providerId: string, modelId: string): Promise<void> => {
      if (!modelId) return;
      const key = `${providerId}:${modelId}`;
      setTestState((prev) => ({
        ...prev,
        [key]: { status: "testing" },
      }));
      try {
        const r = await fetch("/api/models/test", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ providerId, modelId }),
        });
        // 400 (missing fields) is the only non-200 we expect here.
        if (r.status === 400) {
          setTestState((prev) => ({
            ...prev,
            [key]: { status: "fail", error: "bad request" },
          }));
          return;
        }
        const resp = (await r.json()) as TestResponse;
        setTestState((prev) => ({
          ...prev,
          [key]: {
            status: resp.ok ? "ok" : "fail",
            latencyMs: resp.latencyMs,
            statusHttp: resp.status,
            error: resp.error,
            responseText: resp.responseText,
          },
        }));
      } catch (e) {
        setTestState((prev) => ({
          ...prev,
          [key]: {
            status: "fail",
            error: e instanceof Error ? e.message : String(e),
          },
        }));
      }
    },
    [],
  );

  // Reset a row's test pill when provider fields or model id change. Called
  // from onChange handlers that touch the row's identifying inputs.
  const resetTest = useCallback((providerId: string, modelId: string): void => {
    const key = `${providerId}:${modelId}`;
    setTestState((prev) => {
      if (!prev[key] || prev[key].status === "idle") return prev;
      const next = { ...prev };
      delete next[key];
      return next;
    });
  }, []);

  // Determine whether the apiKey field for the selected provider has been
  // edited from the masked value returned by the server. When it has, the
  // discover call must send explicit literal fields (with the new key);
  // otherwise it sends `{providerId}` so the server uses the stored real key.
  const apiKeyDirty = useCallback(
    (provider: ProviderEntry | null): boolean => {
      if (!provider) return false;
      // A real key on submit looks like a mask ("sk-****XXXX") or a fresh
      // value. The field is "dirty" (new key typed) when it's non-empty AND
      // not the mask shape. We can't know the stored mask without the
      // pre-edit value, so we treat any value that doesn't look like a mask
      // (i.e. contains no "****") as a literal key the user typed.
      const k = provider.apiKey ?? "";
      return k.length > 0 && !k.includes("****");
    },
    [],
  );

  // Open the discover modal and fetch models from the selected provider's
  // endpoint. Models already in the table are not pre-checked.
  const handleFetchModels = useCallback(
    async (providerId: string, provider: ProviderEntry): Promise<void> => {
      setDiscover({
        loading: true,
        error: "",
        endpoint: "",
        models: [],
        filter: "",
        selected: new Set(),
      });
      try {
        // If the apiKey field holds a new (non-mask) value, send literal
        // fields so the server uses the typed key. Otherwise send providerId
        // and let the server resolve the stored real key.
        const dirtyKey = apiKeyDirty(provider);
        const body: Record<string, unknown> = dirtyKey
          ? {
              baseUrl: provider.baseUrl,
              api: provider.api,
              apiKey: provider.apiKey,
            }
          : { providerId };
        const r = await fetch("/api/models/discover", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(body),
        });
        if (!r.ok) {
          const text = await r.text();
          setDiscover((d) =>
            d ? { ...d, loading: false, error: text } : d,
          );
          return;
        }
        const resp = (await r.json()) as DiscoverResponse;
        setDiscover({
          loading: false,
          error: "",
          endpoint: resp.endpoint,
          models: resp.models,
          filter: "",
          selected: new Set(),
        });
      } catch (e) {
        setDiscover((d) =>
          d
            ? {
                ...d,
                loading: false,
                error: e instanceof Error ? e.message : String(e),
              }
            : d,
        );
      }
    },
    [apiKeyDirty],
  );

  // Merge selected discovered models into the selected provider's models list
  // (existing ids are left untouched; new ids are appended).
  const addDiscoveredModels = useCallback(
    (providerId: string, models: DiscoveredModel[]): void => {
      setConfig((prev) => {
        if (!prev) return prev;
        const p = prev.providers[providerId];
        if (!p) return prev;
        const existing = new Set(p.models.map((m) => m.id));
        const additions: ModelEntry[] = models
          .filter((m) => !existing.has(m.id))
          .map((m) => ({
            id: m.id,
            name: m.name,
            reasoning: false,
          }));
        if (additions.length === 0) return prev;
        return {
          ...prev,
          providers: {
            ...prev.providers,
            [providerId]: { ...p, models: [...p.models, ...additions] },
          },
        };
      });
      setDirty(true);
    },
    [],
  );

  // ── M3 handlers ─────────────────────────────────────────────────

  // Fetch per-agent install + live readback. Called on agent tab mount and
  // after every apply (to refresh the live line).
  const fetchAgents = useCallback(async (): Promise<void> => {
    try {
      const r = await fetch("/api/models/agents");
      if (!r.ok) return;
      setAgentsStatus(decodeAgents(await r.json()));
    } catch {
      /* leave previous state on fetch failure */
    }
  }, []);

  // Fetch agents status when an agent tab becomes active.
  useEffect(() => {
    if (tab === "pi" || tab === "opencode" || tab === "claude" || tab === "codex") {
      void fetchAgents();
    }
  }, [tab, fetchAgents]);

  // ── M4: usage fetch ─────────────────────────────────────────────

  // Fetch usage rows for the current window. `bypassCache=true` sends
  // ?refresh=1 so the server recomputes regardless of the 30s cache.
  const fetchUsage = useCallback(
    async (window: "today" | "7d" | "all", bypassCache: boolean): Promise<void> => {
      setUsageLoading(true);
      setUsageError("");
      try {
        const url = `/api/models/usage?window=${window}${bypassCache ? "&refresh=1" : ""}`;
        const r = await fetch(url);
        if (!r.ok) {
          setUsageError(`HTTP ${r.status}`);
          return;
        }
        const resp = decodeUsage(await r.json());
        setUsageRows(resp.rows);
        setUsageGeneratedAt(resp.generatedAt);
      } catch (e) {
        setUsageError(e instanceof Error ? e.message : String(e));
      } finally {
        setUsageLoading(false);
      }
    },
    [],
  );

  // Auto-fetch when the usage tab becomes active.
  useEffect(() => {
    if (tab === "usage") void fetchUsage(usageWindow, false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab, usageWindow, fetchUsage]);

  // Update an agent's assignment in config. Changing the provider resets
  // the model when the current one isn't in the new provider's list.
  const updateAgentAssignment = useCallback(
    (agent: AgentTab, patch: Record<string, unknown>): void => {
      setConfig((prev) => {
        if (!prev) return prev;
        const current = prev.agents[agent] as Record<string, unknown> | undefined;
        const next = { ...(current ?? {}), ...patch };
        // When the provider changed, reset the model if it's not in the new
        // provider's model list.
        if (
          patch.provider !== undefined &&
          (!current || patch.provider !== current.provider)
        ) {
          const p = prev.providers[patch.provider as string];
          const models = p?.models.map((m) => m.id) ?? [];
          if (!models.includes(next.model as string)) {
            next.model = models[0] ?? "";
          }
        }
        return {
          ...prev,
          agents: { ...prev.agents, [agent]: next },
        };
      });
      setAgentDirty((prev) => new Set(prev).add(agent));
      setApplyResult(null);
    },
    [],
  );

  // Save the agent assignment (PUT canonical). Sends the full config — the
  // providers section goes back as-is from the GET state (masked keys are
  // merged server-side via merge_api_keys).
  const handleSaveAssignment = useCallback(
    async (agent: AgentTab): Promise<void> => {
      if (!config) return;
      setSaving(true);
      setAgentSaveMsg(null);
      try {
        const r = await fetch("/api/models/config", {
          method: "PUT",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(config),
        });
        if (!r.ok) {
          const text = await r.text();
          setAgentSaveMsg({ ok: false, text });
          return;
        }
        setAgentDirty((prev) => {
          const n = new Set(prev);
          n.delete(agent);
          return n;
        });
        setAgentSaveMsg({ ok: true, text: t(lang, "mcSaved") });
        await fetchConfig();
      } catch (e) {
        setAgentSaveMsg({
          ok: false,
          text: e instanceof Error ? e.message : String(e),
        });
      } finally {
        setSaving(false);
        window.setTimeout(() => setAgentSaveMsg(null), 3000);
      }
    },
    [config, lang, fetchConfig],
  );

  // Apply the agent's saved assignment to its native config files.
  const handleApply = useCallback(
    async (agent: AgentTab): Promise<void> => {
      setApplying(true);
      setApplyResult(null);
      try {
        const r = await fetch(`/api/models/apply/${agent}`, { method: "POST" });
        const resp = (await r.json()) as ApplyResponse;
        setApplyResult(resp);
        // Refresh live readback after apply.
        await fetchAgents();
      } catch (e) {
        setApplyResult({
          ok: false,
          written: [],
          errors: [
            {
              path: agent,
              message: e instanceof Error ? e.message : String(e),
            },
          ],
        });
      } finally {
        setApplying(false);
      }
    },
    [fetchAgents],
  );

  // ── M4: usage tab renderer ─────────────────────────────────────

  /**
   * Render the usage statistics tab: window selector + refresh button,
   * a per-(agent,provider,model) table with a single 缓存 column showing
   * cacheRead+cacheWrite (raw split in the title), an optional cost column
   * shown only when at least one row carries a cost, and a 合计 footer.
   */
  function renderUsageTab(): JSX.Element {
    const windows: { key: "today" | "7d" | "all"; label: string }[] = [
      { key: "today", label: t(lang, "mcUsageToday") },
      { key: "7d", label: t(lang, "mcUsage7d") },
      { key: "all", label: t(lang, "mcUsageAll") },
    ];
    const rows = usageRows ?? [];
    const hasCost = rows.some((r) => r.cost !== undefined);
    const sumIn = rows.reduce((a, r) => a + r.in, 0);
    const sumOut = rows.reduce((a, r) => a + r.out, 0);
    const sumCache = rows.reduce(
      (a, r) => a + r.cacheRead + r.cacheWrite,
      0,
    );
    const sumCost = hasCost
      ? rows.reduce((a, r) => a + (r.cost ?? 0), 0)
      : 0;

    return (
      <div className="mc-usage" data-od-id="usage-tab">
        <div className="mc-usage-bar">
          {windows.map((w) => (
            <button
              key={w.key}
              className={`mc-tab mc-sm${usageWindow === w.key ? " active" : ""}`}
              onClick={() => setUsageWindow(w.key)}
            >
              {w.label}
            </button>
          ))}
          <button
            className="btn btn-secondary mc-sm"
            disabled={usageLoading}
            onClick={() => void fetchUsage(usageWindow, true)}
          >
            {usageLoading ? <Icon name="refresh" /> : null}
            {usageLoading
              ? t(lang, "mcUsageRefreshing")
              : t(lang, "mcUsageRefresh")}
          </button>
          {usageGeneratedAt && (
            <span className="mc-usage-gen">
              {t(lang, "mcUsageGeneratedAt")} {usageGeneratedAt}
            </span>
          )}
        </div>

        {usageError && <div className="mc-error">{usageError}</div>}

        {usageLoading && rows.length === 0 ? (
          <div className="mc-loading">{t(lang, "mcLoading")}</div>
        ) : rows.length === 0 ? (
          <div className="mc-empty">
            <p>{t(lang, "mcUsageEmpty")}</p>
          </div>
        ) : (
          <table className="mc-table mc-usage-table">
            <thead>
              <tr>
                <th>{t(lang, "mcUsageColAgent")}</th>
                <th>{t(lang, "mcUsageColProvider")}</th>
                <th>{t(lang, "mcUsageColModel")}</th>
                <th className="mc-num">{t(lang, "mcUsageColIn")}</th>
                <th className="mc-num">{t(lang, "mcUsageColOut")}</th>
                <th className="mc-num">{t(lang, "mcUsageColCache")}</th>
                {hasCost && (
                  <th className="mc-num">{t(lang, "mcUsageColCost")}</th>
                )}
              </tr>
            </thead>
            <tbody>
              {rows.map((r, i) => {
                const cacheTotal = r.cacheRead + r.cacheWrite;
                return (
                  <tr key={i}>
                    <td>{r.agent}</td>
                    <td>{r.provider ?? "—"}</td>
                    <td>{r.model}</td>
                    <td className="mc-num">{fmtTokens(r.in)}</td>
                    <td className="mc-num">{fmtTokens(r.out)}</td>
                    <td
                      className="mc-num"
                      title={`cacheRead=${r.cacheRead} cacheWrite=${r.cacheWrite}`}
                    >
                      {fmtTokens(cacheTotal)}
                    </td>
                    {hasCost && (
                      <td className="mc-num">
                        {r.cost !== undefined ? fmtCost(r.cost) : "—"}
                      </td>
                    )}
                  </tr>
                );
              })}
              <tr className="mc-usage-total">
                <td colSpan={3}>{t(lang, "mcUsageTotal")}</td>
                <td className="mc-num">{fmtTokens(sumIn)}</td>
                <td className="mc-num">{fmtTokens(sumOut)}</td>
                <td className="mc-num">{fmtTokens(sumCache)}</td>
                {hasCost && (
                  <td className="mc-num">{fmtCost(sumCost)}</td>
                )}
              </tr>
            </tbody>
          </table>
        )}
      </div>
    );
  }

  // ── render ─────────────────────────────────────────────────────

  if (loading) {
    return (
      <div className="pane pane-models">
        <div className="mc-loading">{t(lang, "mcLoading")}</div>
      </div>
    );
  }

  if (loadError) {
    return (
      <div className="pane pane-models">
        <div className="mc-error">
          {t(lang, "mcLoadFailed") + loadError}
        </div>
      </div>
    );
  }

  const providerIds = config ? Object.keys(config.providers) : [];
  const selected = config && selectedId ? config.providers[selectedId] : null;

  // ── M3: agent tab renderer ─────────────────────────────────────

  /**
   * Render one agent tab (pi/opencode/claude/codex): install badge, live
   * readback, provider/model dropdowns (incompatible providers disabled
   * with reason), agent-specific overrides, save + apply buttons, and the
   * apply result panel (written files + errors).
   */
  function renderAgentTab(agent: AgentTab): JSX.Element {
    const assignment = config?.agents[agent] as
      | Record<string, unknown>
      | undefined;
    const status = agentsStatus?.[agent];
    const isDirty = agentDirty.has(agent);
    const currentProviderId = (assignment?.provider as string) ?? "";
    const currentModelId = (assignment?.model as string) ?? "";
    const providerList = config
      ? Object.entries(config.providers)
      : [];

    return (
      <div className="mc-agent" data-od-id={`agent-${agent}`}>
        {/* install badge + live readback */}
        <div className="mc-agent-head">
          <span
            className={`mc-badge ${status?.installed ? "mc-badge-ok" : "mc-badge-warn"}`}
            title={status?.bin ?? undefined}
          >
            {status?.installed
              ? t(lang, "mcInstalled")
              : t(lang, "mcNotInstalled")}
          </span>
          <span className="mc-agent-live">
            {t(lang, "mcLive")} <code>{liveReadbackText(agent, status)}</code>
          </span>
        </div>

        <div className="mc-agent-form">
          {/* provider dropdown */}
          <div className="field">
            <label>{t(lang, "mcProvider")}</label>
            <select
              value={currentProviderId}
              onChange={(e) =>
                updateAgentAssignment(agent, { provider: e.target.value })
              }
            >
              <option value="">{t(lang, "mcSelectProvider")}</option>
              {providerList.map(([id, p]) => {
                const reason = incompatibleReason(agent, p);
                // The currently-selected provider stays selectable even if
                // it became incompatible after the assignment was saved
                // (so the user can see and change it).
                const isCurrent = id === currentProviderId;
                return (
                  <option
                    key={id}
                    value={id}
                    disabled={reason !== null && !isCurrent}
                  >
                    {p.name || id}
                    {reason && !isCurrent
                      ? reason === "incompatible-claude"
                        ? ` — ${t(lang, "mcIncompatibleClaude")}`
                        : ` — ${t(lang, "mcIncompatibleCodex")}`
                      : ""}
                  </option>
                );
              })}
            </select>
          </div>

          {/* model dropdown */}
          <div className="field">
            <label>{t(lang, "mcModel")}</label>
            <select
              value={currentModelId}
              onChange={(e) =>
                updateAgentAssignment(agent, { model: e.target.value })
              }
              disabled={!currentProviderId}
            >
              <option value="">—</option>
              {(config?.providers[currentProviderId]?.models ?? []).map((m) => (
                <option key={m.id} value={m.id}>
                  {m.name ? `${m.name} (${m.id})` : m.id}
                </option>
              ))}
            </select>
          </div>

          {/* claude overrides: three-tier model mapping + auth field */}
          {agent === "claude" && (
            <>
              <div className="field">
                <label>
                  {t(lang, "mcHaikuModel")}{" "}
                  <span className="mc-hint-inline">{t(lang, "mcFollowMain")}</span>
                </label>
                <input
                  value={(assignment?.haikuModel as string) ?? ""}
                  onChange={(e) =>
                    updateAgentAssignment(agent, {
                      haikuModel: e.target.value || null,
                    })
                  }
                />
              </div>
              <div className="field">
                <label>
                  {t(lang, "mcSonnetModel")}{" "}
                  <span className="mc-hint-inline">{t(lang, "mcFollowMain")}</span>
                </label>
                <input
                  value={(assignment?.sonnetModel as string) ?? ""}
                  onChange={(e) =>
                    updateAgentAssignment(agent, {
                      sonnetModel: e.target.value || null,
                    })
                  }
                />
              </div>
              <div className="field">
                <label>
                  {t(lang, "mcOpusModel")}{" "}
                  <span className="mc-hint-inline">{t(lang, "mcFollowMain")}</span>
                </label>
                <input
                  value={(assignment?.opusModel as string) ?? ""}
                  onChange={(e) =>
                    updateAgentAssignment(agent, {
                      opusModel: e.target.value || null,
                    })
                  }
                />
              </div>
              <div className="field">
                <label>{t(lang, "mcAuthField")}</label>
                <select
                  value={(assignment?.authField as string) ?? "AUTH_TOKEN"}
                  onChange={(e) =>
                    updateAgentAssignment(agent, { authField: e.target.value })
                  }
                >
                  <option value="AUTH_TOKEN">ANTHROPIC_AUTH_TOKEN</option>
                  <option value="API_KEY">ANTHROPIC_API_KEY</option>
                </select>
              </div>
            </>
          )}

          {/* codex overrides: reasoning effort + wire api */}
          {agent === "codex" && (
            <>
              <div className="field">
                <label>{t(lang, "mcReasoningEffort")}</label>
                <select
                  value={(assignment?.reasoningEffort as string) ?? ""}
                  onChange={(e) =>
                    updateAgentAssignment(agent, {
                      reasoningEffort: e.target.value || null,
                    })
                  }
                >
                  <option value="">{t(lang, "mcEffortNone")}</option>
                  <option value="low">low</option>
                  <option value="medium">medium</option>
                  <option value="high">high</option>
                </select>
              </div>
              <div className="field">
                <label>
                  {t(lang, "mcWireApi")}{" "}
                  <span className="mc-hint-inline">
                    {t(lang, "mcWireApiDerived")}
                  </span>
                </label>
                <select
                  value={(assignment?.wireApi as string) ?? ""}
                  onChange={(e) =>
                    updateAgentAssignment(agent, { wireApi: e.target.value })
                  }
                >
                  <option value="">{t(lang, "mcEffortNone")}</option>
                  <option value="responses">responses</option>
                  <option value="chat">chat</option>
                </select>
              </div>
            </>
          )}
        </div>

        {/* save + apply bar */}
        <div className="mc-save-bar">
          {isDirty && <span className="mc-dirty">{t(lang, "mcDirty")}</span>}
          {agentSaveMsg && (
            <span className={`mc-msg${agentSaveMsg.ok ? " ok" : " err"}`}>
              {agentSaveMsg.text}
            </span>
          )}
          <button
            className="btn btn-secondary"
            disabled={!isDirty || saving}
            onClick={() => void handleSaveAssignment(agent)}
          >
            {t(lang, "mcSaveAssignment")}
          </button>
          <button
            className="btn btn-primary"
            disabled={isDirty || applying || !currentProviderId || !currentModelId}
            onClick={() => void handleApply(agent)}
          >
            {applying ? <Icon name="refresh" /> : null}
            {applying ? t(lang, "mcApplying") : t(lang, "mcApply")}
          </button>
        </div>

        {/* apply result panel */}
        {applyResult && (
          <div className="mc-apply-result">
            {applyResult.ok && applyResult.errors.length === 0 && (
              <div className="mc-msg ok">{t(lang, "mcApplyOk")}</div>
            )}
            {applyResult.written.length > 0 && (
              <div className="mc-apply-written">
                <div className="mc-apply-label">{t(lang, "mcWrittenFiles")}</div>
                {applyResult.written.map((w) => (
                  <div key={w.path} className="mc-apply-file ok">
                    <code>{w.path}</code>
                    {w.backup && (
                      <span className="mc-apply-backup">→ {w.backup}</span>
                    )}
                  </div>
                ))}
              </div>
            )}
            {applyResult.errors.length > 0 && (
              <div className="mc-apply-errors">
                <div className="mc-apply-label err">{t(lang, "mcApplyErrors")}</div>
                {applyResult.errors.map((e) => (
                  <div key={e.path} className="mc-apply-file err">
                    <code>{e.path}</code>
                    <span className="mc-apply-msg">{e.message}</span>
                  </div>
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    );
  }

  return (
    <div className="pane pane-models" data-od-id="models-pane">
      <div className="mc-tabs">
        {TAB_KEYS.map((k) => (
          <button
            key={k}
            className={`mc-tab${tab === k ? " active" : ""}`}
            onClick={() => setTab(k)}
          >
            {tabLabel(lang, k)}
          </button>
        ))}
      </div>

      <div className="mc-body">
        {tab === "usage" ? (
          renderUsageTab()
        ) : tab !== "providers" ? (
          renderAgentTab(tab as AgentTab)
        ) : !config || providerIds.length === 0 ? (
          <div className="mc-empty">
            <p>{t(lang, "mcNoProviders")}</p>
            <div className="mc-empty-actions">
              <button className="btn btn-primary" onClick={addProvider}>
                <Icon name="plus" />
                {t(lang, "mcAddProvider")}
              </button>
              <button className="btn btn-secondary" onClick={handleImport}>
                {t(lang, "mcImportPi")}
              </button>
            </div>
          </div>
        ) : (
          <div className="mc-split">
            {/* ── provider list ── */}
            <div className="mc-list">
              <div className="mc-list-head">
                <button className="btn btn-secondary mc-sm" onClick={addProvider}>
                  <Icon name="plus" />
                </button>
                <button className="btn btn-secondary mc-sm" onClick={handleImport}>
                  {t(lang, "mcImportPi")}
                </button>
              </div>
              {providerIds.map((id) => {
                const p = config.providers[id];
                return (
                  <div
                    key={id}
                    className={`mc-list-item${selectedId === id ? " active" : ""}`}
                    onClick={() => {
                      setSelectedId(id);
                      setShowAdvanced(false);
                    }}
                  >
                    <div className="mc-list-name">
                      {p.name || id}
                    </div>
                    <div className="mc-list-meta">
                      {p.baseUrl || "—"} · {p.models.length} model{p.models.length !== 1 ? "s" : ""}
                    </div>
                    <button
                      className="mc-del-btn"
                      title={t(lang, "mcDeleteProvider")}
                      onClick={(e) => {
                        e.stopPropagation();
                        deleteProvider(id);
                      }}
                    >
                      <Icon name="x" />
                    </button>
                  </div>
                );
              })}
            </div>

            {/* ── detail ── */}
            <div className="mc-detail">
              {!selected ? (
                <p className="mc-hint">{t(lang, "mcSelectProvider")}</p>
              ) : (
                <div className="mc-form">
                  <div className="field">
                    <label>{t(lang, "mcName")}</label>
                    <input
                      value={selected.name}
                      onChange={(e) => updateProvider(selectedId!, { name: e.target.value })}
                    />
                  </div>
                  <div className="field">
                    <label>{t(lang, "mcBaseUrl")}</label>
                    <input
                      value={selected.baseUrl}
                      onChange={(e) => updateProvider(selectedId!, { baseUrl: e.target.value })}
                      placeholder="https://api.example.com/v1"
                    />
                  </div>
                  <div className="field">
                    <label>{t(lang, "mcApi")}</label>
                    <select
                      value={selected.api}
                      onChange={(e) => updateProvider(selectedId!, { api: e.target.value })}
                    >
                      {API_PROTOCOLS.map((p) => (
                        <option key={p} value={p}>{p}</option>
                      ))}
                    </select>
                  </div>
                  <div className="field">
                    <label>{t(lang, "mcApiKey")}</label>
                    <input
                      type="password"
                      value={selected.apiKey ?? ""}
                      placeholder={t(lang, "mcApiKeyPh")}
                      onChange={(e) => updateProvider(selectedId!, { apiKey: e.target.value })}
                    />
                  </div>
                  <div className="field mc-check">
                    <label>
                      <input
                        type="checkbox"
                        checked={selected.anthropic != null}
                        onChange={(e) =>
                          updateProvider(selectedId!, {
                            anthropic: e.target.checked ? {} : null,
                          })
                        }
                      />
                      {t(lang, "mcAnthropic")}
                    </label>
                    {selected.anthropic != null && (
                      <input
                        className="mc-sub-input"
                        value={selected.anthropic?.baseUrl ?? ""}
                        placeholder={t(lang, "mcAnthropicBaseUrl")}
                        onChange={(e) =>
                          updateProvider(selectedId!, {
                            anthropic: { baseUrl: e.target.value || undefined },
                          })
                        }
                      />
                    )}
                  </div>

                  <button
                    className="mc-adv-toggle"
                    onClick={() => setShowAdvanced((v) => !v)}
                  >
                    {t(lang, "mcAdvanced")}
                    <Icon name={showAdvanced ? "chev-r" : "chev-l"} />
                  </button>
                  {showAdvanced && (
                    <div className="mc-advanced">
                      <div className="field">
                        <label>{t(lang, "mcHeaders")}</label>
                        <textarea
                          className="mc-json-area"
                          value={headersText}
                          onChange={(e) => setHeadersText(e.target.value)}
                          rows={4}
                          spellCheck={false}
                        />
                      </div>
                      <div className="field">
                        <label>{t(lang, "mcCompat")}</label>
                        <textarea
                          className="mc-json-area"
                          value={compatText}
                          onChange={(e) => setCompatText(e.target.value)}
                          rows={4}
                          spellCheck={false}
                        />
                      </div>
                    </div>
                  )}

                  {/* ── models table ── */}
                  <div className="mc-models-section">
                    <div className="mc-models-head">
                      <span>{t(lang, "mcModels")}</span>
                      <div className="mc-models-head-actions">
                        <button
                          className="btn btn-secondary mc-sm"
                          onClick={() => handleFetchModels(selectedId!, selected)}
                          disabled={!selected.baseUrl}
                        >
                          {t(lang, "mcFetchModels")}
                        </button>
                        <button
                          className="btn btn-secondary mc-sm"
                          onClick={() => addModel(selectedId!)}
                        >
                          <Icon name="plus" />
                          {t(lang, "mcAddModel")}
                        </button>
                      </div>
                    </div>
                    <table className="mc-table">
                      <thead>
                        <tr>
                          <th>{t(lang, "mcModelId")}</th>
                          <th>{t(lang, "mcModelName")}</th>
                          <th className="mc-chk">{t(lang, "mcReasoning")}</th>
                          <th>{t(lang, "mcContextWindow")}</th>
                          <th>{t(lang, "mcMaxTokens")}</th>
                          <th>{t(lang, "mcCost")} →</th>
                          <th>in</th>
                          <th>out</th>
                          <th>cacheR</th>
                          <th>cacheW</th>
                          <th>{t(lang, "mcTest")}</th>
                          <th></th>
                        </tr>
                      </thead>
                      <tbody>
                        {selected.models.map((m, idx) => {
                          const tkey = `${selectedId}:${m.id}`;
                          const ts = testState[tkey];
                          return (
                          <tr key={idx}>
                            <td>
                              <input
                                value={m.id}
                                onChange={(e) => {
                                  resetTest(selectedId!, m.id);
                                  updateModel(selectedId!, idx, { id: e.target.value });
                                }}
                              />
                            </td>
                            <td>
                              <input
                                value={m.name ?? ""}
                                onChange={(e) => updateModel(selectedId!, idx, { name: e.target.value || undefined })}
                              />
                            </td>
                            <td className="mc-chk">
                              <input
                                type="checkbox"
                                checked={m.reasoning ?? false}
                                onChange={(e) => updateModel(selectedId!, idx, { reasoning: e.target.checked })}
                              />
                            </td>
                            <td>
                              <input
                                type="number"
                                value={m.contextWindow ?? ""}
                                onChange={(e) => updateModel(selectedId!, idx, {
                                  contextWindow: e.target.value ? parseInt(e.target.value, 10) : undefined,
                                })}
                              />
                            </td>
                            <td>
                              <input
                                type="number"
                                value={m.maxTokens ?? ""}
                                onChange={(e) => updateModel(selectedId!, idx, {
                                  maxTokens: e.target.value ? parseInt(e.target.value, 10) : undefined,
                                })}
                              />
                            </td>
                            <td className="mc-cost-sep"></td>
                            <td>
                              <input
                                type="number"
                                step="any"
                                value={m.cost?.input ?? ""}
                                onChange={(e) => updateCost(selectedId!, idx, "input", e.target.value)}
                              />
                            </td>
                            <td>
                              <input
                                type="number"
                                step="any"
                                value={m.cost?.output ?? ""}
                                onChange={(e) => updateCost(selectedId!, idx, "output", e.target.value)}
                              />
                            </td>
                            <td>
                              <input
                                type="number"
                                step="any"
                                value={m.cost?.cacheRead ?? ""}
                                onChange={(e) => updateCost(selectedId!, idx, "cacheRead", e.target.value)}
                              />
                            </td>
                            <td>
                              <input
                                type="number"
                                step="any"
                                value={m.cost?.cacheWrite ?? ""}
                                onChange={(e) => updateCost(selectedId!, idx, "cacheWrite", e.target.value)}
                              />
                            </td>
                            <td>
                              <div className="mc-test-cell">
                                <button
                                  className="btn btn-secondary mc-sm"
                                  onClick={() => handleTest(selectedId!, m.id)}
                                  disabled={!m.id || ts?.status === "testing"}
                                >
                                  {ts?.status === "testing"
                                    ? t(lang, "mcTesting")
                                    : t(lang, "mcTest")}
                                </button>
                                {ts && ts.status !== "idle" && (
                                  <span
                                    className={`mc-pill mc-pill-${ts.status}`}
                                    title={
                                      ts.status === "ok"
                                        ? `HTTP ${ts.statusHttp ?? "?"} · ${ts.responseText ?? ""}`
                                        : ts.status === "fail"
                                          ? ts.error ?? ""
                                          : ""
                                    }
                                  >
                                    {ts.status === "ok"
                                      ? `${t(lang, "mcTestOk")} · ${ts.latencyMs ?? "?"}ms`
                                      : ts.status === "fail"
                                        ? t(lang, "mcTestFail")
                                        : "…"}
                                  </span>
                                )}
                              </div>
                            </td>
                            <td>
                              <button
                                className="mc-del-btn"
                                onClick={() => deleteModel(selectedId!, idx)}
                              >
                                <Icon name="x" />
                              </button>
                            </td>
                          </tr>
                          );
                        })}
                      </tbody>
                    </table>
                  </div>

                  {/* ── save bar ── */}
                  <div className="mc-save-bar">
                    {dirty && <span className="mc-dirty">{t(lang, "mcDirty")}</span>}
                    {saveMsg && (
                      <span className={`mc-msg${saveMsg.ok ? " ok" : " err"}`}>
                        {saveMsg.text}
                      </span>
                    )}
                    <button
                      className="btn btn-primary"
                      disabled={!dirty || saving}
                      onClick={handleSave}
                    >
                      {saving ? <Icon name="refresh" /> : null}
                      {saving ? t(lang, "mcSaving") : t(lang, "mcSave")}
                    </button>
                  </div>
                </div>
              )}
            </div>
          </div>
        )}
      </div>

      {/* ── M2: discover modal ── */}
      {discover && (
        <div className="overlay open" data-od-id="discover-modal">
          <div className="dialog mc-discover">
            <div className="dialog-head">
              <span>{t(lang, "mcFetchModels")}</span>
              <button
                className="mc-del-btn"
                title={t(lang, "mcClose")}
                onClick={() => setDiscover(null)}
              >
                <Icon name="x" />
              </button>
            </div>
            <div className="mc-discover-body">
              {discover.loading ? (
                <div className="mc-loading">{t(lang, "mcLoading")}</div>
              ) : discover.error ? (
                <div className="mc-error">
                  {t(lang, "mcDiscoverFailed") + discover.error}
                </div>
              ) : (
                <>
                  <div className="mc-discover-endpoint">
                    {t(lang, "mcDiscoverEndpoint")}
                    <code>{discover.endpoint}</code>
                  </div>
                  {discover.models.length === 0 ? (
                    <div className="mc-hint">{t(lang, "mcDiscoverEmpty")}</div>
                  ) : (
                    <>
                      <input
                        className="mc-discover-search"
                        placeholder={t(lang, "mcSearch")}
                        value={discover.filter}
                        onChange={(e) =>
                          setDiscover((d) =>
                            d ? { ...d, filter: e.target.value } : d,
                          )
                        }
                      />
                      <div className="mc-discover-list">
                        {(() => {
                          const existing = new Set(
                            (config?.providers[selectedId ?? ""]?.models ?? []).map(
                              (m) => m.id,
                            ),
                          );
                          const q = discover.filter.trim().toLowerCase();
                          const shown = discover.models.filter(
                            (m) =>
                              !q ||
                              m.id.toLowerCase().includes(q) ||
                              (m.name ?? "").toLowerCase().includes(q),
                          );
                          if (shown.length === 0) {
                            return <div className="mc-hint">—</div>;
                          }
                          return shown.map((m) => {
                            const already = existing.has(m.id);
                            const checked =
                              already || discover.selected.has(m.id);
                            return (
                              <label
                                key={m.id}
                                className={`mc-discover-item${already ? " is-existing" : ""}`}
                              >
                                <input
                                  type="checkbox"
                                  checked={checked}
                                  disabled={already}
                                  onChange={(e) => {
                                    setDiscover((d) => {
                                      if (!d) return d;
                                      const sel = new Set(d.selected);
                                      if (e.target.checked) sel.add(m.id);
                                      else sel.delete(m.id);
                                      return { ...d, selected: sel };
                                    });
                                  }}
                                />
                                <span className="mc-discover-id">{m.id}</span>
                                {m.name && (
                                  <span className="mc-discover-name">{m.name}</span>
                                )}
                                {already && (
                                  <span className="mc-discover-tag">✓</span>
                                )}
                              </label>
                            );
                          });
                        })()}
                      </div>
                      <div className="mc-discover-actions">
                        <button
                          className="btn btn-secondary mc-sm"
                          onClick={() => {
                            const existing = new Set(
                              (config?.providers[selectedId ?? ""]?.models ?? []).map(
                                (m) => m.id,
                              ),
                            );
                            const q = discover.filter.trim().toLowerCase();
                            const shown = discover.models.filter(
                              (m) =>
                                !q ||
                                m.id.toLowerCase().includes(q) ||
                                (m.name ?? "").toLowerCase().includes(q),
                            );
                            setDiscover((d) => {
                              if (!d) return d;
                              const sel = new Set(d.selected);
                              for (const m of shown) {
                                if (!existing.has(m.id)) sel.add(m.id);
                              }
                              return { ...d, selected: sel };
                            });
                          }}
                        >
                          {t(lang, "mcSelectAllPage")}
                        </button>
                        <button
                          className="btn btn-secondary mc-sm"
                          onClick={() =>
                            setDiscover((d) =>
                              d ? { ...d, selected: new Set() } : d,
                            )
                          }
                        >
                          {t(lang, "mcClearSel")}
                        </button>
                        <span className="mc-discover-count">
                          {discover.selected.size}
                        </span>
                        <button
                          className="btn btn-primary mc-sm"
                          disabled={discover.selected.size === 0}
                          onClick={() => {
                            const chosen = discover.models.filter((m) =>
                              discover.selected.has(m.id),
                            );
                            if (selectedId && chosen.length > 0) {
                              addDiscoveredModels(selectedId, chosen);
                            }
                            setDiscover(null);
                          }}
                        >
                          {t(lang, "mcAddSelected")}
                        </button>
                      </div>
                    </>
                  )}
                </>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
