// PresetList — the switch-style agent tabs (claude/codex), redesigned per
// the 09-11 prototype (docs/Web-Prototype/models.html tab-claude/tab-codex).
//
// Layout: .strip paradigm explainer → .two grid. Left column: "预设 (N)"
// sec-head with 新建预设, then one .card.preset per preset (current gets the
// .current ring + badge-ok 当前; body = provider · model mono line + a .kv
// definition list of the agent-specific extras; footer = 切换为当前 /
// 正在生效 + copy / edit / delete icon buttons). Editing and creating stay
// inline PresetForm cards below the entry. Right column: SandboxTable.
//
// Data flow unchanged: preset CRUD edits the canonical config in memory and
// commits through the savebar (PUT /api/models/config?profile=); "切换为当前"
// is setCurrent + save in one click; the sandbox-side render happens when
// the sandbox pulls the config (≤60s).

import { useState } from "react";
import { ConfirmDialog } from "../../components/Dialogs";
import { Icon } from "../../icons";
import { t, type Lang } from "../../i18n";
import type { Sandbox } from "../../types";
import {
  emptyClaudePreset,
  emptyCodexPreset,
  incompatibleReason,
  type AnyPreset,
  type CanonicalConfig,
  type ClaudePreset,
  type CodexPreset,
  type PresetAgent,
} from "./types";
import { MgrNotice } from "./MgrNotice";
import { SandboxTable, runningLinks } from "./SandboxTable";

/** Editing target: null = closed, "" = new-preset form, else a preset id. */
type EditTarget = string | null;

