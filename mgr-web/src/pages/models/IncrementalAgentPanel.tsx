// IncrementalAgentPanel — the pi/opencode half of the sandbox model-config
// tab (task 10-04-model-config-cc-switch Phase 4, prd R2 增量式).
//
// Incremental agents keep ALL provider nodes in the native file(s):
//   pi       ~/.pi/agent/models.json (providers) + settings.json (defaults)
//   opencode ~/.config/opencode/opencode.jsonc (provider + model keys)
// "Switching" is just moving the default provider/model pointer.
//
// The panel edits the providers map + default selector, OR the whole file
// source (10-13-models-config-ux R1/R6): the file text is rendered from the
// managed subtree and parsed BACK into the config on save (single source of
// truth — apply/renderers unchanged). The two editors are mutually
// exclusive to avoid double-entry conflicts.
//
// Layout: default selector row → provider node list → node editor →
// whole-file source view/editor → save bar.

import { useState } from "react";
import { DiscoverModelsDialog } from "../../components/DiscoverModelsDialog";
import { SourceCodeEditor } from "../../components/SourceCodeEditor";
import { TemplatePickerModal } from "../../components/TemplatePickerModal";
import { discoverModels } from "../../api";
import { Icon } from "../../icons";
import { t, type Lang } from "../../i18n";
import { API_PROTOCOLS, protocolBadge, type DiscoverState } from "./types";
import {
  emptyProviderNode,
  type AgentId,
  type AgentStatus,
  type IncrementalAgentConfig,
  type IncrementalModelEntry,
  type ProviderNode,
} from "./sandboxTypes";
import {
  opencodeFileText,
  parseIncrementalFiles,
  piModelsFileText,
  piSettingsFileText,
} from "./sourceFiles";

