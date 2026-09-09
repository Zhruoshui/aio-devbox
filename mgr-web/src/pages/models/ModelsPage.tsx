// ModelsPage — the mgr model-config page, ported from the workbench's
// ModelsPane (web/src/panes/models/ModelsPane.tsx, Phase 4c).
//
// Owns ALL state and /api/models/* handlers for this page and delegates each
// tab's render to a focused sub-component (same split as the workbench):
//   providers → ProviderGrid (card grid) + ProviderEditor drawer
//   pi/opencode → AgentTabs (assignment editor; live parts trimmed)
//   claude/codex → PresetList (preset CRUD; apply trimmed)
//
// Differences vs the workbench pane (mgr has no sandbox-local agent APIs):
//   - no usage tab (usage is its own page over GET /api/usage);
//   - no GET /api/models/agents, no apply, no live provider management;
//   - "switch preset" = setCurrent + save in one click — the agent render
//     happens sandbox-side when the sandbox pulls the config;
//   - lang arrives as a prop (App owns it) instead of localStorage.
//
// API contract: GET/PUT /api/models/config + POST /api/models/import/pi +
// POST /api/models/discover + POST /api/models/test + GET /api/models/catalog
// (mgr/src/models.rs). Responses decode once in ./types (or arrive as typed
// api.ts results); all rendering consumes the typed CanonicalConfig.

import { useCallback, useEffect, useState } from "react";
import {
  discoverModels,
  getModelsCatalog,
  getModelsConfig,
  importPiModels,
  listSandboxes,
  putModelsConfig,
  testModel,
} from "../../api";
import { t, type Lang } from "../../i18n";
import { Icon } from "../../icons";
import { AgentTabs } from "./AgentTabs";
import type { SandboxLink } from "./MgrNotice";
import { PresetList } from "./PresetList";
import { ProviderEditor } from "./ProviderEditor";
import { ProviderGrid } from "./ProviderGrid";
import type { CatalogFillState } from "./ModelRow";
import {
  catalogRecommend,
  decodeConfig,
  emptyProvider,
  genProviderId,
  deriveProviderIdFromName,
  rebindAgentProviders,
  safeStringify,
  type AgentTab,
  type AnyPreset,
  type CanonicalConfig,
  type CatalogResponse,
  type CostEntry,
  type DiscoverState,
  type ModelEntry,
  type PresetAgent,
  type ProviderEntry,
  type PutResponse,
  type TestStateMap,
} from "./types";

type TabKey = "providers" | "pi" | "opencode" | "claude" | "codex";

const TAB_KEYS: TabKey[] = ["providers", "pi", "opencode", "claude", "codex"];

