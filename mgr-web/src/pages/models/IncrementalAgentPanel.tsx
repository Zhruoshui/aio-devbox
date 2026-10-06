// IncrementalAgentPanel — the pi/opencode half of the sandbox model-config
// tab (task 10-04-model-config-cc-switch Phase 4, prd R2 增量式).
//
// Incremental agents keep ALL provider nodes in ONE native file
// (~/.pi/agent/models.json / ~/.config/opencode/opencode.jsonc); "switching"
// is just moving the default provider/model pointer. The panel edits the
// providers map + default selector; the whole-file source view is read-only
// (the merged file is derived at apply time by the app's renderer, so
// round-tripping it through the editor would fight the merge).
//
// Layout: default selector row → provider node list → node editor → save bar.

import { useState } from "react";
import { TemplatePickerModal } from "../../components/TemplatePickerModal";
import { Icon } from "../../icons";
import { t, type Lang } from "../../i18n";
import { API_PROTOCOLS } from "./types";
import {
  emptyProviderNode,
  type AgentId,
  type AgentStatus,
  type IncrementalAgentConfig,
  type IncrementalModelEntry,
  type ProviderNode,
} from "./sandboxTypes";

/** The native file the renderer writes (shown in the source view header). */
const NATIVE_FILE: Partial<Record<AgentId, string>> = {
  pi: "~/.pi/agent/models.json",
  opencode: "~/.config/opencode/opencode.jsonc",
};

/** opencode node shape wraps the base fields under an `options` blob. */
interface OpenCompat {
  npm?: string;
  name?: string;
  options?: { baseURL?: string; apiKey?: string };
}

interface Props {
  sandboxName: string;
  /** pi | opencode. */
  agent: AgentId;
  value: IncrementalAgentConfig | undefined;
  onChange: (next: IncrementalAgentConfig | undefined) => void;
  status: AgentStatus | undefined;
  onSaveConfig: () => Promise<string>;
  onApply: () => Promise<string>;
  busy: boolean;
  lang: Lang;
}

/** Stable id for a new node: slugged name or `provider-N`. */
function genNodeId(existing: Record<string, ProviderNode>, name: string): string {
  const slug = name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  let id = slug || `provider-${Object.keys(existing).length + 1}`;
  for (let n = 2; id in existing; n++) id = `${slug || "provider"}-${n}`;
  return id;
}

