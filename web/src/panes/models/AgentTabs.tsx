// AgentTabs — the incremental agent binding tabs (pi/opencode only).
//
// pi and opencode are incremental agents: one provider + model assignment,
// shared from the provider library, with install status + live readback and
// apply to the agent's native config files. `incompatibleReason` filters the
// provider dropdown. The switch-style agents (claude/codex) have their own
// PresetList — they are NOT handled here (design §5: incremental vs switch
// have different semantics). Apply results surface written paths + backups
// + errors.

import { Icon } from "../../icons";
import { t, type Lang } from "../../i18n";
import {
  incompatibleReason,
  liveReadbackText,
  type AgentStatus,
  type AgentsResponse,
  type ApplyResponse,
  type CanonicalConfig,
} from "./types";

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
  onUpdateAssignment,
  onSaveAssignment,
  onApply,
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
  onUpdateAssignment: (agent: IncrementalAgent, patch: Record<string, unknown>) => void;
  onSaveAssignment: (agent: IncrementalAgent) => void;
  onApply: (agent: IncrementalAgent) => void;
  lang: Lang;
}): JSX.Element {
  const assignment = config.agents[agent];
  const status: AgentStatus | undefined = agentsStatus?.[agent];
  const isDirty = agentDirty.has(agent);
  const currentProviderId = assignment?.provider ?? "";
  const currentModelId = assignment?.model ?? "";
  const providerList = Object.entries(config.providers);

  return (
    <div className="ml-agent">
      {/* install badge + live readback */}
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
      </div>

      <div className="ml-agent-form">
        {/* provider dropdown */}
        <div className="field">
          <label>{t(lang, "mcProvider")}</label>
          <select
            value={currentProviderId}
            onChange={(e) => onUpdateAssignment(agent, { provider: e.target.value })}
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

        {/* model dropdown */}
        <div className="field">
          <label>{t(lang, "mcModel")}</label>
          <select
            value={currentModelId}
            onChange={(e) => onUpdateAssignment(agent, { model: e.target.value })}
            disabled={!currentProviderId}
          >
            <option value="">—</option>
            {(config.providers[currentProviderId]?.models ?? []).map((m) => (
              <option key={m.id} value={m.id}>
                {m.name ? `${m.name} (${m.id})` : m.id}
              </option>
            ))}
          </select>
        </div>
      </div>

      {/* save + apply bar */}
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
  );
}
