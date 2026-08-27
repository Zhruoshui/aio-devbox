// AgentTabs — the incremental agent binding tabs (pi/opencode only).
//
// pi and opencode are incremental agents: one provider + model assignment,
// shared from the provider library, with install status + live readback and
// apply to the agent's native config files. `incompatibleReason` filters the
// provider dropdown. The switch-style agents (claude/codex) have their own
// PresetList — they are NOT handled here (design §5: incremental vs switch
// have different semantics). Apply results surface written paths + backups
// + errors.
//
// Kumo redesign (08-27-model-config-redesign): agent-head (2xl name + status
// badges) → paradigm strip → form cards (Live readback / Assignment+savebar /
// Native config). The model field is a ModelPicker over the chosen provider's
// models[] (no free-text); below the assignment sits the live list — every
// provider in the agent's NATIVE config with sync/edit/delete actions
// (LiveProviderList).

import { useState } from "react";
import { Icon } from "../../icons";
import { t, type Lang } from "../../i18n";
import {
  incompatibleReason,
  liveMatchState,
  liveReadbackText,
  type AgentStatus,
  type AgentsResponse,
  type ApplyResponse,
  type CanonicalConfig,
} from "./types";
import { LiveProviderList, type LiveEditPatch } from "./LiveProviderList";
import { ModelPicker } from "./ModelPicker";

/** The incremental (single-assignment) agents. */
type IncrementalAgent = "pi" | "opencode";

