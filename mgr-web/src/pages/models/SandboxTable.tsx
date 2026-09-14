// SandboxTable — the agent tabs' right column (09-11 prototype
// models.html sandboxTable()): every sandbox's row against the SELECTED
// profile + the one agent this tab edits.
//
// Row states (prototype semantics):
//   - mine (model_profile === profileId): a `.switch` checkbox for this
//     agent's membership in the assignment subset (`model_agents`; null =
//     ALL four agents — decoded before toggling). Toggling PUTs
//     /api/sandboxes/:name/model_profile immediately (the sandbox pulls the
//     result within ~1min); the label reads 生效 / 保留本地.
//   - other profile: muted row + "切到 <name>" button (switches the page's
//     selected profile — no data change).
//   - unassigned: "—" + "去指派" link to the sandbox list page (the
//     quick-assign popover lives there).
//
// Data: full GET /api/sandboxes list passed down from ModelsPage (refreshed
// on tab shows); profiles carry display names for the Profile column.

import { t, type Lang } from "../../i18n";
import type { Sandbox } from "../../types";
import type { AgentTab } from "./types";
import type { SandboxLink } from "./MgrNotice";

const AGENT_LABEL: Record<AgentTab, string> = {
  pi: "pi",
  opencode: "opencode",
  claude: "Claude Code",
  codex: "Codex",
};

/** ALL four agent keys — a null model_agents decodes to this set. */
const ALL_AGENTS = ["pi", "opencode", "claude", "codex"];

export function SandboxTable({
  agent,
  profileId,
  profileNames,
  sandboxList,
  sbxBusy,
  onGoWorkspace,
  onGoList,
  onGoProfile,
  onToggleSandboxAgent,
  lang,
}: {
  agent: AgentTab;
  /** Selected profile id (mine test). */
  profileId: string;
  /** id → display name for other-profile rows' 切到 buttons. */
  profileNames: Record<string, string>;
  sandboxList: Sandbox[] | null;
  /** Sandbox name with a toggle in flight (disables every switch). */
  sbxBusy: string;
  onGoWorkspace?: (name: string) => void;
  onGoList?: () => void;
  onGoProfile: (id: string) => void;
  onToggleSandboxAgent: (name: string, agent: string, on: boolean) => void;
  lang: Lang;
}): JSX.Element {
  const rows = sandboxList ?? [];

  return (
    <div className="card assign">
      <h3>{t(lang, "maEffectiveSandboxes")}</h3>
      {rows.length === 0 ? (
        <span className="ml-hint">{t(lang, "maNoSandboxes")}</span>
      ) : (
        <table className="table sbx-tbl">
          <thead>
            <tr>
              <th>{t(lang, "muColSandbox")}</th>
              <th>Profile</th>
              <th>{AGENT_LABEL[agent]}</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {rows.map((s) => {
              const mine = s.model_profile === profileId;
              const on =
                mine &&
                (s.model_agents ?? ALL_AGENTS).includes(agent);
              const running = s.live === "running";
              return (
                <tr key={s.name} className={mine ? undefined : "other"}>
                  <td>{s.name}</td>
                  <td>
                    {!s.model_profile ? (
                      <span className="muted txs">{t(lang, "mpUnassigned")}</span>
                    ) : mine ? (
                      <span className="badge badge-neutral">
                        {profileNames[s.model_profile] ?? s.model_profile}
                      </span>
                    ) : (
                      <span className="muted txs">
                        {profileNames[s.model_profile] ?? s.model_profile}
                      </span>
                    )}
                  </td>
                  <td>
                    {mine ? (
                      <label
                        style={{ display: "inline-flex", alignItems: "center", gap: 8 }}
                      >
                        <input
                          className="switch"
                          type="checkbox"
                          checked={on}
                          disabled={sbxBusy !== ""}
                          aria-label={`${s.name} ${AGENT_LABEL[agent]}`}
                          onChange={(e) =>
                            onToggleSandboxAgent(s.name, agent, e.target.checked)
                          }
                        />
                        <span className="txs">
                          {on ? t(lang, "maAgentOn") : t(lang, "maAgentLocal")}
                        </span>
                      </label>
                    ) : s.model_profile ? (
                      <span className="muted txs">{t(lang, "maOtherProfile")}</span>
                    ) : (
                      <span className="muted txs">—</span>
                    )}
                  </td>
                  <td className="r">
                    {s.model_profile && !mine ? (
                      <button
                        className="btn btn-ghost btn-sm"
                        onClick={() => onGoProfile(s.model_profile as string)}
                      >
                        {t(lang, "maSwitchToProfile").replace(
                          "{name}",
                          profileNames[s.model_profile] ?? s.model_profile,
                        )}
                      </button>
                    ) : !s.model_profile ? (
                      <button
                        className="btn btn-ghost btn-sm"
                        onClick={() => onGoList?.()}
                        disabled={!onGoList}
                      >
                        {t(lang, "maGoAssign")}
                      </button>
                    ) : onGoWorkspace ? (
                      <button
                        className="btn btn-ghost btn-sm"
                        aria-disabled={!running}
                        title={running ? undefined : t(lang, "wsStoppedHint")}
                        onClick={() => onGoWorkspace(s.name)}
                      >
                        {t(lang, "maOpenWorkspace")}
                      </button>
                    ) : null}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
      <p className="txs muted" style={{ margin: 0 }}>
        {t(lang, "maSbxTblNote")}
      </p>
    </div>
  );
}

/** Running-sandbox links for the tab's MgrNotice (derived from the same
 * list the table renders — no second fetch). */
export function runningLinks(sandboxList: Sandbox[] | null): SandboxLink[] {
  return (sandboxList ?? [])
    .filter((s) => s.live === "running")
    .map((s) => ({ name: s.name }));
}