export function IncrementalAgentPanel({
  sandboxName,
  agent,
  value,
  onChange,
  status,
  onSaveConfig,
  onApply,
  busy,
  lang,
}: Props): JSX.Element {
  const providers = value?.providers ?? {};
  const ids = Object.keys(providers);
  // The working node copy (undefined = creating a new node).
  const [draft, setDraft] = useState<{ id: string; node: ProviderNode; isNew: boolean } | null>(
    null,
  );
  const [picking, setPicking] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);

  const live = status?.live as Record<string, unknown> | undefined;
  const modelsFlat = ids.flatMap((pid) =>
    (providers[pid]?.models ?? []).map((m) => `${pid} / ${m.id}`),
  );

  // ── config-level edits ────────────────────────────────────────────

  const patchConfig = (next: IncrementalAgentConfig | undefined) => onChange(next);

  const onSetDefault = (pair: string) => {
    if (!value) return;
    const [provider, model] = pair.split(" / ");
    patchConfig({ ...value, defaultProvider: provider, defaultModel: model });
  };

  const onAddNode = () => {
    setDraft({ id: "", node: emptyProviderNode(), isNew: true });
    setMsg(null);
  };

  const openEditor = (id: string) => {
    setDraft({ id, node: structuredClone(providers[id]), isNew: false });
    setMsg(null);
  };

  const commitDraft = () => {
    if (!draft) return;
    const id = draft.isNew
      ? genNodeId(providers, draft.node.name || draft.node.baseUrl)
      : draft.id;
    if (id === "") return;
    patchConfig({
      providers: { ...providers, [id]: draft.node },
      defaultProvider: value?.defaultProvider,
      defaultModel: value?.defaultModel,
    });
    setDraft(null);
  };

  const onDeleteNode = (id: string) => {
    const nextProviders = { ...providers };
    delete nextProviders[id];
    const nextIds = Object.keys(nextProviders);
    const defProvider = value?.defaultProvider;
    const defModel = value?.defaultModel;
    // Dangling default pointer: clear it (the app validates on apply).
    const defaultBroken =
      defProvider === id || (defProvider != null && !(defProvider in nextProviders));
    patchConfig(
      nextIds.length === 0 && !defaultBroken
        ? undefined
        : {
            providers: nextProviders,
            defaultProvider: defaultBroken ? undefined : defProvider,
            defaultModel: defaultBroken ? undefined : defModel,
          },
    );
    if (draft !== null && draft.id === id) setDraft(null);
  };

  // ── template picker (prefill a new/edited node's baseUrl/api/models) ──

  const onTemplatePicked = (tp: {
    id: string;
    name: string;
    baseUrl: string;
    api: string;
    models: { id: string; name?: string }[];
  }) => {
    setPicking(false);
    if (!draft) return;
    setDraft({
      ...draft,
      node: {
        ...draft.node,
        name: draft.node.name || tp.name,
        baseUrl: draft.node.baseUrl || tp.baseUrl,
        api: draft.node.api || tp.api,
        models:
          draft.node.models.length > 0
            ? draft.node.models
            : tp.models.map((m) => ({ id: m.id, name: m.name })),
      },
    });
  };

  // ── save + apply ──────────────────────────────────────────────────

  const onSave = async () => {
    setMsg(null);
    const err = await onSaveConfig();
    if (err !== "") {
      setMsg({ ok: false, text: err });
      return;
    }
    const applyErr = await onApply();
    setMsg(
      applyErr !== ""
        ? { ok: false, text: applyErr }
        : { ok: true, text: t(lang, "sbxApplied") },
    );
  };

  const dirtyDraft =
    draft !== null &&
    (draft.isNew ||
      JSON.stringify(draft.node) !== JSON.stringify(providers[draft.id] ?? null));

  return (
    <div className="sbx-panel">
      <div className="sbx-meta">
        {status && (
          <span className={`badge ${status.installed ? "badge-ok" : "badge-neutral"}`}>
            <span className="dot" />
            {status.installed ? t(lang, "sbxInstalled") : t(lang, "sbxNotInstalled")}
          </span>
        )}
        <span className="muted txs">
          {t(lang, "sbxApplyTarget")}
          <code className="mono">{NATIVE_FILE[agent]}</code>
        </span>
        {live && (
          <span className="muted txs">
            {t(lang, "sbxLive")}{" "}
            {agent === "pi"
              ? `${String((live.provider as string) ?? "—")} · ${String((live.model as string) ?? "—")}`
              : String((live.model as string) ?? "—")}
          </span>
        )}
      </div>

      {/* default selector */}
      {ids.length > 0 && (
        <div className="field-row">
          <div className="field">
            <label>{t(lang, "sbxDefaultPair")}</label>
            <select
              className="input"
              value={
                value?.defaultProvider && value?.defaultModel
                  ? `${value.defaultProvider} / ${value.defaultModel}`
                  : ""
              }
              onChange={(e) => onSetDefault(e.target.value)}
            >
              <option value="">{t(lang, "sbxNoDefault")}</option>
              {modelsFlat.map((pair) => (
                <option key={pair} value={pair}>
                  {pair}
                </option>
              ))}
            </select>
          </div>
        </div>
      )}

      {/* provider node list */}
      {ids.length === 0 ? (
        <p className="muted">{t(lang, "sbxNoProviders")}</p>
      ) : (
        <div className="sbx-cards">
          {ids.map((id) => {
            const node = providers[id];
            const isDefault = value?.defaultProvider === id;
            return (
              <div key={id} className={`sbx-card${isDefault ? " current" : ""}`}>
                <div className="sbx-card-head">
                  <span className="sbx-card-name">{node.name || id}</span>
                  {isDefault && (
                    <span className="badge badge-info">
                      {t(lang, "sbxDefaultBadge")}
                    </span>
                  )}
                </div>
                <span className="muted txs mono">{id}</span>
                <span className="muted txs mono">
                  {node.baseUrl} · {node.models.length}{" "}
                  {t(lang, "sbxModelsCount")}
                </span>
                <div className="sbx-card-actions">
                  <button className="btn btn-ghost btn-sm" onClick={() => openEditor(id)}>
                    <Icon name="edit" />
                    {t(lang, "mcEdit")}
                  </button>
                  <button
                    className="icon-btn danger"
                    aria-label={t(lang, "sbxDeleteProvider")}
                    onClick={() => onDeleteNode(id)}
                  >
                    <Icon name="trash" />
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}

      <div className="sbx-actions">
        <button className="btn btn-secondary btn-sm" onClick={onAddNode}>
          <Icon name="plus" />
          {t(lang, "mcAddProvider")}
        </button>
        <span className="sbx-spacer" />
        {msg && (
          <span className={`wizard-msg ${msg.ok ? "ok" : "err"}`}>{msg.text}</span>
        )}
        <button className="btn btn-primary" disabled={busy || ids.length === 0} onClick={() => void onSave()}>
          {busy ? t(lang, "mcSaving") : t(lang, "mcSave")}
        </button>
      </div>

      {/* node editor */}
      {draft && (
        <div className="sbx-editor">
          <div className="field-row-3">
            <div className="field">
              <label>{t(lang, "mcName")}</label>
              <input
                className="input"
                value={draft.node.name}
                placeholder={t(lang, "mcNamePlaceholder")}
                onChange={(e) =>
                  setDraft({ ...draft, node: { ...draft.node, name: e.target.value } })
                }
              />
            </div>
            <div className="field">
              <label>{t(lang, "mcBaseUrl")}</label>
              <input
                className="input mono"
                value={draft.node.baseUrl}
                placeholder="https://api.example.com/v1"
                onChange={(e) =>
                  setDraft({
                    ...draft,
                    node: { ...draft.node, baseUrl: e.target.value },
                  })
                }
              />
            </div>
            <div className="field">
              <label>{t(lang, "mcApi")}</label>
              <select
                className="input"
                value={draft.node.api}
                onChange={(e) =>
                  setDraft({ ...draft, node: { ...draft.node, api: e.target.value } })
                }
              >
                {API_PROTOCOLS.map((p) => (
                  <option key={p} value={p}>
                    {p}
                  </option>
                ))}
              </select>
            </div>
          </div>

          <div className="field">
            <label>{t(lang, "mcApiKey")}</label>
            <input
              className="input mono"
              type="password"
              value={draft.node.apiKey ?? ""}
              autoComplete="off"
              onChange={(e) =>
                setDraft({
                  ...draft,
                  node: { ...draft.node, apiKey: e.target.value || undefined },
                })
              }
            />
          </div>

          {agent === "opencode" && (
            <div className="field">
              <label>{t(lang, "sbxOpencodeCompat")}</label>
              <textarea
                className="input mono"
                rows={3}
                spellCheck={false}
                placeholder='{ "npm": "@ai-sdk/openai-compatible", "options": { "baseURL": "..." } }'
                value={compatText(draft.node.compat)}
                onChange={(e) => {
                  try {
                    const v: unknown = JSON.parse(e.target.value || "null");
                    setDraft({
                      ...draft,
                      node: { ...draft.node, compat: v ?? undefined },
                    });
                  } catch {
                    // keep typing; invalid JSON is not committed
                  }
                }}
              />
            </div>
          )}

          {/* models */}
          <span className="ml-section-title">{t(lang, "mcModels")}</span>
          {draft.node.models.map((m, idx) => (
            <ModelLine
              key={idx}
              model={m}
              onPatch={(patch) => {
                const models = draft.node.models.slice();
                models[idx] = { ...models[idx], ...patch };
                setDraft({ ...draft, node: { ...draft.node, models } });
              }}
              onDelete={() => {
                const models = draft.node.models.filter((_, i) => i !== idx);
                setDraft({ ...draft, node: { ...draft.node, models } });
              }}
              lang={lang}
            />
          ))}
          <button
            className="btn btn-ghost btn-sm"
            onClick={() =>
              setDraft({
                ...draft,
                node: { ...draft.node, models: [...draft.node.models, { id: "" }] },
              })
            }
          >
            <Icon name="plus" />
            {t(lang, "mcAddModel")}
          </button>

          <div className="dialog-actions">
            <button className="btn btn-secondary btn-sm" onClick={() => setPicking(true)}>
              <Icon name="box" />
              {t(lang, "sbxFromTemplate")}
            </button>
            <span className="sbx-spacer" />
            <button className="btn btn-secondary" onClick={() => setDraft(null)}>
              {t(lang, "cancel")}
            </button>
            <button
              className="btn btn-primary"
              disabled={!dirtyDraft || draft.node.baseUrl.trim() === ""}
              onClick={commitDraft}
            >
              {t(lang, "sbxUseSnapshot")}
            </button>
          </div>
        </div>
      )}

      {/* read-only whole-file source view */}
      <div className="sbx-source">
        <div className="sbx-source-head">
          <span className="muted txs">
            {t(lang, "sbxWholeFile")}
            <code className="mono">{NATIVE_FILE[agent]}</code>
            {sandboxName ? ` · ${sandboxName}` : ""}
          </span>
        </div>
        <pre className="sce-pre">{mergedPreview(agent, value)}</pre>
      </div>

      {picking && (
        <TemplatePickerModal
          onSelect={onTemplatePicked}
          onClose={() => setPicking(false)}
          lang={lang}
        />
      )}
    </div>
  );
}

/** One model row inside the node editor. */
function ModelLine({
  model,
  onPatch,
  onDelete,
  lang,
}: {
  model: IncrementalModelEntry;
  onPatch: (patch: Partial<IncrementalModelEntry>) => void;
  onDelete: () => void;
  lang: Lang;
}): JSX.Element {
  return (
    <div className="sbx-model-line">
      <input
        className="input mono"
        value={model.id}
        placeholder={t(lang, "mcModelId")}
        onChange={(e) => onPatch({ id: e.target.value })}
      />
      <input
        className="input"
        value={model.name ?? ""}
        placeholder={t(lang, "mcModelName")}
        onChange={(e) => onPatch({ name: e.target.value || undefined })}
      />
      <button className="icon-btn danger" aria-label={t(lang, "mcDeleteProvider")} onClick={onDelete}>
        <Icon name="trash" />
      </button>
    </div>
  );
}

function compatText(v: unknown): string {
  if (v == null) return "";
  try {
    return JSON.stringify(v, null, 2);
  } catch {
    return "";
  }
}

/** What the renderer will write: the merged native file, pretty-printed. */
function mergedPreview(agent: AgentId, value: IncrementalAgentConfig | undefined): string {
  if (!value || Object.keys(value.providers).length === 0) {
    return "// " + (agent === "pi" ? "models.json" : "opencode.jsonc");
  }
  if (agent === "pi") {
    return JSON.stringify(
      {
        providers: value.providers,
        ...(value.defaultProvider ? { defaultProvider: value.defaultProvider } : {}),
        ...(value.defaultModel ? { defaultModel: value.defaultModel } : {}),
      },
      null,
      2,
    );
  }
  // opencode: providers under `provider.<id>.options.baseURL` + top model.
  const provider: Record<string, unknown> = {};
  for (const [id, node] of Object.entries(value.providers)) {
    const compat = (node.compat ?? {}) as OpenCompat;
    provider[id] = {
      npm: compat.npm ?? "@ai-sdk/openai-compatible",
      name: node.name || id,
      options: {
        ...(compat.options ?? {}),
        baseURL: compat.options?.baseURL ?? node.baseUrl,
        apiKey: compat.options?.apiKey ?? node.apiKey ?? "",
      },
      models: Object.fromEntries(node.models.map((m) => [m.id, { name: m.name ?? m.id }])),
    };
  }
  return JSON.stringify(
    {
      ...(value.defaultProvider ? { model: `${value.defaultProvider}/${value.defaultModel ?? ""}` } : {}),
      provider,
    },
    null,
    2,
  );
}
