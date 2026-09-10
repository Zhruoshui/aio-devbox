// MgrNotice — the strip shown at the top of every agent tab on the models
// page. mgr edits the SHARED canonical library of the SELECTED profile
// (assigned sandboxes pull it on their own schedule); agent install status
// and native config files are per-sandbox local state, so the strip points
// at each running sandbox in the WORKSPACE (unified Phase 4, design §4.4:
// the old "去沙箱工作台" new-tab links became workspace navigation — no more
// per-sandbox workbench SPA).

import { Icon } from "../../icons";
import { t, type Lang } from "../../i18n";

/** One entry: running mgr sandboxes only (a link into a stopped sandbox's
 * panes is worse than no link). */
export interface SandboxLink {
  name: string;
}

export function MgrNotice({
  links,
  lang,
  onGoWorkspace,
}: {
  links: SandboxLink[];
  lang: Lang;
  /** Navigate to the workspace focused on that sandbox (App's goWorkspace;
   * absent = render names without navigation). */
  onGoWorkspace?: (name: string) => void;
}): JSX.Element {
  return (
    <div className="ml-mgr-note">
      <Icon name="cube" />
      <span>{t(lang, "maMgrNotice")}</span>
      {links.map((l) =>
        onGoWorkspace ? (
          <button key={l.name} className="ml-chip" type="button" onClick={() => onGoWorkspace(l.name)}>
            {l.name}
          </button>
        ) : (
          <span key={l.name} className="ml-chip">
            {l.name}
          </span>
        ),
      )}
    </div>
  );
}
