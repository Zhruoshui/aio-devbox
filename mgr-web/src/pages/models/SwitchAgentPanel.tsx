// SwitchAgentPanel — the claude/codex half of the sandbox model-config tab
// (task 10-04-model-config-cc-switch Phase 4, prd R2 切换式).
//
// Switch-style agents keep a list of named provider SNAPSHOTS (cc-switch
// style); exactly one is `current` and gets rendered to the agent's native
// config files on save/apply. Each snapshot's settings_config is opaque:
// for claude it IS the ~/.claude/settings.json object (env block); for codex
// it is `{ config: "<config.toml string>", auth: {...} }` — the source
// editor edits BOTH files (config.toml + auth.json, 10-13 R6 cc-switch
// alignment).
//
// Layout: provider card list (current badge + 设为当前/编辑/源码/删除) →
// save bar → source view/editor. Editing a snapshot opens a small inline
// form (name + the agent's well-known fields) OR the raw source editor —
// both write back into the draft. The template picker (mgr's provider
// library, prd R3) prefills a new/edited snapshot from a library entry.

import { useState } from "react";
import { SourceCodeEditor } from "../../components/SourceCodeEditor";
import { TemplatePickerModal } from "../../components/TemplatePickerModal";
import { Icon } from "../../icons";
import { t, type Lang } from "../../i18n";
import {
  emptyClaudeSnapshot,
  emptyCodexSnapshot,
  snapshotToSource,
  sourceToSnapshotSettings,
  type AgentStatus,
  type AgentId,
  type ProviderSnapshot,
  type SwitchAgentConfig,
} from "./sandboxTypes";
import { codexSourceParts, parseCodexSourceParts } from "./sourceFiles";

/** claude's well-known settings.json env keys (edit form). */
const CLAUDE_ENV_FIELDS = [
  ["ANTHROPIC_BASE_URL", "mcBaseUrl"],
  ["ANTHROPIC_AUTH_TOKEN", "mcApiKey"],
  ["ANTHROPIC_MODEL", "mcModel"],
  ["ANTHROPIC_SMALL_FAST_MODEL", "sbxHaikuModel"],
] as const;

interface Props {
  sandboxName: string;
  /** claude | codex — drives the defaults/env-field shape. */
  agent: AgentId;
  /** The draft block being edited (undefined = agent not configured yet). */
  value: SwitchAgentConfig | undefined;
  onChange: (next: SwitchAgentConfig | undefined) => void;
  status: AgentStatus | undefined;
  /** PUT the draft config to the sandbox app. Returns "" on success. */
  onSaveConfig: () => Promise<string>;
  /** POST apply/:agent after a successful save. */
  onApply: () => Promise<string>;
  /** null while a save/apply round-trip is in flight. */
  busy: boolean;
  lang: Lang;
}

/** Stable id for a new snapshot: `<agent>-N` not colliding with existing. */
function genSnapshotId(agent: AgentId, existing: ProviderSnapshot[]): string {
  let n = 1;
  while (existing.some((s) => s.id === `${agent}-${n}`)) n++;
  return `${agent}-${n}`;
}

/** The snapshot's base URL for the row list (claude: env; codex: first
 *  base_url line of the toml). "—" when not set. */
function snapshotBaseUrl(agent: AgentId, snap: ProviderSnapshot): string {
  if (agent === "claude") {
    const env = snap.settingsConfig.env as Record<string, unknown> | undefined;
    const u = env?.ANTHROPIC_BASE_URL;
    return typeof u === "string" && u !== "" ? u : "—";
  }
  const cfg = snap.settingsConfig.config;
  if (typeof cfg === "string") {
    const m = cfg.match(/^\s*base_url\s*=\s*"([^"]+)"/m);
    if (m) return m[1];
  }
  return "—";
}