export function PresetList({
  agent,
  config,
  agentDirty,
  saving,
  agentSaveMsg,
  profileId,
  profileName,
  profileNames,
  sandboxList,
  sbxBusy,
  onGoWorkspace,
  onGoList,
  onGoProfile,
  onToggleSandboxAgent,
  onAddPreset,
  onUpdatePreset,
  onDeletePreset,
  onDuplicatePreset,
  onSwitchPreset,
  onSaveAssignment,
  onDiscardAssignment,
  lang,
}: {
  agent: PresetAgent;
  config: CanonicalConfig;
  agentDirty: Set<string>;
  saving: boolean;
  agentSaveMsg: { ok: boolean; text: string } | null;
  profileId: string;
  profileName: string;
  /** id → display name for every profile (SandboxTable's Profile column). */
  profileNames: Record<string, string>;
  sandboxList: Sandbox[] | null;
  sbxBusy: string;
  onGoWorkspace?: (name: string) => void;
  onGoList?: () => void;
  onGoProfile: (id: string) => void;
  onToggleSandboxAgent: (name: string, agent: string, on: boolean) => void;
  onAddPreset: (agent: PresetAgent, preset: AnyPreset) => void;
  onUpdatePreset: (agent: PresetAgent, id: string, preset: AnyPreset) => void;
  onDeletePreset: (agent: PresetAgent, id: string) => void;
  onDuplicatePreset: (agent: PresetAgent, id: string) => void;
  /** Switch = setCurrent + save, one click (the render happens sandbox-side). */
  onSwitchPreset: (agent: PresetAgent, id: string) => void;
  onSaveAssignment: (agent: PresetAgent) => void;
  onDiscardAssignment: (agent: PresetAgent) => void;
  lang: Lang;
}): JSX.Element {
  const [editing, setEditing] = useState<EditTarget>(null);
  // R1: preset deletion confirms through the shared in-app dialog (the
  // native confirm() is gone); the id being deleted, null = closed.
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);

  const block = agent === "claude" ? config.agents.claude : config.agents.codex;
  const presets: AnyPreset[] = block?.presets ?? [];
  const currentId = block?.current ?? null;
  const isDirty = agentDirty.has(agent);

  return (
    <div>
      {/* paradigm strip (prototype: switch-style render-target explainer) */}
      <div className="strip">
        <Icon name="info" />
        <span>
          {t(lang, "maStripSwitcher")
            .replace("{profile}", profileName || profileId)
            .replace(
              "{file}",
              agent === "claude" ? "~/.claude/settings.json" : "~/.codex/config.toml",
            )}
        </span>
      </div>

      <MgrNotice links={runningLinks(sandboxList)} lang={lang} onGoWorkspace={onGoWorkspace} />

      <div className="two">
        {/* left: preset cards */}
        <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-4)" }}>
          <div className="sec-head" style={{ margin: 0 }}>
            <div>
              <h2 style={{ fontSize: "var(--text-base)", margin: 0 }}>
                {t(lang, "maPresetHeading")}{" "}
                <span className="muted" style={{ fontWeight: 400 }}>
                  ({presets.length})
                </span>
              </h2>
            </div>
            <div className="sec-acts">
              <button
                className="btn btn-primary btn-sm"
                disabled={editing !== null || isDirty || saving}
                onClick={() => setEditing("")}
              >
                <Icon name="plus" />
                {t(lang, "maNewPreset")}
              </button>
            </div>
          </div>

          {/* new-preset form */}
          {editing === "" && (
            <div className="card assign">
              <h3 className="ml-preset-form-title">{t(lang, "maNewPreset")}</h3>
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

          {presets.length === 0 && editing === null && (
            <div
              className="card card-pad muted tsm"
              style={{ textAlign: "center", padding: "var(--space-8)" }}
            >
              {t(lang, "maNoPresets")}
            </div>
          )}

          {/* preset cards */}
          {presets.map((preset) => {
            const isCurrent = preset.id === currentId;
            const provider = config.providers[preset.provider];
            const incompat = provider ? incompatibleReason(agent, provider) : null;
            return (
              <div key={preset.id} className={`card preset${isCurrent ? " current" : ""}`}>
                <div className="preset-head">
                  <h3>{preset.name || t(lang, "maDefaultPreset")}</h3>
                  {incompat && (
                    <span className="badge badge-warn">
                      {incompat === "incompatible-claude"
                        ? t(lang, "mcIncompatibleClaude")
                        : t(lang, "mcIncompatibleCodex")}
                    </span>
                  )}
                  {isCurrent && (
                    <span className="badge badge-ok">
                      <span className="dot" />
                      {t(lang, "maCurrent")}
                    </span>
                  )}
                </div>
                <div className="mono tsm">
                  {(provider ? provider.name || preset.provider : preset.provider || "—")}{" "}
                  · {preset.model || "—"}
                </div>
                <dl className="kv">{presetExtras(agent, preset)}</dl>
                <div className="preset-acts">
                  {isCurrent ? (
                    <span className="txs muted" style={{ marginRight: "auto" }}>
                      {t(lang, "maTakingEffect")}
                    </span>
                  ) : (
                    <button
                      className="btn btn-secondary btn-sm"
                      disabled={isDirty || saving}
                      onClick={() => onSwitchPreset(agent, preset.id)}
                    >
                      {t(lang, "maSetCurrent")}
                    </button>
                  )}
                  <button
                    className="icon-btn"
                    disabled={isDirty || saving}
                    aria-label={t(lang, "maDuplicate")}
                    title={t(lang, "maDuplicate")}
                    onClick={() => onDuplicatePreset(agent, preset.id)}
                  >
                    <Icon name="copy" />
                  </button>
                  <button
                    className="icon-btn"
                    disabled={isDirty || saving}
                    aria-label={t(lang, "mcEdit")}
                    title={t(lang, "mcEdit")}
                    onClick={() => setEditing(editing === preset.id ? null : preset.id)}
                  >
                    <Icon name="edit" />
                  </button>
                  <button
                    className="icon-btn danger"
                    disabled={isDirty || saving}
                    aria-label={t(lang, "mcDeleteProvider")}
                    title={t(lang, "mcDeleteProvider")}
                    onClick={() => setDeleteTarget(preset.id)}
                  >
                    <Icon name="trash" />
                  </button>
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
              </div>
            );
          })}

          {/* no-current warning — currentId "" is the freshly-added-first-preset
           * placeholder (backend backfills the id on PUT), not "unset"; only
           * a genuinely null/absent current warns. */}
          {presets.length > 0 && currentId == null && (
            <div className="ml-warn-strip">
              <Icon name="alert" />
              {t(lang, "maNoCurrentPreset")}
            </div>
          )}

          {/* save bar */}
          <div className="savebar">
            <span className={isDirty ? "ml-dirty" : undefined}>
              {isDirty ? t(lang, "mcDirty") : t(lang, "maInSync")}
            </span>
            {agentSaveMsg && (
              <span className={`ml-msg${agentSaveMsg.ok ? " ok" : " err"}`}>
                {agentSaveMsg.text}
              </span>
            )}
            <button
              className="btn btn-ghost btn-sm"
              disabled={!isDirty || saving}
              onClick={() => onDiscardAssignment(agent)}
            >
              {t(lang, "maDiscard")}
            </button>
            <button
              className="btn btn-primary"
              disabled={!isDirty || saving}
              onClick={() => onSaveAssignment(agent)}
            >
              {saving ? t(lang, "mcSaving") : t(lang, "mcSave")}
            </button>
          </div>
        </div>

        {/* right: per-sandbox agent-subset switches */}
        <SandboxTable
          agent={agent}
          profileId={profileId}
          profileNames={profileNames}
          sandboxList={sandboxList}
          sbxBusy={sbxBusy}
          onGoWorkspace={onGoWorkspace}
          onGoList={onGoList}
          onGoProfile={onGoProfile}
          onToggleSandboxAgent={onToggleSandboxAgent}
          lang={lang}
        />
      </div>

      {/* R1: destructive delete confirm (shared ConfirmDialog). */}
      {deleteTarget !== null && (
        <ConfirmDialog
          lang={lang}
          danger
          title={t(lang, "mcDeleteProvider")}
          desc={t(lang, "maDeletePresetConfirm")}
          confirmLabel={t(lang, "mcDeleteProvider")}
          onConfirm={() => {
            onDeletePreset(agent, deleteTarget);
            setDeleteTarget(null);
          }}
          onCancel={() => setDeleteTarget(null)}
        />
      )}
    </div>
  );
}

