// MgrNotice — the strip shown at the top of every agent tab on the models
// page. mgr edits the SHARED canonical library (sandboxes pull it on their
// own schedule); agent install status and native config files are per-sandbox
// local state, so the strip points at each running sandbox's workbench
// (entry_url opens sbx-<name>.mgr.localhost in a new tab, D10).

import { Icon } from "../../icons";
import { t, type Lang } from "../../i18n";

/** One entry link: running mgr sandboxes only (an entry_url that 502s is
 * worse than no link). */
export interface SandboxLink {
  name: string;
  entryUrl: string;
}

export function MgrNotice({
  links,
  lang,
}: {
  links: SandboxLink[];
  lang: Lang;
}): JSX.Element {
  return (
    <div className="ml-mgr-note">
      <Icon name="cube" />
      <span>{t(lang, "maMgrNotice")}</span>
      {links.map((l) => (
        <a
          key={l.name}
          className="ml-chip"
          href={l.entryUrl}
          target="_blank"
          rel="noopener noreferrer"
        >
          {l.name}
        </a>
      ))}
    </div>
  );
}
