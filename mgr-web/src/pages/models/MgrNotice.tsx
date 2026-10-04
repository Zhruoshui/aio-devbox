// MgrNotice — the strip at the top of the models page, explaining the
// cc-switch model-config split (task 10-04-model-config-cc-switch Phase 3):
// mgr keeps ONLY the provider library (a shared template source); each
// sandbox owns its agent configs end-to-end. There is no profile system
// anymore and no per-sandbox assignment — so no "go to workspace" chips.

import { Icon } from "../../icons";
import { t, type Lang } from "../../i18n";

export function MgrNotice({ lang }: { lang: Lang }): JSX.Element {
  return (
    <div className="ml-mgr-note">
      <Icon name="cube" />
      <span>{t(lang, "mcMgrNotice")}</span>
    </div>
  );
}
