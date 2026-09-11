// AgentTabs — the incremental agent binding tabs (pi/opencode), ported from
// web/src/panes/models/AgentTabs.tsx with the sandbox-local parts trimmed
// (Phase 4c): the assignment editor (provider dropdown + ModelPicker + save
// through PUT /api/models/config) is kept verbatim, while the install-status
// badge, live readback card, apply button/apply-result panel and the
// LiveProviderList (native-file management) are dropped — those read and
// write files INSIDE a sandbox, which mgr has no API for. The MgrNotice strip
// links out to each running sandbox's workbench where that live view lives.

import { useState } from "react";
import { Icon } from "../../icons";
import { t, type Lang } from "../../i18n";
import { incompatibleReason, protocolLabel, type CanonicalConfig } from "./types";
import { MgrNotice, type SandboxLink } from "./MgrNotice";
import { ModelPicker } from "./ModelPicker";

/** The incremental (single-assignment) agents. */
type IncrementalAgent = "pi" | "opencode";

export function AgentTabs({
  agent,
  config,
  agentDirty,
  saving,
  agentSaveMsg,
  sandboxLinks,
  onGoWorkspace,
  onUpdateAssignment,
  onSaveAssignment,
  lang,
}: {
  agent: IncrementalAgent;
  config: CanonicalConfig;
  agentDirty: Set<string>;
  saving: boolean;
  agentSaveMsg: { ok: boolean; text: string } | null;
  sandboxLinks: SandboxLink[];
  onGoWorkspace?: (name: string) => void;
  onUpdateAssignment: (agent: IncrementalAgent, patch: Record<string, unknown>) => void;
  onSaveAssignment: (agent: IncrementalAgent) => void;
  lang: Lang;
}): JSX.Element {
  const [pickerOpen, setPickerOpen] = useState(false);

  const assignment = config.agents[agent];
  const isDirty = agentDirty.has(agent);
  const currentProviderId = assignment?.provider ?? "";
  const currentModelId = assignment?.model ?? "";
  const providerList = Object.entries(config.providers);
  const models = config.providers[currentProviderId]?.models ?? [];
  const selectedModel = models.find((m) => m.id === currentModelId);

  return (
    <div className="ml-agent">
      {/* agent-head: 2xl name (install/live badges live in the sandbox UI) */}
      <div className="ml-agent-head">
        <span className="ml-agent-name">{agent}</span>
      </div>

      {/* paradigm strip */}
      <div className="ml-paradigm-strip">
        <span>{t(lang, "maParadigmIncremental")}</span>
      </div>

      <MgrNotice links={sandboxLinks} lang={lang} onGoWorkspace={onGoWorkspace} />

      {/* S2 (R1): provider card wall — click a card to point this agent's
       * assignment at that provider (one-click switch, cc-switch-style).
       * The card for the CURRENTLY-assigned provider is highlighted; the
       * provider dropdown below still handles incompatible providers /
       * explicit model picking. */}
      <div className="ml-agent-cards">
        <div className="ml-sec-head">
          <h2>{t(lang, "maProviderCards")}</h2>
        </div>
        <div className="ml-grid">
          {providerList.map(([id, p]) => {
            const isActive = id === currentProviderId;
            return (
              <div
                key={id}
                className={`ml-card${isActive ? " is-active" : ""}`}
                role="button"
                tabIndex={0}
                aria-label={p.name || id}
                aria-current={isActive ? "true" : undefined}
                onClick={() => {
                  onUpdateAssignment(agent, { provider: id });
                  setPickerOpen(false);
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    onUpdateAssignment(agent, { provider: id });
                    setPickerOpen(false);
                  }
                }}
              >
                <div className="ml-card-head">
                  <div className="ml-card-main">
                    <span className="ml-badge ml-badge-protocol" title={p.api}>
                      {protocolLabel(p.api)}
                    </span>
                    <span className="ml-card-name">{p.name || id}</span>
                  </div>
                  {isActive && <span className="ml-badge ml-badge-current">{t(lang, "maActive")}</span>}
                </div>
                <div className="ml-card-url" title={p.baseUrl}>
                  {p.baseUrl || "—"}
                </div>
                <div className="ml-card-meta">
                  <span>
                    {p.models.length} {t(lang, "mcModels")}
                  </span>
                </div>
              </div>
            );
          })}
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

        {/* save bar (no Apply: rendering happens sandbox-side on pull) */}
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
            className="btn btn-primary"
            disabled={!isDirty || saving}
            onClick={() => onSaveAssignment(agent)}
          >
            {t(lang, "mcSaveAssignment")}
          </button>
        </div>
      </div>
    </div>
  );
}