export function AgentTabs({
  agent,
  config,
  agentsStatus,
  agentDirty,
  saving,
  applying,
  applyResult,
  agentSaveMsg,
  liveBusyId,
  onUpdateAssignment,
  onSaveAssignment,
  onApply,
  onSyncLive,
  onEditLive,
  onDeleteLive,
  lang,
}: {
  agent: IncrementalAgent;
  config: CanonicalConfig;
  agentsStatus: AgentsResponse | null;
  agentDirty: Set<string>;
  saving: boolean;
  applying: boolean;
  applyResult: ApplyResponse | null;
  agentSaveMsg: { ok: boolean; text: string } | null;
  /** Row currently running a live sync/edit/delete (disables its buttons). */
  liveBusyId: string | null;
  onUpdateAssignment: (agent: IncrementalAgent, patch: Record<string, unknown>) => void;
  onSaveAssignment: (agent: IncrementalAgent) => void;
  onApply: (agent: IncrementalAgent) => void;
  onSyncLive: (agent: IncrementalAgent, id: string) => void;
  onEditLive: (agent: IncrementalAgent, id: string, patch: LiveEditPatch) => void;
  onDeleteLive: (agent: IncrementalAgent, id: string) => void;
  lang: Lang;
}): JSX.Element {
  const [pickerOpen, setPickerOpen] = useState(false);

  const assignment = config.agents[agent];
  const status: AgentStatus | undefined = agentsStatus?.[agent];
  const isDirty = agentDirty.has(agent);
  const currentProviderId = assignment?.provider ?? "";
  const currentModelId = assignment?.model ?? "";
  const providerList = Object.entries(config.providers);
  const models = config.providers[currentProviderId]?.models ?? [];
  const selectedModel = models.find((m) => m.id === currentModelId);

  const match = liveMatchState(agent, status?.live ?? null, assignment);

  return (
    <div className="ml-agent">
      {/* agent-head: 2xl name + install + consistency badges */}
      <div className="ml-agent-head">
        <span className="ml-agent-name">{agent}</span>
        <span
          className={`ml-badge ${status?.installed ? "ml-badge-ok" : "ml-badge-warn"}`}
          title={status?.bin ?? undefined}
        >
          <span className="dot" />
          {status?.installed ? t(lang, "mcInstalled") : t(lang, "mcNotInstalled")}
        </span>
        {match !== "unknown" && (
          <span
            className={`ml-badge ${match === "match" ? "ml-badge-ok" : "ml-badge-warn"}`}
            title={t(lang, match === "match" ? "maLiveMatchTip" : "maLiveMismatchTip")}
          >
            <span className="dot" />
            {t(lang, match === "match" ? "maLiveMatch" : "maLiveMismatch")}
          </span>
        )}
      </div>

      {/* paradigm strip */}
      <div className="ml-paradigm-strip">
        <Icon name="cube" />
        <span>{t(lang, "maParadigmIncremental")}</span>
      </div>

      {/* live readback card */}
      <div className="ml-form-card">
        <h3>{t(lang, "mcLiveHeading")}</h3>
        <p className="sub">{t(lang, "mcLive")}</p>
        <div className="ml-agent-live">
          <code>{liveReadbackText(agent, status)}</code>
        </div>
      </div>

      {/* assignment card */}
      <div className="ml-form-card">
        <h3>{t(lang, "mcAssign")}</h3>
        <div className="ml-agent-form">
          {/* provider dropdown */}
          <div className="field">
            <label>{t(lang, "mcProvider")}</label>
            <select
              value={currentProviderId}
              onChange={(e) => {
                onUpdateAssignment(agent, { provider: e.target.value });
                setPickerOpen(false);
              }}
            >
              <option value="">{t(lang, "mcSelectProvider")}</option>
              {providerList.map(([id, p]) => {
                const reason = incompatibleReason(agent, p);
                // The currently-selected provider stays selectable even if it
                // became incompatible after the assignment was saved (so the
                // user can see and change it).
                const isCurrent = id === currentProviderId;
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

          {/* model picker over the provider's models[] (no free text) */}
          <div className="field">
            <label>{t(lang, "mcModel")}</label>
            <button
              className="ml-model-trigger"
              disabled={!currentProviderId}
              onClick={() => setPickerOpen(!pickerOpen)}
            >
              <code>
                {selectedModel
                  ? selectedModel.name
                    ? `${selectedModel.name} (${selectedModel.id})`
                    : selectedModel.id
                  : currentModelId || t(lang, "maPickModel")}
              </code>
              <Icon name={pickerOpen ? "chev-r" : "chev-l"} />
            </button>
            {pickerOpen &&
              (models.length === 0 ? (
                <div className="ml-hint">{t(lang, "maNoModelsInProvider")}</div>
              ) : (
                <ModelPicker
                  models={models}
                  selectedId={currentModelId || undefined}
                  onPick={(id) => {
                    onUpdateAssignment(agent, { model: id });
                    setPickerOpen(false);
                  }}
                  lang={lang}
                />
              ))}
          </div>
        </div>

        {/* save + apply bar */}
        <div className="ml-savebar">
          {isDirty && (
            <span className="dirty">
              <span className="dot" />
              {t(lang, "mcDirty")}
            </span>
          )}
          {agentSaveMsg && (
            <span className={`ml-msg${agentSaveMsg.ok ? " ok" : " err"}`}>
              {agentSaveMsg.text}
            </span>
          )}
          <span className="spacer" />
          <button
            className="btn btn-secondary"
            disabled={!isDirty || saving}
            onClick={() => onSaveAssignment(agent)}
          >
            {t(lang, "mcSaveAssignment")}
          </button>
          <button
            className="btn btn-primary"
            disabled={isDirty || applying || !currentProviderId || !currentModelId}
            onClick={() => onApply(agent)}
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

      {/* native config card */}
      <div className="ml-form-card">
        <h3>{t(lang, "mcNativeHeading")}</h3>
        <p className="sub">{t(lang, "mcNativeSub")}</p>
        {!status?.installed && (
          <div className="ml-hint">{t(lang, "maLivePreWrite")}</div>
        )}
        <LiveProviderList
          agent={agent}
          live={status?.live ?? null}
          busyId={liveBusyId}
          onSync={(id) => onSyncLive(agent, id)}
          onEdit={(id, patch) => onEditLive(agent, id, patch)}
          onDelete={(id) => onDeleteLive(agent, id)}
          lang={lang}
        />
      </div>
    </div>
  );
}