function tabLabel(lang: Lang, key: TabKey): string {
  switch (key) {
    case "providers":
      return t(lang, "mcProviders");
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

export function ModelsPage({ lang }: { lang: Lang }): JSX.Element {
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

  // Per-(provider,model) test pills + discover modal state.
  const [testState, setTestState] = useState<TestStateMap>({});
  const [discover, setDiscover] = useState<DiscoverState | null>(null);

  // models.dev catalog (lazy-fetched once, kept in memory for the page's
  // lifetime — the catalog changes rarely, no refresh button needed).
  const [catalog, setCatalog] = useState<CatalogResponse | null>(null);
  const [catalogFillState, setCatalogFillState] = useState<
    Record<string, CatalogFillState>
  >({});

  // Agent-tab shared state: which agent has unsaved canonical edits + the
  // last save message (shown in that tab's save bar).
  const [agentDirty, setAgentDirty] = useState<Set<string>>(new Set());
  const [agentSaveMsg, setAgentSaveMsg] = useState<{
    ok: boolean;
    text: string;
  } | null>(null);

  // Sandbox workbench links for the agent tabs' MgrNotice (entry_url per
  // running sandbox — that is where the live agent view lives).
  const [sandboxLinks, setSandboxLinks] = useState<SandboxLink[] | null>(null);

  // ── config fetch / save ─────────────────────────────────────────

  const fetchConfig = useCallback(async (): Promise<void> => {
    try {
      const cfg = decodeConfig(await getModelsConfig());
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

  // Sandbox links refresh whenever an agent tab is shown (cheap list call;
  // running state changes as sandboxes start/stop).
  useEffect(() => {
    if (tab === "providers") return;
    let cancelled = false;
    listSandboxes()
      .then((r) => {
        if (cancelled) return;
        setSandboxLinks(
          r.sandboxes
            .filter((s) => s.live === "running")
            .map((s) => ({ name: s.name, entryUrl: s.entry_url })),
        );
      })
      .catch(() => {
        /* links are advisory — keep whatever we had */
      });
    return () => {
      cancelled = true;
    };
  }, [tab]);

  // Sync the advanced JSON textareas when the selected provider changes.
  useEffect(() => {
    if (!config || !selectedId) return;
    const p = config.providers[selectedId];
    if (!p) return;
    setHeadersText(safeStringify(p.headers));
    setCompatText(safeStringify(p.compat));
  }, [selectedId, config]);

  // Reset all test pills for the selected provider when its identifying
  // fields change (a stale pill would mislead).
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
      // While the key is still the auto `provider-N` placeholder, derive the
      // id from the display name: pi/pi-web display the provider ID (never
      // the node name), so the id must carry the name for it to show up
      // anywhere (08-28-provider-id-from-name). Custom ids are never touched.
      const derived =
        patch.name !== undefined
          ? deriveProviderIdFromName(patch.name, config?.providers ?? {}, id)
          : "";
      const renamed = derived && derived !== id ? { from: id, to: derived } : null;
      if (renamed) {
        setSelectedId((prev) => (prev === renamed.from ? renamed.to : prev));
      }
      setConfig((prev) => {
        if (!prev) return prev;
        const old = prev.providers[id];
        if (!old) return prev;
        const entry = { ...old, ...patch };
        if (!renamed) {
          return {
            ...prev,
            providers: { ...prev.providers, [id]: entry },
          };
        }
        // Re-key + rebind agent references. If the derived key collided with
        // a provider created since (rare typing race), keep the old key —
        // the next keystroke re-derives.
        if (renamed.to !== id && prev.providers[renamed.to]) {
          return { ...prev, providers: { ...prev.providers, [id]: entry } };
        }
        const { [id]: _, ...rest } = prev.providers;
        void _;
        return {
          ...prev,
          providers: { ...rest, [renamed.to]: entry },
          agents: rebindAgentProviders(prev.agents, renamed.from, renamed.to),
        };
      });
      setDirty(true);
    },
    [config],
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

      const resp: PutResponse = await putModelsConfig(body);
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
      const resp = await importPiModels();
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

  // ── test + discover ─────────────────────────────────────────────

  const handleTest = useCallback(
    async (providerId: string, modelId: string): Promise<void> => {
      if (!modelId) return;
      const key = `${providerId}:${modelId}`;
      setTestState((prev) => ({ ...prev, [key]: { status: "testing" } }));
      const t0 = performance.now();
      try {
        const resp = await testModel(providerId, modelId);
        setTestState((prev) => ({
          ...prev,
          [key]: {
            status: resp.ok ? "ok" : "fail",
            // Fall back to the client-measured round trip when the backend
            // omits latencyMs so the pill still shows a real number.
            latencyMs: resp.latencyMs ?? Math.round(performance.now() - t0),
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
      const resp = await discoverModels(
        apiKeyDirty(provider)
          ? { baseUrl: provider.baseUrl, api: provider.api, apiKey: provider.apiKey }
          : { providerId: selectedId },
      );
      setDiscover({
        loading: false,
        error: "",
        endpoint: resp.endpoint,
        models: resp.models,
        filter: "",
        selected: new Set(),
      });
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setDiscover((d) => (d ? { ...d, loading: false, error: msg } : d));
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

  // Lazy-fetch the models.dev catalog once and cache it in state; subsequent
  // fill clicks reuse it without another request (same policy as the
  // workbench: never fetched until a fill button is clicked).
  const fetchCatalogOnce = useCallback(async (): Promise<CatalogResponse | null> => {
    if (catalog) return catalog;
    try {
      const c = await getModelsCatalog();
      setCatalog(c);
      return c;
    } catch {
      return null;
    }
  }, [catalog]);

  const handleCatalogFill = useCallback(
    async (providerId: string, idx: number): Promise<void> => {
      const provider = config?.providers[providerId];
      const model = provider?.models[idx];
      if (!provider || !model || !model.id) return;
      const key = `${providerId}:${model.id}`;
      setCatalogFillState((prev) => ({ ...prev, [key]: "loading" }));
      const c = await fetchCatalogOnce();
      if (!c) {
        setCatalogFillState((prev) => ({ ...prev, [key]: "error" }));
        return;
      }
      const hit = catalogRecommend(c, provider.baseUrl, model.id);
      if (!hit) {
        setCatalogFillState((prev) => ({ ...prev, [key]: "notfound" }));
        return;
      }
      setCatalogFillState((prev) => {
        const next = { ...prev };
        delete next[key];
        return next;
      });
      updateModel(providerId, idx, {
        name: hit.name,
        reasoning: hit.reasoning,
        contextWindow: hit.contextWindow,
        maxTokens: hit.maxTokens,
        cost: hit.cost,
      });
    },
    [config, fetchCatalogOnce, updateModel],
  );

  // ── agent assignments + presets (canonical edits only) ───────────

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
    },
    [],
  );

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
    },
    [],
  );

  const addPreset = useCallback(
    (agent: PresetAgent, preset: AnyPreset): void => {
      setPresets(agent, (block) => {
        const presets = [...(block?.presets ?? []), preset];
        // First preset auto-becomes current (sent as "" until the backend
        // backfills the id). Otherwise leave current alone.
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
        // (or null). Never dangle.
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
        // after the source so it appears adjacent.
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

  /** Switch = setCurrent + save, one click (the sandbox renders on pull). */
  const handleSwitchPreset = useCallback(
    async (agent: PresetAgent, id: string): Promise<void> => {
      if (!config || id === "") return;
      const block = agent === "claude" ? config.agents.claude : config.agents.codex;
      if (id === (block?.current ?? null)) return; // already current
      const next = {
        ...config,
        agents: { ...config.agents, [agent]: { ...block, current: id } },
      };
      setConfig(next);
      setSaving(true);
      setAgentSaveMsg(null);
      try {
        await putModelsConfig(next);
        await fetchConfig();
        setAgentDirty((prev) => {
          const n = new Set(prev);
          n.delete(agent);
          return n;
        });
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
    [config, fetchConfig],
  );

  const handleSaveAssignment = useCallback(
    async (agent: AgentTab): Promise<void> => {
      if (!config) return;
      setSaving(true);
      setAgentSaveMsg(null);
      try {
        await putModelsConfig(config);
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

  // ── render ──────────────────────────────────────────────────────

  const selected = config && selectedId ? config.providers[selectedId] : null;

  const jumpToAgent = (agent: AgentTab): void => {
    setSelectedId(null);
    setTab(agent);
  };

  return (
    <div className="page models-page">
      <div className="page-head">
        <h1>{t(lang, "navModels")}</h1>
        <p className="sub">{t(lang, "modelsSub")}</p>
      </div>

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

      {loading ? (
        <div className="ml-loading">{t(lang, "mcLoading")}</div>
      ) : loadError ? (
        <div className="ml-error">{t(lang, "mcLoadFailed") + loadError}</div>
      ) : tab === "claude" || tab === "codex" ? (
        <PresetList
          agent={tab}
          config={config ?? { version: 1, providers: {}, agents: {} }}
          agentDirty={agentDirty}
          saving={saving}
          agentSaveMsg={agentSaveMsg}
          sandboxLinks={sandboxLinks ?? []}
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
          agentDirty={agentDirty}
          saving={saving}
          agentSaveMsg={agentSaveMsg}
          sandboxLinks={sandboxLinks ?? []}
          onUpdateAssignment={updateAgentAssignment}
          onSaveAssignment={(a) => void handleSaveAssignment(a)}
          lang={lang}
        />
      ) : config ? (
        <>
          <div className="ml-sec-head">
            <div>
              <h2>{t(lang, "mcProviders")}</h2>
              <p>{t(lang, "mcProvidersSub")}</p>
            </div>
            <div className="ml-sec-actions">
              <button className="btn btn-secondary" onClick={() => void handleImport()}>
                {t(lang, "mcImportPi")}
              </button>
              <button className="btn btn-primary" onClick={addProvider}>
                <Icon name="plus" />
                {t(lang, "mcAddProvider")}
              </button>
            </div>
          </div>
          <ProviderGrid
            config={config}
            onSelect={setSelectedId}
            onAdd={addProvider}
            onImport={() => void handleImport()}
            onDelete={deleteProvider}
            onJumpToAgent={jumpToAgent}
            lang={lang}
          />
        </>
      ) : null}

      {/* The drawer + scrim are viewport-fixed (see ProviderEditor); rendered
       * as a sibling of the tab content so they cover the whole page. */}
      {tab === "providers" && config && selectedId && selected && (
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
          catalogFillState={catalogFillState}
          onClose={() => setSelectedId(null)}
          onPatchProvider={(patch) => updateProvider(selectedId, patch)}
          onPatchModel={(idx, patch) => updateModel(selectedId, idx, patch)}
          onAddModel={() => addModel(selectedId)}
          onDeleteModel={(idx) => deleteModel(selectedId, idx)}
          onUpdateCost={(idx, field, val) => updateCost(selectedId, idx, field, val)}
          onHeadersChange={setHeadersText}
          onCompatChange={setCompatText}
          onToggleAdvanced={() => setShowAdvanced((v) => !v)}
          onSave={() => void handleSave()}
          onTest={(modelId) => void handleTest(selectedId, modelId)}
          onResetTest={resetTest}
          onFetchModels={() => void handleFetchModels()}
          onDiscoverSet={setDiscover}
          onDiscoverAddSelected={handleDiscoverAddSelected}
          onFillFromCatalog={(idx) => void handleCatalogFill(selectedId, idx)}
          onJumpToAgent={jumpToAgent}
          onDeleteProvider={() => deleteProvider(selectedId)}
          lang={lang}
        />
      )}
    </div>
  );
}
