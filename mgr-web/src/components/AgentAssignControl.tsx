// AgentAssignControl — the S2 agent-subset checkboxes (pi/claude/codex/
// opencode, fixed order) shared by EditPage and the SandboxListPage profile
// chip popover (design §4.2). The value is the wire shape verbatim:
//   null  = all agents (legacy rows / older backends — displayed all-on)
//   []    = zero agents (the sandbox's pull keeps every local config)
//   [...] = the explicit subset
// null only ever arrives from a payload; unchecking the LAST box emits []
// (never null — null would silently re-widen to all four on save).

import { t, type Lang } from "../i18n";

/** The four agents in fixed display order (design §4.2). Keys are the wire
 * values (mgr routes.rs VALID_AGENTS) - load-bearing, never localize them. */
export const ASSIGN_AGENTS = ["pi", "claude", "codex", "opencode"] as const;

/** Agent display label (matches the models page tab labels). */
export function agentLabel(agent: string): string {
  switch (agent) {
    case "pi":
      return "pi";
    case "claude":
      return "Claude";
    case "codex":
      return "Codex";
    case "opencode":
      return "opencode";
    default:
      return agent;
  }
}

/** Compact subset summary for chips: null (all) = "", a subset = "pi+2"
 * style (first entry + the count of the rest), zero = "∅". */
export function agentSubsetSummary(agents: string[] | null): string {
  if (agents === null) return "";
  if (agents.length === 0) return "∅";
  return agents.length === 1 ? agentLabel(agents[0]) : `${agentLabel(agents[0])}+${agents.length - 1}`;
}

export function AgentAssignControl({
  lang,
  value,
  onChange,
  disabled,
  grid,
}: {
  lang: Lang;
  /** null = all agents (all boxes checked, locked semantics "all"); an
   * array is the explicit subset. */
  value: string[] | null;
  onChange: (agents: string[] | null) => void;
  disabled?: boolean;
  /** 09-11 prototype compact variant: the 2-column `.agents` grid used by
   * the sandbox-card quick-assign popover (sandbox-list.html), instead of
   * the editor page's scn-list rows. */
  grid?: boolean;
}): JSX.Element {
  // null renders as every box checked. The FIRST interaction on a null value
  // materializes the explicit all-four subset, so unchecking one box of a
  // null value yields the other three (not "all minus that one" ambiguity).
  const checked = (agent: string): boolean =>
    value === null ? true : value.includes(agent);

  const toggle = (agent: string, on: boolean): void => {
    const base = value === null ? [...ASSIGN_AGENTS] : [...value];
    const next = on
      ? base.includes(agent)
        ? base
        : [...base, agent]
      : base.filter((a) => a !== agent);
    onChange(next);
  };

  if (grid) {
    return (
      <div className="agents" role="group" aria-label={t(lang, "mpAgents")}>
        {ASSIGN_AGENTS.map((agent) => (
          <label key={agent}>
            <input
              className="check"
              type="checkbox"
              checked={checked(agent)}
              disabled={disabled}
              aria-label={agentLabel(agent)}
              onChange={(e) => toggle(agent, e.target.checked)}
            />
            <span>{agentLabel(agent)}</span>
          </label>
        ))}
      </div>
    );
  }

  return (
    <div className="scn-list" role="group" aria-label={t(lang, "mpAgents")}>
      {ASSIGN_AGENTS.map((agent) => (
        <label key={agent} className="scn-row" style={{ border: 0, padding: 0, background: "transparent" }}>
          <input
            className="check"
            type="checkbox"
            checked={checked(agent)}
            disabled={disabled}
            aria-label={agentLabel(agent)}
            onChange={(e) => toggle(agent, e.target.checked)}
          />
          <span className="scn-name">
            <span>{agentLabel(agent)}</span>
          </span>
        </label>
      ))}
    </div>
  );
}
