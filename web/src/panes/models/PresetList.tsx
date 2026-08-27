// PresetList — cc-switch-style preset cards for the switch-style agents
// (claude/codex): N named presets derived from the shared provider library,
// exactly one `current` takes effect. Switch = setCurrent + save + apply in
// one click; add/edit/duplicate/delete edit local canonical state and save
// through the shared save bar (same PUT /api/models/config channel — no new
// backend routes). A preset never copies the provider's key/headers blob:
// credentials stay in the provider library (SSOT, design §5).

import { useState } from "react";
import { Icon } from "../../icons";
import { t, type Lang } from "../../i18n";
import {
  emptyClaudePreset,
  emptyCodexPreset,
  incompatibleReason,
  liveReadbackText,
  protocolLabel,
  type AgentStatus,
  type AgentsResponse,
  type AnyPreset,
  type ApplyResponse,
  type CanonicalConfig,
  type ClaudePreset,
  type CodexPreset,
  type PresetAgent,
} from "./types";

/** Editing target: null = closed, "" = new-preset form, else a preset id. */
type EditTarget = string | null;

export function PresetList({
  agent,
  config,
  agentsStatus,
  agentDirty,
  saving,
  applying,
  applyResult,
  agentSaveMsg,
  onAddPreset,
  onUpdatePreset,
  onDeletePreset,
  onDuplicatePreset,
  onSwitchPreset,
  onSaveAssignment,
  lang,
}: {
  agent: PresetAgent;
  config: CanonicalConfig;
  agentsStatus: AgentsResponse | null;
  agentDirty: Set<string>;
  saving: boolean;
  applying: boolean;
  applyResult: ApplyResponse | null;
  agentSaveMsg: { ok: boolean; text: string } | null;
  onAddPreset: (agent: PresetAgent, preset: AnyPreset) => void;
  onUpdatePreset: (agent: PresetAgent, id: string, preset: AnyPreset) => void;
  onDeletePreset: (agent: PresetAgent, id: string) => void;
  onDuplicatePreset: (agent: PresetAgent, id: string) => void;
  /** Switch = setCurrent + save + apply, one click (design §4). */
  onSwitchPreset: (agent: PresetAgent, id: string) => void;
  onSaveAssignment: (agent: PresetAgent) => void;
  lang: Lang;
}): JSX.Element {
  const [editing, setEditing] = useState<EditTarget>(null);

  const block = agent === "claude" ? config.agents.claude : config.agents.codex;
  const presets: AnyPreset[] = block?.presets ?? [];
  const currentId = block?.current ?? null;
  const currentPreset = presets.find((p) => p.id === currentId);
  const status: AgentStatus | undefined = agentsStatus?.[agent];
  const isDirty = agentDirty.has(agent);

  // Does the live (native-file) config match the current preset? PRD: "与
  // current preset 对照,显示「当前生效与 current preset 是否一致」". A mismatch
  // means: switched current but not applied yet, or the file was edited
  // externally. Undefined when there is no live config or no current preset.
  let liveMatch: boolean | null = null;
  if (status?.live && currentPreset) {
    const provider = config.providers[currentPreset.provider];
    if (agent === "claude") {
      liveMatch =
        status.live.model === currentPreset.model &&
        (!provider || !status.live.baseUrl || status.live.baseUrl === provider.baseUrl);
    } else {
      liveMatch = status.live.model === currentPreset.model;
    }
  }

  return (
    <div className="ml-agent ml-preset-list">
      {/* install badge + live readback (shared header with the pi/opencode tabs) */}
      <div className="ml-agent-head">
        <span
          className={`ml-badge ${status?.installed ? "ml-badge-ok" : "ml-badge-warn"}`}
          title={status?.bin ?? undefined}
        >
          {status?.installed ? t(lang, "mcInstalled") : t(lang, "mcNotInstalled")}
        </span>
        <span className="ml-agent-live">
          {t(lang, "mcLive")} <code>{liveReadbackText(agent, status)}</code>
        </span>
        {liveMatch !== null && (
          <span
            className={`ml-badge ${liveMatch ? "ml-badge-ok" : "ml-badge-warn"}`}
            title={t(lang, liveMatch ? "maLiveMatchTip" : "maLiveMismatchTip")}
          >
            {t(lang, liveMatch ? "maLiveMatch" : "maLiveMismatch")}
          </span>
        )}
      </div>

      {presets.length === 0 && editing === null && (
        <div className="ml-empty">{t(lang, "maNoPresets")}</div>
      )}

      {/* new-preset entry */}
      <div className="ml-preset-toolbar">
        <button
          className="btn btn-primary btn-sm"
          disabled={editing !== null || isDirty || saving}
          onClick={() => setEditing("")}
        >
          <Icon name="plus" />
          {t(lang, "maNewPreset")}
        </button>
      </div>

      {/* new-preset form */}
      {editing === "" && (
        <div className="ml-preset-form-wrap">
          <div className="ml-preset-form-title">{t(lang, "maNewPreset")}</div>
          <PresetForm
            agent={agent}
            preset={agent === "claude" ? emptyClaudePreset() : emptyCodexPreset()}
            config={config}
            onSave={(p) => {
              onAddPreset(agent, p);
              setEditing(null);
            }}
            onCancel={() => setEditing(null)}
            lang={lang}
          />
        </div>
      )}

      {/* preset cards */}
      {presets.map((preset, idx) => {
        const isCurrent = preset.id === currentId;
        const provider = config.providers[preset.provider];
        const incompat = provider ? incompatibleReason(agent, provider) : null;
        return (
          <div key={preset.id} className="ml-preset-card">
            <div className="ml-preset-card-row">
              <span className="ml-preset-name">{preset.name || t(lang, "maDefaultPreset")}</span>
              {provider && (
                <span className="ml-badge ml-badge-protocol" title={preset.provider}>
                  {protocolLabel(provider.api)}
                </span>
              )}
              {incompat && (
                <span className="ml-badge ml-badge-warn">
                  {incompat === "incompatible-claude"
                    ? t(lang, "mcIncompatibleClaude")
                    : t(lang, "mcIncompatibleCodex")}
                </span>
              )}
              {isCurrent && (
                <span className="ml-badge ml-badge-current">{t(lang, "maCurrent")}</span>
              )}
              <span className="ml-preset-actions">
                <button
                  className="btn btn-secondary btn-sm"
                  disabled={isCurrent || isDirty || saving || applying}
                  title={isCurrent ? t(lang, "maAlreadyCurrent") : undefined}
                  onClick={() => onSwitchPreset(agent, preset.id)}
                >
                  {t(lang, "maSetCurrent")}
                </button>
                <button
                  className="btn btn-secondary btn-sm"
                  disabled={isDirty || saving}
                  onClick={() => setEditing(editing === preset.id ? null : preset.id)}
                >
                  {t(lang, "mcEdit")}
                </button>
                <button
                  className="btn btn-secondary btn-sm"
                  disabled={isDirty || saving}
                  onClick={() => onDuplicatePreset(agent, preset.id)}
                >
                  {t(lang, "maDuplicate")}
                </button>
                <button
                  className="btn btn-danger btn-sm"
                  disabled={isDirty || saving}
                  onClick={() => {
                    if (confirm(t(lang, "maDeletePresetConfirm"))) {
                      onDeletePreset(agent, preset.id);
                    }
                  }}
                  title={t(lang, "mcDeleteProvider")}
                >
                  <Icon name="x" />
                </button>
              </span>
            </div>
            <div className="ml-preset-meta">
              <code>{preset.provider || "—"}</code>
              <span className="ml-preset-sep">/</span>
              <code>{preset.model || "—"}</code>
            </div>

            {/* inline editor */}
            {editing === preset.id && (
              <div className="ml-preset-form-wrap">
                <PresetForm
                  agent={agent}
                  preset={preset}
                  config={config}
                  onSave={(p) => {
                    onUpdatePreset(agent, preset.id, p);
                    setEditing(null);
                  }}
                  onCancel={() => setEditing(null)}
                  lang={lang}
                />
              </div>
            )}
            {idx < presets.length - 1 && <div className="ml-preset-divider" />}
          </div>
        );
      })}

      {/* no-current warning — currentId "" is the freshly-added-first-preset
       * placeholder (backend backfills the id on PUT), not "unset"; only a
       * genuinely null/absent current warns. */}
      {presets.length > 0 && currentId == null && (
        <div className="ml-warn-line">{t(lang, "maNoCurrentPreset")}</div>
      )}

      {/* save + apply bar (shared with pi/opencode tabs) */}
      <div className="ml-save-bar">
        {isDirty && <span className="ml-dirty">{t(lang, "mcDirty")}</span>}
        {agentSaveMsg && (
          <span className={`ml-msg${agentSaveMsg.ok ? " ok" : " err"}`}>
            {agentSaveMsg.text}
          </span>
        )}
        <button
          className="btn btn-secondary"
          disabled={!isDirty || saving}
          onClick={() => onSaveAssignment(agent)}
        >
          {t(lang, "mcSave")}
        </button>
        <button
          className="btn btn-primary"
          disabled={isDirty || applying || !currentId}
          title={!currentId ? t(lang, "maNoCurrentPreset") : undefined}
          onClick={() => onSwitchPreset(agent, currentId ?? "")}
        >
          {applying ? <Icon name="refresh" /> : null}
          {applying ? t(lang, "mcApplying") : t(lang, "mcApply")}
        </button>
      </div>

      {/* apply result panel */}
      {applyResult && (
        <div className="ml-apply-result">
          {applyResult.ok && applyResult.errors.length === 0 && (
            <div className="ml-msg ok">{t(lang, "mcApplyOk")}</div>
          )}
          {applyResult.written.length > 0 && (
            <div className="ml-apply-written">
              <div className="ml-apply-label">{t(lang, "mcWrittenFiles")}</div>
              {applyResult.written.map((w) => (
                <div key={w.path} className="ml-apply-file ok">
                  <code>{w.path}</code>
                  {w.backup && (
                    <span className="ml-apply-backup">→ {w.backup}</span>
                  )}
                </div>
              ))}
            </div>
          )}
          {applyResult.errors.length > 0 && (
            <div className="ml-apply-errors">
              <div className="ml-apply-label err">{t(lang, "mcApplyErrors")}</div>
              {applyResult.errors.map((e) => (
                <div key={e.path} className="ml-apply-file err">
                  <code>{e.path}</code>
                  <span className="ml-apply-msg">{e.message}</span>
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

// ── form ─────────────────────────────────────────────────────────

/**
 * New/edit form for one preset: name, provider (compat-filtered select),
 * model (from the provider's list, manual fallback), then agent-specific
 * overrides. Local draft state only — the parent commits on save.
 */
function PresetForm({
  agent,
  preset,
  config,
  onSave,
  onCancel,
  lang,
}: {
  agent: PresetAgent;
  preset: AnyPreset;
  config: CanonicalConfig;
  onSave: (preset: AnyPreset) => void;
  onCancel: () => void;
  lang: Lang;
}): JSX.Element {
  // Draft: start from the preset (or blank defaults), commit on save only.
  const [name, setName] = useState(preset.name);
  const [provider, setProvider] = useState(preset.provider);
  const [model, setModel] = useState(preset.model);
  const [haikuModel, setHaikuModel] = useState(
    "haikuModel" in preset ? (preset as ClaudePreset).haikuModel ?? "" : "",
  );
  const [sonnetModel, setSonnetModel] = useState(
    "sonnetModel" in preset ? (preset as ClaudePreset).sonnetModel ?? "" : "",
  );
  const [opusModel, setOpusModel] = useState(
    "opusModel" in preset ? (preset as ClaudePreset).opusModel ?? "" : "",
  );
  const [authField, setAuthField] = useState(
    "authField" in preset ? (preset as ClaudePreset).authField || "AUTH_TOKEN" : "AUTH_TOKEN",
  );
  const [reasoningEffort, setReasoningEffort] = useState(
    "reasoningEffort" in preset ? (preset as CodexPreset).reasoningEffort ?? "" : "",
  );
  const [wireApi, setWireApi] = useState(
    "wireApi" in preset ? (preset as CodexPreset).wireApi || "responses" : "responses",
  );

  const providerEntry = config.providers[provider];
  const models = providerEntry?.models ?? [];

  const commit = (): void => {
    if (agent === "claude") {
      const p: ClaudePreset = {
        id: preset.id,
        name: name.trim(),
        provider,
        model,
        haikuModel: haikuModel || null,
        sonnetModel: sonnetModel || null,
        opusModel: opusModel || null,
        authField,
      };
      onSave(p);
    } else {
      const p: CodexPreset = {
        id: preset.id,
        name: name.trim(),
        provider,
        model,
        reasoningEffort: reasoningEffort || null,
        wireApi,
      };
      onSave(p);
    }
  };

  const valid = provider !== "" && model !== "";

  return (
    <div className="ml-preset-form">
      <div className="field">
        <label>{t(lang, "mcName")}</label>
        <input
          value={name}
          placeholder={t(lang, "maDefaultPreset")}
          onChange={(e) => setName(e.target.value)}
        />
      </div>

      <div className="field">
        <label>{t(lang, "mcProvider")}</label>
        <select
          value={provider}
          onChange={(e) => {
            const next = e.target.value;
            setProvider(next);
            // Reset the model when it isn't offered by the new provider.
            const nextModels = config.providers[next]?.models.map((m) => m.id) ?? [];
            if (!nextModels.includes(model)) setModel(nextModels[0] ?? "");
          }}
        >
          <option value="">{t(lang, "mcSelectProvider")}</option>
          {Object.entries(config.providers).map(([id, p]) => {
            const reason = incompatibleReason(agent, p);
            const isCurrent = id === provider;
            return (
              <option key={id} value={id} disabled={reason !== null && !isCurrent}>
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

      <div className="field">
        <label>{t(lang, "mcModel")}</label>
        {models.length > 0 ? (
          <select value={model} onChange={(e) => setModel(e.target.value)}>
            <option value="">—</option>
            {models.map((m) => (
              <option key={m.id} value={m.id}>
                {m.name ? `${m.name} (${m.id})` : m.id}
              </option>
            ))}
          </select>
        ) : (
          // Manual fallback when the provider has no discovered models yet.
          <input
            value={model}
            placeholder={provider ? t(lang, "maModelManual") : t(lang, "maSelectProviderFirst")}
            disabled={!provider}
            onChange={(e) => setModel(e.target.value)}
          />
        )}
      </div>

      {agent === "claude" && (
        <>
          <div className="field">
            <label>
              {t(lang, "mcHaikuModel")} <span className="hint">{t(lang, "mcFollowMain")}</span>
            </label>
            <input value={haikuModel} onChange={(e) => setHaikuModel(e.target.value)} />
          </div>
          <div className="field">
            <label>
              {t(lang, "mcSonnetModel")} <span className="hint">{t(lang, "mcFollowMain")}</span>
            </label>
            <input value={sonnetModel} onChange={(e) => setSonnetModel(e.target.value)} />
          </div>
          <div className="field">
            <label>
              {t(lang, "mcOpusModel")} <span className="hint">{t(lang, "mcFollowMain")}</span>
            </label>
            <input value={opusModel} onChange={(e) => setOpusModel(e.target.value)} />
          </div>
          <div className="field">
            <label>{t(lang, "mcAuthField")}</label>
            <select value={authField} onChange={(e) => setAuthField(e.target.value)}>
              <option value="AUTH_TOKEN">ANTHROPIC_AUTH_TOKEN</option>
              <option value="API_KEY">ANTHROPIC_API_KEY</option>
            </select>
          </div>
        </>
      )}

      {agent === "codex" && (
        <>
          <div className="field">
            <label>{t(lang, "mcReasoningEffort")}</label>
            <select value={reasoningEffort} onChange={(e) => setReasoningEffort(e.target.value)}>
              <option value="">{t(lang, "mcEffortNone")}</option>
              <option value="low">low</option>
              <option value="medium">medium</option>
              <option value="high">high</option>
            </select>
          </div>
          <div className="field">
            <label>
              {t(lang, "mcWireApi")} <span className="hint">{t(lang, "mcWireApiDerived")}</span>
            </label>
            <select value={wireApi} onChange={(e) => setWireApi(e.target.value)}>
              <option value="responses">responses</option>
              <option value="chat">chat</option>
            </select>
          </div>
        </>
      )}

      <div className="ml-preset-form-actions">
        <button className="btn btn-secondary" onClick={onCancel}>
          {t(lang, "cancel")}
        </button>
        <button className="btn btn-primary" disabled={!valid} onClick={commit}>
          {t(lang, "maSavePreset")}
        </button>
      </div>
    </div>
  );
}
