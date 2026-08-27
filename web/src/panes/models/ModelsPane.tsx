// ModelsPane — shell for the unified model config page (native "page" pane).
//
// Owns ALL state and /api/models/* handlers; renders the tab bar and delegates
// each tab's render to a focused sub-component:
//   providers → ProviderGrid (cc-switch card grid) + ProviderEditor drawer
//   pi/opencode/claude/codex → AgentTabs
//   usage → UsageTab
//
// API contract: GET/PUT /api/models/config + POST /api/models/import/pi +
// GET /api/models/agents + POST /api/models/apply/:agent + GET /api/models/usage.
// Responses are decoded once here (types.ts) — all rendering uses the typed
// CanonicalConfig / AgentsResponse / UsageResponse (cross-layer-thinking-guide).

import { useCallback, useEffect, useState } from "react";
import type { ServiceEntry } from "../../types";
import { Icon } from "../../icons";
import { t, type Lang } from "../../i18n";
import { AgentTabs } from "./AgentTabs";
import { PresetList } from "./PresetList";
import { ProviderEditor } from "./ProviderEditor";
import { ProviderGrid } from "./ProviderGrid";
import { UsageTab, type UsageWindow } from "./UsageTab";
import {
  decodeAgents,
  decodeConfig,
  decodeUsage,
  emptyProvider,
  genProviderId,
  safeStringify,
  type AgentTab,
  type AgentsResponse,
  type AnyPreset,
  type ApplyResponse,
  type CanonicalConfig,
  type CostEntry,
  type DiscoverState,
  type DiscoveredModel,
  type ModelEntry,
  type PresetAgent,
  type ProviderEntry,
  type TestStateMap,
  type UsageRow,
} from "./types";

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

  // Editor drawer state: `selectedId` non-null opens the drawer for that
  // provider. It also carries the provider whose headers/compat textareas are
  // live, and scopes the test-pill reset effect.
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saveMsg, setSaveMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [headersText, setHeadersText] = useState("");
  const [compatText, setCompatText] = useState("");
  const [showAdvanced, setShowAdvanced] = useState(false);

  // M2: per-(provider,model) test pills + discover modal.
  const [testState, setTestState] = useState<TestStateMap>({});
  const [discover, setDiscover] = useState<DiscoverState | null>(null);

  // M3: agent tabs.
  const [agentsStatus, setAgentsStatus] = useState<AgentsResponse | null>(null);
  const [agentDirty, setAgentDirty] = useState<Set<string>>(new Set());
  const [applying, setApplying] = useState(false);
  const [applyResult, setApplyResult] = useState<ApplyResponse | null>(null);
  const [agentSaveMsg, setAgentSaveMsg] = useState<{
    ok: boolean;
    text: string;
  } | null>(null);

  // M4: usage tab.
  const [usageRows, setUsageRows] = useState<UsageRow[] | null>(null);
  const [usageGeneratedAt, setUsageGeneratedAt] = useState("");
  const [usageWindow, setUsageWindow] = useState<UsageWindow>("today");
  const [usageLoading, setUsageLoading] = useState(false);
  const [usageError, setUsageError] = useState("");

  // ── config fetch / save ─────────────────────────────────────────

  const fetchConfig = useCallback(async (): Promise<void> => {
    try {
      const r = await fetch("/api/models/config");
      if (!r.ok) throw new Error(`HTTP ${r.status}`);
      const cfg = decodeConfig(await r.json());
      setConfig(cfg);
      setSelectedId((prev) =>
        prev && prev in cfg.providers ? prev : null,
      );
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

  // Sync the advanced JSON textareas when the selected provider changes.
  useEffect(() => {
    if (!config || !selectedId) return;
    const p = config.providers[selectedId];
    if (!p) return;
    setHeadersText(safeStringify(p.headers));
    setCompatText(safeStringify(p.compat));
  }, [selectedId, config]);

  // Reset all test pills for the selected provider when its identifying
  // fields change (a stale pill would mislead — design §5).
  useEffect(() => {
    if (!selectedId) return;
    setTestState((prev) => {
      const prefix = `${selectedId}:`;
      const has = Object.keys(prev).some((k) => k.startsWith(prefix));
      if (!has) return prev;
      const next: typeof prev = {};
      for (const [k, v] of Object.entries(prev)) {
        if (!k.startsWith(prefix)) next[k] = v;
      }
      return next;
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    selectedId,
    config?.providers[selectedId ?? ""]?.baseUrl,
    config?.providers[selectedId ?? ""]?.api,
    config?.providers[selectedId ?? ""]?.apiKey,
  ]);

  // ── provider CRUD ───────────────────────────────────────────────

  const updateProvider = useCallback(
    (id: string, patch: Partial<ProviderEntry>): void => {
      setConfig((prev) => {
        if (!prev) return prev;
        const old = prev.providers[id];
        if (!old) return prev;
        return {
          ...prev,
          providers: { ...prev.providers, [id]: { ...old, ...patch } },
        };
      });
      setDirty(true);
    },
    [],
  );

  const updateModel = useCallback(
    (providerId: string, idx: number, patch: Partial<ModelEntry>): void => {
      setConfig((prev) => {
        if (!prev) return prev;
        const p = prev.providers[providerId];
        if (!p) return prev;
        const models = [...p.models];
        models[idx] = { ...models[idx], ...patch };
        return {
          ...prev,
          providers: { ...prev.providers, [providerId]: { ...p, models } },
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
          [providerId]: { ...p, models: [...p.models, { id: "", reasoning: false }] },
        },
      };
    });
    setDirty(true);
  }, []);

  const deleteModel = useCallback((providerId: string, idx: number): void => {
    setConfig((prev) => {
      if (!prev) return prev;
      const p = prev.providers[providerId];
      if (!p) return prev;
      const models = p.models.filter((_, i) => i !== idx);
      return {
        ...prev,
        providers: { ...prev.providers, [providerId]: { ...p, models } },
      };
    });
    setDirty(true);
  }, []);

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

  const deleteProvider = useCallback((id: string): void => {
    if (!config) return;
    const { [id]: _, ...rest } = config.providers;
    void _;
    setConfig({ ...config, providers: rest });
    setSelectedId((prev) => (prev === id ? null : prev));
    setDirty(true);
  }, [config]);

  const updateCost = useCallback(
    (providerId: string, idx: number, field: keyof CostEntry, val: string): void => {
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
    if (!config || !selectedId) return;
    setSaving(true);
    setSaveMsg(null);
    try {
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
      const body: CanonicalConfig = {
        ...config,
        providers: {
          ...config.providers,
          [selectedId]: { ...config.providers[selectedId], headers, compat },
        },
      };

      const r = await fetch("/api/models/config", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      if (!r.ok) {
        setSaveMsg({ ok: false, text: await r.text() });
        return;
      }
      const resp = (await r.json()) as { ok: boolean; warnings?: string[] };
      setSaveMsg({
        ok: true,
        text:
          resp.warnings?.length && resp.warnings.length > 0
            ? resp.warnings.join("; ")
            : t(lang, "mcSaved"),
      });
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
        setSaveMsg({ ok: false, text: await r.text() });
        return;
      }
      const resp = (await r.json()) as { imported: string[]; skipped: string[] };
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

  // ── M2: test + discover ─────────────────────────────────────────

  const handleTest = useCallback(
    async (providerId: string, modelId: string): Promise<void> => {
      if (!modelId) return;
      const key = `${providerId}:${modelId}`;
      setTestState((prev) => ({ ...prev, [key]: { status: "testing" } }));
      try {
        const r = await fetch("/api/models/test", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ providerId, modelId }),
        });
        if (r.status === 400) {
          setTestState((prev) => ({
            ...prev,
            [key]: { status: "fail", error: "bad request" },
          }));
          return;
        }
        const resp = (await r.json()) as {
          ok: boolean;
          latencyMs?: number;
          status?: number;
          error?: string;
          responseText?: string;
        };
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

  const resetTest = useCallback((providerId: string, modelId: string): void => {
    const key = `${providerId}:${modelId}`;
    setTestState((prev) => {
      if (!prev[key] || prev[key].status === "idle") return prev;
      const next = { ...prev };
      delete next[key];
      return next;
    });
  }, []);

  /** True when the apiKey field holds a freshly typed literal (not the mask). */
  const apiKeyDirty = useCallback((provider: ProviderEntry): boolean => {
    const k = provider.apiKey ?? "";
    return k.length > 0 && !k.includes("****");
  }, []);

  // Open the discover modal and fetch models for the selected provider.
  const handleFetchModels = useCallback(async (): Promise<void> => {
    if (!selectedId) return;
    const provider = config?.providers[selectedId];
    if (!provider) return;
    setDiscover({ loading: true, error: "", endpoint: "", models: [], filter: "", selected: new Set() });
    try {
      const dirtyKey = apiKeyDirty(provider);
      const body: Record<string, unknown> = dirtyKey
        ? { baseUrl: provider.baseUrl, api: provider.api, apiKey: provider.apiKey }
        : { providerId: selectedId };
      const r = await fetch("/api/models/discover", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      if (!r.ok) {
        const text = await r.text();
        setDiscover((d) => (d ? { ...d, loading: false, error: text } : d));
        return;
      }
      const resp = (await r.json()) as { models: DiscoveredModel[]; endpoint: string };
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
  }, [selectedId, config, apiKeyDirty]);

  // Merge the discover-selected models into the selected provider.
  const handleDiscoverAddSelected = useCallback((): void => {
    if (!selectedId || !discover) return;
    const chosen = discover.models.filter((m) => discover.selected.has(m.id));
    if (chosen.length === 0) return;
    setConfig((prev) => {
      if (!prev) return prev;
      const p = prev.providers[selectedId];
      if (!p) return prev;
      const existing = new Set(p.models.map((m) => m.id));
      const additions: ModelEntry[] = chosen
        .filter((m) => !existing.has(m.id))
        .map((m) => ({ id: m.id, name: m.name, reasoning: false }));
      if (additions.length === 0) return prev;
      return {
        ...prev,
        providers: {
          ...prev.providers,
          [selectedId]: { ...p, models: [...p.models, ...additions] },
        },
      };
    });
    setDirty(true);
  }, [selectedId, discover]);

  // ── M3: agents ──────────────────────────────────────────────────

  const fetchAgents = useCallback(async (): Promise<void> => {
    try {
      const r = await fetch("/api/models/agents");
      if (!r.ok) return;
      setAgentsStatus(decodeAgents(await r.json()));
    } catch {
      /* leave previous state on fetch failure */
    }
  }, []);

  useEffect(() => {
    if (tab === "pi" || tab === "opencode" || tab === "claude" || tab === "codex") {
      void fetchAgents();
    }
  }, [tab, fetchAgents]);

  const updateAgentAssignment = useCallback(
    (agent: "pi" | "opencode", patch: Record<string, unknown>): void => {
      setConfig((prev) => {
        if (!prev) return prev;
        const current = prev.agents[agent];
        const next = { ...(current ?? { provider: "", model: "" }), ...patch };
        // When the provider changed, reset the model if it's not in the new
        // provider's model list.
        if (
          patch.provider !== undefined &&
          (!current || patch.provider !== current.provider)
        ) {
          const p = prev.providers[patch.provider as string];
          const models = p?.models.map((m) => m.id) ?? [];
          if (!models.includes(next.model)) {
            next.model = models[0] ?? "";
          }
        }
        return { ...prev, agents: { ...prev.agents, [agent]: next } };
      });
      setAgentDirty((prev) => new Set(prev).add(agent));
      setApplyResult(null);
    },
    [],
  );

  // ── claude/codex preset CRUD (design §4) ──────────────────────
  //
  // All five edit the local canonical state and mark the agent dirty; the
  // user commits via the shared save bar (same PUT /api/models/config
  // channel). Switch is the exception - it setCurrent + save + apply in one
  // click so a preset takes effect immediately.

  /** Replace the preset list block for a switch-style agent. */
  const setPresets = useCallback(
    (
      agent: PresetAgent,
      build: (block: { presets: AnyPreset[]; current?: string | null } | undefined) => {
        presets: AnyPreset[];
        current?: string | null;
      },
    ): void => {
      setConfig((prev) => {
        if (!prev) return prev;
        const existing =
          agent === "claude" ? prev.agents.claude : prev.agents.codex;
        const next = build(
          existing
            ? { presets: existing.presets, current: existing.current }
            : undefined,
        );
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

  const addPreset = useCallback(
    (agent: PresetAgent, preset: AnyPreset): void => {
      setPresets(agent, (block) => {
        const presets = [...(block?.presets ?? []), preset];
        // First preset auto-becomes current (sent as "" until the backend
        // backfills the id - design §2). Otherwise leave current alone.
        const current =
          (block?.presets?.length ?? 0) === 0 ? "" : block?.current ?? null;
        return { presets, current };
      });
    },
    [setPresets],
  );

  const updatePreset = useCallback(
    (agent: PresetAgent, id: string, preset: AnyPreset): void => {
      setPresets(agent, (block) => {
        const presets = (block?.presets ?? []).map((p) =>
          p.id === id ? { ...preset, id } : p,
        );
        return { presets, current: block?.current ?? null };
      });
    },
    [setPresets],
  );

  const deletePreset = useCallback(
    (agent: PresetAgent, id: string): void => {
      setPresets(agent, (block) => {
        const presets = (block?.presets ?? []).filter((p) => p.id !== id);
        // Deleting the current preset: shift current to the first remaining
        // (or null). Never dangle (PRD AC; design §2).
        let current = block?.current ?? null;
        if (current === id) {
          current = presets[0]?.id ?? null;
        }
        return { presets, current };
      });
    },
    [setPresets],
  );

  const duplicatePreset = useCallback(
    (agent: PresetAgent, id: string): void => {
      setPresets(agent, (block) => {
        const src = (block?.presets ?? []).find((p) => p.id === id);
        if (!src) return { presets: block?.presets ?? [], current: block?.current ?? null };
        // New id (backend backfills); name gets the copy suffix; insert right
        // after the source so it appears adjacent (design §4).
        const copy: AnyPreset = {
          ...(src as object),
          id: "",
          name: `${src.name} ${t(lang, "maCopySuffix")}`,
        } as AnyPreset;
        const presets: AnyPreset[] = [];
        for (const p of block?.presets ?? []) {
          presets.push(p);
          if (p.id === id) presets.push(copy);
        }
        return { presets, current: block?.current ?? null };
      });
    },
    [setPresets, lang],
  );

  /** Switch = setCurrent + save + apply, one click (design §4). */
  const handleSwitchPreset = useCallback(
    async (agent: PresetAgent, id: string): Promise<void> => {
      if (!config || id === "") return;
      const block = agent === "claude" ? config.agents.claude : config.agents.codex;
      const next =
        id === (block?.current ?? null)
          ? config // already current -> just apply
          : {
              ...config,
              agents: { ...config.agents, [agent]: { ...block, current: id } },
            };
      setConfig(next);
      setSaving(true);
      setAgentSaveMsg(null);
      setApplyResult(null);
      try {
        const r = await fetch("/api/models/config", {
          method: "PUT",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(next),
        });
        if (!r.ok) {
          setAgentSaveMsg({ ok: false, text: await r.text() });
          return;
        }
        await fetchConfig();
        setAgentDirty((prev) => {
          const n = new Set(prev);
          n.delete(agent);
          return n;
        });
        // Apply the now-current preset to the agent's native files.
        setApplying(true);
        const ar = await fetch(`/api/models/apply/${agent}`, { method: "POST" });
        setApplyResult((await ar.json()) as ApplyResponse);
        await fetchAgents();
      } catch (e) {
        setApplyResult({
          ok: false,
          written: [],
          errors: [
            { path: agent, message: e instanceof Error ? e.message : String(e) },
          ],
        });
      } finally {
        setSaving(false);
        setApplying(false);
        window.setTimeout(() => setAgentSaveMsg(null), 3000);
      }
    },
    [config, fetchConfig, fetchAgents],
  );

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
          setAgentSaveMsg({ ok: false, text: await r.text() });
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

  const handleApply = useCallback(
    async (agent: AgentTab): Promise<void> => {
      setApplying(true);
      setApplyResult(null);
      try {
        const r = await fetch(`/api/models/apply/${agent}`, { method: "POST" });
        setApplyResult((await r.json()) as ApplyResponse);
        await fetchAgents();
      } catch (e) {
        setApplyResult({
          ok: false,
          written: [],
          errors: [
            { path: agent, message: e instanceof Error ? e.message : String(e) },
          ],
        });
      } finally {
        setApplying(false);
      }
    },
    [fetchAgents],
  );

  // ── M4: usage ───────────────────────────────────────────────────

  const fetchUsage = useCallback(
    async (window: UsageWindow, bypassCache: boolean): Promise<void> => {
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

  useEffect(() => {
    if (tab === "usage") void fetchUsage(usageWindow, false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab, usageWindow, fetchUsage]);

  // ── render ──────────────────────────────────────────────────────

  if (loading) {
    return (
      <div className="pane pane-models">
        <div className="ml-loading">{t(lang, "mcLoading")}</div>
      </div>
    );
  }

  if (loadError) {
    return (
      <div className="pane pane-models">
        <div className="ml-error">{t(lang, "mcLoadFailed") + loadError}</div>
      </div>
    );
  }

  const selected = config && selectedId ? config.providers[selectedId] : null;

  const jumpToAgent = (agent: AgentTab): void => {
    setSelectedId(null);
    setTab(agent);
  };

  return (
    <div className="pane pane-models" data-od-id="models-pane">
      <div className="ml-tabs">
        {TAB_KEYS.map((k) => (
          <button
            key={k}
            className={`ml-tab${tab === k ? " active" : ""}`}
            onClick={() => setTab(k)}
          >
            {tabLabel(lang, k)}
          </button>
        ))}
      </div>

      <div className="ml-body">
        {tab === "usage" ? (
          <UsageTab
            rows={usageRows}
            generatedAt={usageGeneratedAt}
            window={usageWindow}
            loading={usageLoading}
            error={usageError}
            onWindowChange={setUsageWindow}
            onRefresh={() => void fetchUsage(usageWindow, true)}
            lang={lang}
          />
        ) : tab === "claude" || tab === "codex" ? (
          <PresetList
            agent={tab}
            config={config ?? { version: 1, providers: {}, agents: {} }}
            agentsStatus={agentsStatus}
            agentDirty={agentDirty}
            saving={saving}
            applying={applying}
            applyResult={applyResult}
            agentSaveMsg={agentSaveMsg}
            onAddPreset={addPreset}
            onUpdatePreset={updatePreset}
            onDeletePreset={deletePreset}
            onDuplicatePreset={duplicatePreset}
            onSwitchPreset={(a, id) => void handleSwitchPreset(a, id)}
            onSaveAssignment={(a) => void handleSaveAssignment(a)}
            lang={lang}
          />
        ) : tab !== "providers" ? (
          <AgentTabs
            agent={tab as "pi" | "opencode"}
            config={config ?? { version: 1, providers: {}, agents: {} }}
            agentsStatus={agentsStatus}
            agentDirty={agentDirty}
            saving={saving}
            applying={applying}
            applyResult={applyResult}
            agentSaveMsg={agentSaveMsg}
            onUpdateAssignment={updateAgentAssignment}
            onSaveAssignment={(a) => void handleSaveAssignment(a)}
            onApply={(a) => void handleApply(a)}
            lang={lang}
          />
        ) : config ? (
          <>
            {Object.keys(config.providers).length > 0 && (
              <div className="ml-toolbar">
                <button className="btn btn-primary" onClick={addProvider}>
                  <Icon name="plus" />
                  {t(lang, "mcAddProvider")}
                </button>
                <button className="btn btn-secondary" onClick={() => void handleImport()}>
                  {t(lang, "mcImportPi")}
                </button>
              </div>
            )}
            <ProviderGrid
              config={config}
              onSelect={setSelectedId}
              onAdd={addProvider}
              onImport={() => void handleImport()}
              onDelete={deleteProvider}
              onJumpToAgent={jumpToAgent}
              lang={lang}
            />
            {selectedId && selected && (
              <ProviderEditor
                providerId={selectedId}
                provider={selected}
                config={config}
                dirty={dirty}
                saving={saving}
                saveMsg={saveMsg}
                headersText={headersText}
                compatText={compatText}
                showAdvanced={showAdvanced}
                testState={testState}
                discover={discover}
                onClose={() => setSelectedId(null)}
                onPatchProvider={(patch) => updateProvider(selectedId, patch)}
                onPatchModel={(idx, patch) => updateModel(selectedId, idx, patch)}
                onAddModel={() => addModel(selectedId)}
                onDeleteModel={(idx) => deleteModel(selectedId, idx)}
                onUpdateCost={(idx, field, val) =>
                  updateCost(selectedId, idx, field, val)
                }
                onHeadersChange={setHeadersText}
                onCompatChange={setCompatText}
                onToggleAdvanced={() => setShowAdvanced((v) => !v)}
                onSave={() => void handleSave()}
                onTest={(modelId) => void handleTest(selectedId, modelId)}
                onResetTest={resetTest}
                onFetchModels={() => void handleFetchModels()}
                onDiscoverSet={setDiscover}
                onDiscoverAddSelected={handleDiscoverAddSelected}
                onJumpToAgent={jumpToAgent}
                onDeleteProvider={() => deleteProvider(selectedId)}
                lang={lang}
              />
            )}
          </>
        ) : null}
      </div>
    </div>
  );
}