/** Default fields when a template prefills a NEW claude snapshot. */
function claudeDefaultsFromTemplate(tp: {
  name: string;
  baseUrl: string;
  api: string;
  apiKey?: string;
  models: { id: string }[];
}): Record<string, unknown> {
  const env: Record<string, unknown> = { ANTHROPIC_BASE_URL: tp.baseUrl };
  if (tp.models[0]) env.ANTHROPIC_MODEL = tp.models[0].id;
  // R2: carry the library key so the user doesn't re-paste it.
  if (tp.apiKey) env.ANTHROPIC_AUTH_TOKEN = tp.apiKey;
  return { env };
}

/** Default config.toml when a template prefills a NEW codex snapshot. */
function codexTomlFromTemplate(tp: {
  id: string;
  name: string;
  baseUrl: string;
  api: string;
  models: { id: string }[];
}): string {
  const isResponses = tp.api === "openai-responses";
  const lines = [
    `model = "${tp.models[0]?.id ?? ""}"`,
    `model_provider = "${tp.id}"`,
    "",
    `[model_providers.${tp.id}]`,
    `name = "${tp.name || tp.id}"`,
    `base_url = "${tp.baseUrl}"`,
    `wire_api = "${isResponses ? "responses" : "chat"}"`,
    "",
  ];
  return lines.join("\n");
}