/** The preset card's .kv extras (prototype: Opus 映射 / 认证字段 / 推理强度 /
 * wire API). Null optional values render as —. Flat dt/dd pairs keep the
 * .kv grid's two-column auto-placement (label | value) intact. */
function presetExtras(agent: PresetAgent, preset: AnyPreset): JSX.Element[] {
  const rows: [string, string][] = [];
  if (agent === "claude") {
    const p = preset as ClaudePreset;
    rows.push(["Haiku 映射", p.haikuModel || "—"]);
    rows.push(["Sonnet 映射", p.sonnetModel || "—"]);
    rows.push(["Opus 映射", p.opusModel || "—"]);
    rows.push([
      "认证字段",
      p.authField === "API_KEY" ? "ANTHROPIC_API_KEY" : "ANTHROPIC_AUTH_TOKEN",
    ]);
  } else {
    const p = preset as CodexPreset;
    rows.push(["推理强度", p.reasoningEffort || "—"]);
    rows.push(["wire API", p.wireApi]);
  }
  return rows.flatMap(([k, v]) => [
    <dt key={k}>{k}</dt>,
    <dd key={`${k}:v`}>{v}</dd>,
  ]);
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
          className="input"
          value={name}
          placeholder={t(lang, "maDefaultPreset")}
          onChange={(e) => setName(e.target.value)}
        />
      </div>

      <div className="field">
        <label>{t(lang, "mcProvider")}</label>
        <select
          className="input"
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
          <select className="input" value={model} onChange={(e) => setModel(e.target.value)}>
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
            className="input mono"
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
            <input
              className="input mono"
              value={haikuModel}
              onChange={(e) => setHaikuModel(e.target.value)}
            />
          </div>
          <div className="field">
            <label>
              {t(lang, "mcSonnetModel")} <span className="hint">{t(lang, "mcFollowMain")}</span>
            </label>
            <input
              className="input mono"
              value={sonnetModel}
              onChange={(e) => setSonnetModel(e.target.value)}
            />
          </div>
          <div className="field">
            <label>
              {t(lang, "mcOpusModel")} <span className="hint">{t(lang, "mcFollowMain")}</span>
            </label>
            <input
              className="input mono"
              value={opusModel}
              onChange={(e) => setOpusModel(e.target.value)}
            />
          </div>
          <div className="field">
            <label>{t(lang, "mcAuthField")}</label>
            <select className="input" value={authField} onChange={(e) => setAuthField(e.target.value)}>
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
            <select
              className="input"
              value={reasoningEffort}
              onChange={(e) => setReasoningEffort(e.target.value)}
            >
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
            <select className="input" value={wireApi} onChange={(e) => setWireApi(e.target.value)}>
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
