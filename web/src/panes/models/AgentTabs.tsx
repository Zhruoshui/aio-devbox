// AgentTabs — the four agent binding tabs (pi/opencode/Claude/Codex).
//
// Each tab binds a provider + model (with agent-specific overrides) from the
// shared provider library, shows install status + live readback, and applies
// the assignment to the agent's native config files. `incompatibleReason`
// filters the provider dropdown (R1: claude needs anthropic-messages; codex
// rejects it). Apply results surface written paths + backups + errors.

import { Icon } from "../../icons";
import { t, type Lang } from "../../i18n";
import {
  incompatibleReason,
  liveReadbackText,
  type AgentStatus,
  type AgentsResponse,
  type AgentTab,
  type ApplyResponse,
  type CanonicalConfig,
} from "./types";

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
  agent: AgentTab;
  config: CanonicalConfig;
  agentsStatus: AgentsResponse | null;
  agentDirty: Set<string>;
  saving: boolean;
  applying: boolean;
  applyResult: ApplyResponse | null;
  agentSaveMsg: { ok: boolean; text: string } | null;
  onUpdateAssignment: (agent: AgentTab, patch: Record<string, unknown>) => void;
  onSaveAssignment: (agent: AgentTab) => void;
  onApply: (agent: AgentTab) => void;
  lang: Lang;
}): JSX.Element {
  const assignment = config.agents[agent] as Record<string, unknown> | undefined;
  const status: AgentStatus | undefined = agentsStatus?.[agent];
  const isDirty = agentDirty.has(agent);
  const currentProviderId = (assignment?.provider as string) ?? "";
  const currentModelId = (assignment?.model as string) ?? "";
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
              onChange={(e) =>
                onUpdateAssignment(agent, { provider: e.target.value })
              }
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

          {/* claude overrides: three-tier model mapping + auth field */}
          {agent === "claude" && (
            <>
              <div className="field">
                <label>
                  {t(lang, "mcHaikuModel")}{" "}
                  <span className="hint">{t(lang, "mcFollowMain")}</span>
                </label>
                <input
                  value={(assignment?.haikuModel as string) ?? ""}
                  onChange={(e) =>
                    onUpdateAssignment(agent, { haikuModel: e.target.value || null })
                  }
                />
              </div>
              <div className="field">
                <label>
                  {t(lang, "mcSonnetModel")}{" "}
                  <span className="hint">{t(lang, "mcFollowMain")}</span>
                </label>
                <input
                  value={(assignment?.sonnetModel as string) ?? ""}
                  onChange={(e) =>
                    onUpdateAssignment(agent, { sonnetModel: e.target.value || null })
                  }
                />
              </div>
              <div className="field">
                <label>
                  {t(lang, "mcOpusModel")}{" "}
                  <span className="hint">{t(lang, "mcFollowMain")}</span>
                </label>
                <input
                  value={(assignment?.opusModel as string) ?? ""}
                  onChange={(e) =>
                    onUpdateAssignment(agent, { opusModel: e.target.value || null })
                  }
                />
              </div>
              <div className="field">
                <label>{t(lang, "mcAuthField")}</label>
                <select
                  value={(assignment?.authField as string) ?? "AUTH_TOKEN"}
                  onChange={(e) => onUpdateAssignment(agent, { authField: e.target.value })}
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
                    onUpdateAssignment(agent, { reasoningEffort: e.target.value || null })
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
                  <span className="hint">{t(lang, "mcWireApiDerived")}</span>
                </label>
                <select
                  value={(assignment?.wireApi as string) ?? ""}
                  onChange={(e) => onUpdateAssignment(agent, { wireApi: e.target.value })}
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
