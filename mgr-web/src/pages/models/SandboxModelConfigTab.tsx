// SandboxModelConfigTab — a sandbox's own agent model config, "模型配置"
// (task 10-04-model-config-cc-switch Phase 4, prd R2/R3; host moved from the
// sandbox edit page's tab to the workspace's client-synthesized modelsConfig
// pane in 10-10-models-workspace-pane — mounted by panes/ModelsPane.tsx).
//
// Each sandbox owns its four agent configs end-to-end (prd D1); this tab
// loads the sandbox app's /api/models/config + /api/models/agents through
// the mgr /api/sbx/:name proxy and hands each agent block to its panel:
//   claude / codex  → SwitchAgentPanel (named snapshots, one current)
//   pi / opencode   → IncrementalAgentPanel (provider nodes + default pair)
// The tab owns the DRAFT config; panels mutate it via onChange; the save
// bar PUTs the whole config then POSTs apply for the active agent.

import { useCallback, useEffect, useState } from "react";
import {
  applySandboxAgent,
  getSandboxAgents,
  getSandboxModelConfig,
  putSandboxModelConfig,
} from "../../api";
import { t, type Lang } from "../../i18n";
import {
  AGENT_IDS,
  isSwitchAgent,
  pickAgent,
  setAgent,
  type AgentsResponse,
  type AgentId,
  type SandboxAgentConfig,
} from "./sandboxTypes";
import { SwitchAgentPanel } from "./SwitchAgentPanel";
import { IncrementalAgentPanel } from "./IncrementalAgentPanel";

interface Props {
  sandboxName: string;
  /** DB intent (running | stopped | …) — a stopped sandbox's app is
   *  unreachable, so the whole tab renders a hint instead of erroring. */
  sandboxStatus: string;
  lang: Lang;
}

export function SandboxModelConfigTab({
  sandboxName,
  sandboxStatus,
  lang,
}: Props): JSX.Element {
  const [config, setConfig] = useState<SandboxAgentConfig | null>(null);
  const [saved, setSaved] = useState<SandboxAgentConfig | null>(null);
  const [agents, setAgents] = useState<AgentsResponse | null>(null);
  const [loadErr, setLoadErr] = useState("");
  const [agent, setAgentTab] = useState<AgentId>("claude");
  const [saving, setSaving] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);

  const running = sandboxStatus === "running";

  const load = useCallback(async () => {
    try {
      const [cfg, ag] = await Promise.all([
        getSandboxModelConfig(sandboxName),
        getSandboxAgents(sandboxName),
      ]);
      setConfig(cfg);
      setSaved(cfg);
      setAgents(ag);
      setLoadErr("");
    } catch (e) {
      setLoadErr(String(e));
    }
  }, [sandboxName]);

  useEffect(() => {
    if (running) void load();
  }, [running, load]);

  /** PUT the draft config; returns "" on success (the message otherwise). */
  const saveConfig = async (): Promise<string> => {
    if (!config) return "no config";
    setSaving(true);
    setMsg(null);
    try {
      await putSandboxModelConfig(sandboxName, config);
      setSaved(config);
      return "";
    } catch (e) {
      return String(e);
    } finally {
      setSaving(false);
    }
  };

  /** Apply the ACTIVE agent after a successful save. */
  const applyActive = async (): Promise<string> => {
    try {
      const r = await applySandboxAgent(sandboxName, agent);
      // Refresh the live readback so the meta badge shows the new effective
      // provider/model without a manual reload.
      void getSandboxAgents(sandboxName)
        .then(setAgents)
        .catch(() => {});
      return r.ok ? "" : r.errors.map((e) => `${e.path}: ${e.error}`).join("; ");
    } catch (e) {
      return String(e);
    }
  };

  if (!running) {
    return (
      <div className="sbx-panel">
        <p className="muted">
          {t(lang, "sbxNeedRunning")}
          <code className="mono">{sandboxName}</code>
        </p>
      </div>
    );
  }
  if (loadErr !== "") {
    return (
      <div className="sbx-panel">
        <p className="ml-msg err">
          {t(lang, "mcLoadFailed")}
          {loadErr}
        </p>
      </div>
    );
  }
  if (!config || !agents) {
    return (
      <div className="sbx-panel">
        <p className="muted">{t(lang, "mcLoading")}</p>
      </div>
    );
  }

  const dirty = JSON.stringify(config) !== JSON.stringify(saved);
  const draftValue = pickAgent(config, agent);

  const onAgentChange = (next: typeof draftValue) => {
    setConfig(setAgent(config, agent, next));
    setMsg(null);
  };

  return (
    <div className="sbx-tab">
      {/* agent sub-tabs */}
      <div className="tabs" role="tablist">
        {AGENT_IDS.map((a) => (
          <button
            key={a}
            role="tab"
            aria-selected={agent === a}
            className={agent === a ? "active" : ""}
            onClick={() => {
              setAgentTab(a);
              setMsg(null);
            }}
          >
            {a}
            {agents[a].installed && <span className="cnt">●</span>}
          </button>
        ))}
      </div>

      {/* per-agent paradigm hint */}
      <p className="muted txs">
        {isSwitchAgent(agent) ? t(lang, "maParadigmSwitcher") : t(lang, "maParadigmIncremental")}
      </p>

      {isSwitchAgent(agent) ? (
        <SwitchAgentPanel
          sandboxName={sandboxName}
          agent={agent}
          value={draftValue as Extract<typeof draftValue, { providers: unknown[] }> | undefined}
          onChange={onAgentChange}
          status={agents[agent]}
          onSaveConfig={saveConfig}
          onApply={applyActive}
          busy={saving}
          lang={lang}
        />
      ) : (
        <IncrementalAgentPanel
          sandboxName={sandboxName}
          agent={agent}
          value={draftValue as Extract<typeof draftValue, { providers: Record<string, unknown> }> | undefined}
          onChange={onAgentChange}
          status={agents[agent]}
          onSaveConfig={saveConfig}
          onApply={applyActive}
          busy={saving}
          lang={lang}
        />
      )}

      {dirty && (
        <p className="muted txs">
          <span className="badge badge-warn">{t(lang, "mcDirty")}</span>
        </p>
      )}
      {msg && (
        <p className={`wizard-msg ${msg.ok ? "ok" : "err"}`}>{msg.text}</p>
      )}
    </div>
  );
}
