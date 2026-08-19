// Statusbar - bottom strip of the main area. Left: a success dot plus the
// number of enabled manifest services (and their ids); right: the gateway
// host (mono), the workspace-volume note, and the theme/language toggles
// (they live here so the sidebar head stays brand + refresh + collapse).
// Pure presentation - all data arrives via props, no fetches.

import type { ServiceEntry } from "./types";
import { fmt, t, type Lang } from "./i18n";
import { Icon } from "./icons";

interface Props {
  services: ServiceEntry[];
  lang: Lang;
  theme: "dark" | "light";
  onToggleTheme: () => void;
  onToggleLang: () => void;
}

export function Statusbar({ services, lang, theme, onToggleTheme, onToggleLang }: Props): JSX.Element {
  const enabled = services.filter((s) => s.enabled);
  const ids = enabled.map((s) => s.id).join(" / ");
  return (
    <footer className="statusbar" data-od-id="statusbar">
      <span className="seg">
        <span className="dot" aria-hidden="true" />
        <span>{fmt(lang, "statusAvail", enabled.length)}</span>
        {ids && (
          <>
            <span aria-hidden="true">·</span>
            <span className="mono">{ids}</span>
          </>
        )}
      </span>
      <span className="seg">
        <span className="mono">{window.location.host}</span>
        <span aria-hidden="true">·</span>
        <span>{t(lang, "statusMounted")}</span>
        <button
          className="icon-btn"
          title={theme === "dark" ? t(lang, "toLight") : t(lang, "toDark")}
          aria-label={theme === "dark" ? t(lang, "toLight") : t(lang, "toDark")}
          onClick={onToggleTheme}
        >
          <Icon name={theme === "dark" ? "sun" : "moon"} />
        </button>
        <button
          className="icon-btn"
          title={t(lang, "switchLang")}
          aria-label={t(lang, "switchLang")}
          onClick={onToggleLang}
        >
          <Icon name="globe" />
        </button>
      </span>
    </footer>
  );
}