/** The native file the renderer writes (shown in the source view header). */
const NATIVE_FILE: Partial<Record<AgentId, string>> = {
  pi: "~/.pi/agent/models.json",
  opencode: "~/.config/opencode/opencode.jsonc",
};

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
  // Fetch-models multi-select dialog state (R5; open from the node editor).
  const [discover, setDiscover] = useState<DiscoverState | null>(null);
  // Whole-file source editing (mutually exclusive with the node editor).
  const [sourceEdit, setSourceEdit] = useState(false);
  const [srcMain, setSrcMain] = useState("");
  const [srcSettings, setSrcSettings] = useState(""); // pi settings.json
  const [srcErr, setSrcErr] = useState("");

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
    setSourceEdit(false);
    setMsg(null);
  };

  const openEditor = (id: string) => {
    setDraft({ id, node: structuredClone(providers[id]), isNew: false });
    setSourceEdit(false);
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

  // ── whole-file source editing (R1/R6) ─────────────────────────────

  const isPi = agent === "pi";

  const openSourceEdit = () => {
    // Entering source editing closes the node editor (mutual exclusion).
    setDraft(null);
    setSrcMain(isPi ? piModelsFileText(value) : opencodeFileText(value));
    if (isPi) setSrcSettings(piSettingsFileText(value));
    setSrcErr("");
    setSourceEdit(true);
  };

  const onSourceSave = () => {
    const r = parseIncrementalFiles(agent, srcMain, isPi ? srcSettings : undefined);
    if (r.error !== undefined || r.config === undefined) {
      setSrcErr(r.error ?? "unknown error");
      return;
    }
    patchConfig(r.config);
    setSrcErr("");
    setSourceEdit(false);
  };

  // Honest save-disabled state: the shared save commits BOTH files, so it
  // is disabled whenever the combined parse fails (not just this editor's
  // own JSON validity).
  const srcParse = parseIncrementalFiles(agent, srcMain, isPi ? srcSettings : undefined);
  const srcSaveDisabled = srcParse.error !== undefined;

  // ── fetch models (R5: discover via mgr's literal probe, multi-select
  //    the results into the node's model list) ────────────────────────

  const onFetchModels = async () => {
    if (!draft) return;
    setDiscover({
      loading: true,
      error: "",
      endpoint: "",
      models: [],
      filter: "",
      selected: new Set(),
    });
    try {
      const body: { baseUrl: string; api: string; apiKey?: string } = {
        baseUrl: draft.node.baseUrl,
        api: draft.node.api,
      };
      if (draft.node.apiKey) body.apiKey = draft.node.apiKey;
      const resp = await discoverModels(body);
      setDiscover({
        loading: false,
        error: "",
        endpoint: resp.endpoint,
        models: resp.models,
        filter: "",
        selected: new Set(),
      });
    } catch (e) {
      setDiscover({
        loading: false,
        error: String(e),
        endpoint: "",
        models: [],
        filter: "",
        selected: new Set(),
      });
    }
  };

  const onDiscoverAddSelected = () => {
    if (!draft || !discover) return;
    const existing = new Set(draft.node.models.map((m) => m.id));
    const additions = [...discover.selected]
      .filter((id) => !existing.has(id))
      .map((id) => {
        const d = discover.models.find((m) => m.id === id);
        return { id, name: d?.name };
      });
    setDraft({
      ...draft,
      node: { ...draft.node, models: [...draft.node.models, ...additions] },
    });
    setDiscover(null);
  };

  // ── template picker (prefill a new/edited node's baseUrl/api/models) ──

  const onTemplatePicked = (tp: {
    id: string;
    name: string;
    baseUrl: string;
    api: string;
    apiKey?: string;
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
        // R2: prefill the key only when the node has none yet.
        apiKey: draft.node.apiKey || tp.apiKey,
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

      {/* provider node list — row layout (R3): one config item per line so a
       *  long baseUrl truncates (title tooltip) instead of overflowing. */}
      {ids.length === 0 ? (
        <p className="muted">{t(lang, "sbxNoProviders")}</p>
      ) : (
        <div className="sbx-list">
          {ids.map((id) => {
            const node = providers[id];
            const isDefault = value?.defaultProvider === id;
            return (
              <div key={id} className={`sbx-row${isDefault ? " current" : ""}`}>
                <div className="sbx-row-main">
                  <div className="sbx-row-title">
                    <span className="sbx-row-name">{node.name || id}</span>
                    {isDefault && (
                      <span className="badge badge-info">
                        {t(lang, "sbxDefaultBadge")}
                      </span>
                    )}
                  </div>
                  <span className="muted txs mono sbx-row-id">{id}</span>
                  <span
                    className="muted txs mono sbx-row-url"
                    title={node.baseUrl}
                  >
                    {node.baseUrl || "—"}
                  </span>
                  <span className="muted txs">
                    <span className="badge badge-neutral">{protocolBadge(node.api)}</span>
                    {" · "}
                    {node.models.length} {t(lang, "sbxModelsCount")}
                  </span>
                </div>
                <div className="sbx-row-actions">
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
          <div className="sbx-actions">
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
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              onClick={() => void onFetchModels()}
              disabled={draft.node.baseUrl.trim() === ""}
              title={
                draft.node.baseUrl.trim() === ""
                  ? t(lang, "mcNeedBaseUrl")
                  : undefined
              }
            >
              <Icon name="refresh" />
              {t(lang, "mcFetchModels")}
            </button>
          </div>

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

      {/* whole-file source view/editor (hidden while the node editor is
       *  open — the two editors are mutually exclusive) */}
      {!draft && (
        <div className="sbx-source">
          <div className="sbx-source-head">
            <span className="muted txs">
              {t(lang, "sbxWholeFile")}
              {sandboxName ? ` · ${sandboxName}` : ""}
            </span>
            {!sourceEdit && (
              <button
                className="btn btn-ghost btn-sm"
                onClick={openSourceEdit}
              >
                <Icon name="edit" />
                {t(lang, "sbxEditWholeFile")}
              </button>
            )}
          </div>
          {sourceEdit && (
            <p className="muted txs">{t(lang, "sbxSourceManagedHint")}</p>
          )}
          {srcErr !== "" && (
            <p className="ml-msg err">
              {t(lang, "sbxSourceParseError")}
              {srcErr}
            </p>
          )}
          {sourceEdit ? (
            <>
              <div className="sbx-file-label mono">{NATIVE_FILE[agent]}</div>
              <SourceCodeEditor
                text={srcMain}
                language={isPi ? "json" : "jsonc"}
                editMode
                onChange={setSrcMain}
                onSave={onSourceSave}
                onCancel={() => setSourceEdit(false)}
                saveDisabled={srcSaveDisabled}
                lang={lang}
              />
              {isPi && (
                <div className="sbx-source-extra">
                  <div className="sbx-file-label mono">
                    ~/.pi/agent/settings.json
                  </div>
                  <SourceCodeEditor
                    text={srcSettings}
                    language="json"
                    editMode
                    onChange={setSrcSettings}
                    onSave={onSourceSave}
                    onCancel={() => setSourceEdit(false)}
                    saveDisabled={srcSaveDisabled}
                    lang={lang}
                  />
                </div>
              )}
            </>
          ) : (
            <>
              <div className="sbx-file-label mono">{NATIVE_FILE[agent]}</div>
              <SourceCodeEditor
                text={isPi ? piModelsFileText(value) : opencodeFileText(value)}
                language={isPi ? "json" : "jsonc"}
                editMode={false}
                lang={lang}
              />
              {isPi && (
                <div className="sbx-source-extra">
                  <div className="sbx-file-label mono">
                    ~/.pi/agent/settings.json
                  </div>
                  <SourceCodeEditor
                    text={piSettingsFileText(value)}
                    language="json"
                    editMode={false}
                    lang={lang}
                  />
                </div>
              )}
            </>
          )}
        </div>
      )}

      {picking && (
        <TemplatePickerModal
          onSelect={onTemplatePicked}
          onClose={() => setPicking(false)}
          lang={lang}
        />
      )}

      {discover && (
        <DiscoverModelsDialog
          discover={discover}
          onSet={setDiscover}
          onConfirm={onDiscoverAddSelected}
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

