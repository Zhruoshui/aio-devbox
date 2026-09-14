// AgentTabs — the incremental agent tabs (pi/opencode), redesigned per the
// 09-11 prototype (docs/Web-Prototype/models.html tab-pi/tab-opencode).
//
// Layout: .strip paradigm explainer → .two grid. Left column (.card.assign):
// "当前指向" single-select over EVERY (provider, model) pair in the library,
// rendered as .model-opt radio rows (mono id + provider · name · cost meta +
// reasoning chip); the savebar below shows the dirty state with 保存/放弃.
// Right column: SandboxTable (per-sandbox agent-subset switches).
//
// Data flow unchanged vs the pre-redesign tab: a radio pick patches the
// canonical config through onUpdateAssignment (provider reset drops a model
// the new provider doesn't list — ModelsPage's reducer); save PUTs the whole
// config with ?profile=; the sandbox pull (≤60s) renders the result.
// MgrNotice keeps the shared-library strip with running-sandbox links.

import { t, type Lang } from "../../i18n";
import { Icon } from "../../icons";
import type { Sandbox } from "../../types";
import type { CanonicalConfig } from "./types";
import { MgrNotice } from "./MgrNotice";
import { SandboxTable, runningLinks } from "./SandboxTable";

/** The incremental (single-assignment) agents. */
type IncrementalAgent = "pi" | "opencode";

export function AgentTabs({
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
  onUpdateAssignment,
  onSaveAssignment,
  onDiscardAssignment,
  lang,
}: {
  agent: IncrementalAgent;
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
  onUpdateAssignment: (agent: IncrementalAgent, patch: Record<string, unknown>) => void;
  onSaveAssignment: (agent: IncrementalAgent) => void;
  onDiscardAssignment: (agent: IncrementalAgent) => void;
  lang: Lang;
}): JSX.Element {
  const assignment = config.agents[agent];
  const isDirty = agentDirty.has(agent);
  const currentProviderId = assignment?.provider ?? "";
  const currentModelId = assignment?.model ?? "";
  const currentKey = `${currentProviderId}/${currentModelId}`;

  return (
    <div>
      {/* paradigm strip (prototype: incremental render-target explainer) */}
      <div className="strip">
        <Icon name="info" />
        <span>
          {t(lang, "maStripIncremental")
            .replace("{profile}", profileName || profileId)
            .replace("{file}", agent === "pi" ? "~/.pi/models.json" : "opencode.json")}
        </span>
      </div>

      <MgrNotice links={runningLinks(sandboxList)} lang={lang} onGoWorkspace={onGoWorkspace} />

      <div className="two">
        {/* left: current binding single-select */}
        <div className="card assign">
          <h3>
            {t(lang, "maCurrentBinding")}{" "}
            <span className="muted" style={{ fontWeight: 400 }}>
              · {profileName || profileId}
            </span>
          </h3>
          <div
            style={{ display: "flex", flexDirection: "column", gap: 8 }}
            role="radiogroup"
            aria-label={t(lang, "mcModel")}
          >
            {Object.entries(config.providers).flatMap(([pid, p]) =>
              p.models.map((m) => {
                const key = `${pid}/${m.id}`;
                return (
                  <label key={key} className="model-opt">
                    <input
                      className="radio"
                      type="radio"
                      name={`m-${agent}`}
                      value={key}
                      checked={key === currentKey}
                      onChange={() =>
                        onUpdateAssignment(agent, { provider: pid, model: m.id })
                      }
                    />
                    <div>
                      <div className="id">{m.id}</div>
                      <div className="meta">
                        {p.name || pid} · {m.name ?? m.id}
                        {m.cost?.input != null || m.cost?.output != null
                          ? ` · $${m.cost.input ?? 0} / $${m.cost.output ?? 0} ${t(
                              lang,
                              "maPerMillion",
                            )}`
                          : ` · ${t(lang, "maLocalNoCost")}`}
                      </div>
                    </div>
                    {m.reasoning && <span className="chip tag">{t(lang, "mcReasoning")}</span>}
                  </label>
                );
              }),
            )}
            {Object.keys(config.providers).length === 0 && (
              <span className="ml-hint">{t(lang, "mcSelectProvider")}</span>
            )}
          </div>

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
    </div>
  );
}