export function SwitchAgentPanel({
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
  const snapshots = value?.providers ?? [];
  const current = value?.current;

  // Snapshot editor state: which snapshot is being edited + the working copy.
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState<ProviderSnapshot | null>(null);
  // Source view/editor state. For codex `sourceText` is config.toml and
  // `sourceAuthText` is auth.json (the snapshot's two files).
  const [sourceId, setSourceId] = useState<string | null>(null);
  const [sourceEdit, setSourceEdit] = useState(false);
  const [sourceText, setSourceText] = useState("");
  const [sourceAuthText, setSourceAuthText] = useState("");
  // Template picker target: "new" | snapshot id (prefill the editor draft).
  const [picking, setPicking] = useState<string | null>(null);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);

  const live = status?.live as Record<string, unknown> | undefined;
  const applyTarget =
    agent === "claude"
      ? "~/.claude/settings.json"
      : "~/.codex/config.toml + ~/.codex/auth.json";

  // ── snapshot CRUD (all on the DRAFT config; persisted via onSaveConfig) ──

  const patchDraftConfig = (next: SwitchAgentConfig | undefined) => onChange(next);

  const openEditor = (snap: ProviderSnapshot) => {
    setDraft(structuredClone(snap));
    setEditingId(snap.id);
    setSourceEdit(false);
    setSourceId(null);
    setMsg(null);
  };

  const closeEditor = () => {
    setEditingId(null);
    setDraft(null);
  };

  const onAddSnapshot = (fromTemplate?: {
    id: string;
    name: string;
    baseUrl: string;
    api: string;
    apiKey?: string;
    models: { id: string }[];
  }) => {
    const id = genSnapshotId(agent, snapshots);
    const snap =
      agent === "claude" ? emptyClaudeSnapshot(id) : emptyCodexSnapshot(id);
    if (fromTemplate) {
      snap.name = fromTemplate.name;
      snap.settingsConfig =
        agent === "claude"
          ? claudeDefaultsFromTemplate(fromTemplate)
          : {
              ...snap.settingsConfig,
              config: codexTomlFromTemplate(fromTemplate),
              // R2: codex's key lives in auth.json's OPENAI_API_KEY.
              auth: fromTemplate.apiKey ? { OPENAI_API_KEY: fromTemplate.apiKey } : snap.settingsConfig.auth,
            };
    }
    const next: SwitchAgentConfig = {
      providers: [...snapshots, snap],
      current: current ?? snap.id,
    };
    patchDraftConfig(next);
    openEditor(snap);
  };

  const commitDraft = () => {
    if (!draft || editingId === null) return;
    const providers = snapshots.map((s) => (s.id === editingId ? { ...draft } : s));
    patchDraftConfig({ providers, current });
    closeEditor();
  };

  const onDeleteSnapshot = (id: string) => {
    const providers = snapshots.filter((s) => s.id !== id);
    const nextCurrent =
      current === id ? (providers[0]?.id ?? undefined) : current;
    patchDraftConfig(
      providers.length === 0 && nextCurrent === undefined
        ? undefined
        : { providers, current: nextCurrent },
    );
    if (editingId === id) closeEditor();
    if (sourceId === id) setSourceId(null);
  };

  const onSetCurrent = (id: string) => {
    patchDraftConfig({ providers: snapshots, current: id });
  };

  // ── source view / edit ────────────────────────────────────────────

  const openSource = (snap: ProviderSnapshot, edit: boolean) => {
    setSourceId(snap.id);
    if (agent === "codex") {
      const parts = codexSourceParts(snap);
      setSourceText(parts.configText);
      setSourceAuthText(parts.authText);
    } else {
      setSourceText(snapshotToSource(snap));
    }
    setSourceEdit(edit);
    closeEditor();
  };

  const onSourceSave = () => {
    if (sourceId === null) return;
    const snap = snapshots.find((s) => s.id === sourceId);
    if (!snap) return;
    const settings =
      agent === "codex"
        ? parseCodexSourceParts(sourceText, sourceAuthText)
        : sourceToSnapshotSettings(sourceText);
    if (settings === null) return; // validator keeps the save button disabled
    patchDraftConfig({
      providers: snapshots.map((s) =>
        s.id === sourceId ? { ...s, settingsConfig: settings } : s,
      ),
      current,
    });
    setSourceEdit(false);
  };

  /** codex's save commits both files at once — disabled whenever either
   *  fails its parse (auth.json must be a JSON object; toml passes verbatim). */
  const codexSaveDisabled =
    agent === "codex" && parseCodexSourceParts(sourceText, sourceAuthText) === null;

  // ── template picker ───────────────────────────────────────────────

  const onTemplatePicked = (tp: {
    id: string;
    name: string;
    baseUrl: string;
    api: string;
    apiKey?: string;
    models: { id: string }[];
  }) => {
    const target = picking;
    setPicking(null);
    if (target === "new") {
      onAddSnapshot(tp);
      return;
    }
    if (target !== null && draft && editingId === target) {
      // Picked while editing: prefill the draft's EMPTY fields only —
      // a user-typed key must survive a template pick (R2).
      const next = { ...draft };
      next.name = next.name || tp.name;
      if (agent === "claude") {
        const env = { ...((next.settingsConfig.env as Record<string, unknown>) ?? {}) };
        if (!env.ANTHROPIC_BASE_URL) env.ANTHROPIC_BASE_URL = tp.baseUrl;
        if (!env.ANTHROPIC_MODEL && tp.models[0]) env.ANTHROPIC_MODEL = tp.models[0].id;
        if (!env.ANTHROPIC_AUTH_TOKEN && tp.apiKey) env.ANTHROPIC_AUTH_TOKEN = tp.apiKey;
        next.settingsConfig = { ...next.settingsConfig, env };
      } else {
        const auth = { ...((next.settingsConfig.auth as Record<string, unknown>) ?? {}) };
        if (!auth.OPENAI_API_KEY && tp.apiKey) auth.OPENAI_API_KEY = tp.apiKey;
        next.settingsConfig = {
          ...next.settingsConfig,
          auth,
          config:
            typeof next.settingsConfig.config === "string" &&
            next.settingsConfig.config.trim() !== ""
              ? next.settingsConfig.config
              : codexTomlFromTemplate(tp),
        };
      }
      setDraft(next);
    }
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

  // ── derived ───────────────────────────────────────────────────────

  const dirtyDraft =
    draft !== null && editingId !== null
      ? JSON.stringify(draft) !==
        JSON.stringify(snapshots.find((s) => s.id === editingId) ?? null)
      : false;

  const editingSnap =
    editingId !== null ? snapshots.find((s) => s.id === editingId) : undefined;
  const sourceSnap = sourceId !== null ? snapshots.find((s) => s.id === sourceId) : undefined;

  return (
    <div className="sbx-panel">
      {/* agent meta: install status + live effective config + apply target */}
      <div className="sbx-meta">
        {status && (
          <span className={`badge ${status.installed ? "badge-ok" : "badge-neutral"}`}>
            <span className="dot" />
            {status.installed ? t(lang, "sbxInstalled") : t(lang, "sbxNotInstalled")}
          </span>
        )}
        <span className="muted txs">
          {t(lang, "sbxApplyTarget")}
          <code className="mono">{applyTarget}</code>
        </span>
        {live && (
          <span className="muted txs">
            {t(lang, "sbxLive")}{" "}
            {agent === "claude"
              ? `${String(live.baseUrl ?? "—")} · ${String(live.model ?? "—")}`
              : `${String(live.modelProvider ?? "—")} · ${String(live.model ?? "—")}`}
          </span>
        )}
      </div>

      {/* snapshot list — row layout (R3): one config item per line so a long
       *  base URL truncates (title tooltip) instead of overflowing. */}
      {snapshots.length === 0 ? (
        <p className="muted">{t(lang, "sbxNoSnapshots")}</p>
      ) : (
        <div className="sbx-list">
          {snapshots.map((s) => (
            <div
              key={s.id}
              className={`sbx-row${current === s.id ? " current" : ""}`}
            >
              <div className="sbx-row-main">
                <div className="sbx-row-title">
                  <span className="sbx-row-name">{s.name || s.id}</span>
                  {current === s.id && (
                    <span className="badge badge-info">{t(lang, "maCurrent")}</span>
                  )}
                </div>
                <span className="muted txs mono sbx-row-id">{s.id}</span>
                <span
                  className="muted txs mono sbx-row-url"
                  title={snapshotBaseUrl(agent, s)}
                >
                  {snapshotBaseUrl(agent, s)}
                </span>
              </div>
              <div className="sbx-row-actions">
                {current !== s.id && (
                  <button
                    className="btn btn-ghost btn-sm"
                    onClick={() => onSetCurrent(s.id)}
                  >
                    <Icon name="check" />
                    {t(lang, "maSetCurrent")}
                  </button>
                )}
                <button
                  className="btn btn-ghost btn-sm"
                  onClick={() => openEditor(s)}
                >
                  <Icon name="edit" />
                  {t(lang, "mcEdit")}
                </button>
                <button
                  className="btn btn-ghost btn-sm"
                  onClick={() => openSource(s, false)}
                >
                  <Icon name="code" />
                  {t(lang, "sbxViewSource")}
                </button>
                <button
                  className="icon-btn danger"
                  aria-label={t(lang, "sbxDeleteSnapshot")}
                  onClick={() => onDeleteSnapshot(s.id)}
                >
                  <Icon name="trash" />
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* add + save/apply bar */}
      <div className="sbx-actions">
        <button className="btn btn-secondary btn-sm" onClick={() => onAddSnapshot()}>
          <Icon name="plus" />
          {t(lang, "maNewPreset")}
        </button>
        <button className="btn btn-secondary btn-sm" onClick={() => setPicking("new")}>
          <Icon name="box" />
          {t(lang, "sbxFromTemplate")}
        </button>
        <span className="sbx-spacer" />
        {msg && (
          <span className={`wizard-msg ${msg.ok ? "ok" : "err"}`}>{msg.text}</span>
        )}
        <button
          className="btn btn-primary"
          disabled={busy || snapshots.length === 0}
          onClick={() => void onSave()}
        >
          {busy ? t(lang, "mcSaving") : t(lang, "mcSave")}
        </button>
      </div>

      {/* snapshot editor (form or source) */}
      {draft && editingSnap && (
        <div className="sbx-editor">
          <div className="field">
            <label>{t(lang, "mcName")}</label>
            <input
              className="input"
              value={draft.name}
              placeholder={t(lang, "mcNamePlaceholder")}
              onChange={(e) => setDraft({ ...draft, name: e.target.value })}
            />
          </div>

          {agent === "claude" ? (
            <div className="sbx-env-grid">
              {CLAUDE_ENV_FIELDS.map(([key, labelKey]) => (
                <div className="field" key={key}>
                  <label>{t(lang, labelKey)}</label>
                  <input
                    className="input mono"
                    value={
                      String(
                        (draft.settingsConfig.env as Record<string, unknown>)?.[key] ?? "",
                      )
                    }
                    onChange={(e) => {
                      const env = {
                        ...((draft.settingsConfig.env as Record<string, unknown>) ?? {}),
                      };
                      if (e.target.value === "") {
                        delete env[key];
                      } else {
                        env[key] = e.target.value;
                      }
                      setDraft({ ...draft, settingsConfig: { ...draft.settingsConfig, env } });
                    }}
                  />
                </div>
              ))}
            </div>
          ) : (
            <>
              <div className="field">
                <label>{t(lang, "sbxCodexToml")}</label>
                <textarea
                  className="input mono"
                  rows={8}
                  spellCheck={false}
                  value={String(draft.settingsConfig.config ?? "")}
                  onChange={(e) =>
                    setDraft({
                      ...draft,
                      settingsConfig: { ...draft.settingsConfig, config: e.target.value },
                    })
                  }
                />
              </div>
              <div className="field">
                <label>{t(lang, "sbxCodexAuth")}</label>
                <textarea
                  className="input mono"
                  rows={3}
                  spellCheck={false}
                  placeholder='{ "OPENAI_API_KEY": "..." }'
                  value={safeJson(draft.settingsConfig.auth)}
                  onChange={(e) => {
                    try {
                      const v: unknown = JSON.parse(e.target.value || "{}");
                      setDraft({
                        ...draft,
                        settingsConfig: { ...draft.settingsConfig, auth: v },
                      });
                    } catch {
                      // keep typing; invalid JSON is not committed
                    }
                  }}
                />
              </div>
            </>
          )}

          <div className="dialog-actions">
            <button
              className="btn btn-secondary btn-sm"
              onClick={() => setPicking(editingId)}
            >
              <Icon name="box" />
              {t(lang, "sbxFromTemplate")}
            </button>
            <button
              className="btn btn-ghost btn-sm"
              onClick={() => openSource(editingSnap, true)}
            >
              <Icon name="code" />
              {t(lang, "sbxEditSource")}
            </button>
            <span className="sbx-spacer" />
            <button className="btn btn-secondary" onClick={closeEditor}>
              {t(lang, "cancel")}
            </button>
            <button className="btn btn-primary" disabled={!dirtyDraft} onClick={commitDraft}>
              {t(lang, "sbxUseSnapshot")}
            </button>
          </div>
        </div>
      )}

      {/* source view / editor */}
      {sourceSnap && !draft && (
        <div className="sbx-source">
          <div className="sbx-source-head">
            <span className="muted txs">
              {sourceSnap.name || sourceSnap.id} · {sandboxName}
            </span>
            {!sourceEdit && (
              <button
                className="btn btn-ghost btn-sm"
                onClick={() => setSourceEdit(true)}
              >
                <Icon name="edit" />
                {t(lang, "mcEdit")}
              </button>
            )}
          </div>
          <div className="sbx-file-label mono">
            {agent === "claude"
              ? "~/.claude/settings.json"
              : "~/.codex/config.toml"}
          </div>
          <SourceCodeEditor
            text={sourceText}
            language={agent === "claude" ? "json" : "toml"}
            editMode={sourceEdit}
            onChange={setSourceText}
            onSave={onSourceSave}
            onCancel={() => setSourceEdit(false)}
            saveDisabled={codexSaveDisabled}
            lang={lang}
          />
          {agent === "codex" && (
            <div className="sbx-source-extra">
              <div className="sbx-file-label mono">~/.codex/auth.json</div>
              <SourceCodeEditor
                text={sourceAuthText}
                language="json"
                editMode={sourceEdit}
                onChange={setSourceAuthText}
                onSave={onSourceSave}
                onCancel={() => setSourceEdit(false)}
                saveDisabled={codexSaveDisabled}
                lang={lang}
              />
            </div>
          )}
        </div>
      )}

      {picking !== null && (
        <TemplatePickerModal
          onSelect={onTemplatePicked}
          onClose={() => setPicking(null)}
          lang={lang}
        />
      )}
    </div>
  );
}

function safeJson(v: unknown): string {
  try {
    return v == null ? "" : JSON.stringify(v, null, 2);
  } catch {
    return "";
  }
}
