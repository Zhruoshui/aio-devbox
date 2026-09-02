// Sidebar - left rail of LAUNCHER buttons for each enabled service, grouped
// (Web tools / Terminals & agents / Custom), plus theme + language toggles, a
// manual refresh, and the register-button opener. Collapsible to an icon rail.
//
// Buttons are launchers: every click creates a NEW instance (tab) in the
// workspace; clicking again creates another. Closing instances happens via
// the tab's close icon, not the sidebar. Registration happens in the modal
// RegisterDialog (App owns its open state).

import { useState } from "react";

import type { ServiceEntry } from "./types";
import { t, type Lang } from "./i18n";
import { Icon, serviceIcon } from "./icons";

interface Props {
  services: ServiceEntry[];
  collapsed: boolean;
  lang: Lang;
  onToggleCollapse: () => void;
  onLaunch: (service: ServiceEntry) => void;
  onRefresh: () => void;
  onOpenRegister: () => void;
  onDelete: (id: string) => void;
}

export function Sidebar({
  services,
  collapsed,
  lang,
  onToggleCollapse,
  onLaunch,
  onRefresh,
  onOpenRegister,
  onDelete,
}: Props): JSX.Element {
  // Refresh affordance state: spin + disable for a short window (the manifest
  // refetch is fire-and-forget upstream; this mirrors the Kumo reference's
  // labeled-spinner guidance without a promise contract change).
  const [refreshing, setRefreshing] = useState(false);

  const enabled = services.filter((s) => s.enabled);
  const groups: Array<{ key: string; label: string; items: ServiceEntry[] }> = [
    {
      key: "web",
      label: t(lang, "groupWeb"),
      // User-registered buttons (deletable) belong to the "custom" group
      // only - without the exclusion every user web button would show up
      // here AND there (09-02-web-button-ux-fix R1).
      items: enabled.filter((s) => s.type === "web" && !s.deletable),
    },
    {
      key: "page",
      label: t(lang, "groupSystem"),
      items: enabled.filter((s) => s.type === "page"),
    },
    {
      key: "tui",
      label: t(lang, "groupTui"),
      items: enabled.filter((s) => s.type === "agent" && !s.deletable),
    },
    {
      key: "custom",
      label: t(lang, "groupCustom"),
      items: enabled.filter((s) => s.deletable),
    },
  ];

  const refresh = () => {
    if (refreshing) return;
    onRefresh();
    setRefreshing(true);
    window.setTimeout(() => setRefreshing(false), 900);
  };

  return (
    <aside
      className={`sidebar${collapsed ? " collapsed" : ""}`}
      aria-label={t(lang, "brand")}
    >
      <div className="sb-head">
        <div className="sb-brand">
          <Icon name="cube" large />
          <span className="sb-title">{t(lang, "brand")}</span>
        </div>
        <button
          className="icon-btn refresh-btn"
          title={t(lang, "refresh")}
          aria-label={t(lang, "refresh")}
          disabled={refreshing}
          onClick={refresh}
        >
          <Icon name="refresh" />
        </button>
        <button
          className="icon-btn expand-btn"
          title={collapsed ? t(lang, "expand") : t(lang, "collapse")}
          aria-label={collapsed ? t(lang, "expand") : t(lang, "collapse")}
          aria-expanded={!collapsed}
          onClick={onToggleCollapse}
        >
          <Icon name={collapsed ? "chev-r" : "chev-l"} />
        </button>
      </div>

      <nav className="sb-list" aria-label={t(lang, "brand")}>
        {groups.map((g) =>
          g.items.length === 0 ? null : (
            <div key={g.key}>
              <p className="sb-group-label">{g.label}</p>
              {g.items.map((s) => (
                <div key={s.id} className="sb-row">
                  <button
                    className="launch-btn"
                    title={`${s.label}${t(lang, "openInstanceSuffix")}`}
                    aria-label={`${s.label}${t(lang, "openInstanceSuffix")}`}
                    onClick={() => onLaunch(s)}
                  >
                    <Icon name={serviceIcon(s.id, s.type)} />
                    <span className="launch-label">{s.label}</span>
                  </button>
                  {s.deletable && (
                    <button
                      className="del-btn"
                      title={`${t(lang, "removePrefix")}${s.label}`}
                      aria-label={`${t(lang, "removePrefix")}${s.label}`}
                      onClick={() => onDelete(s.id)}
                    >
                      <Icon name="x" />
                    </button>
                  )}
                </div>
              ))}
            </div>
          ),
        )}
        {enabled.length === 0 && <p className="sb-empty">{t(lang, "sidebarEmpty")}</p>}
      </nav>

      <div className="sb-foot">
        <button
          className="register-btn"
          title={t(lang, "register")}
          data-od-id="register-button"
          onClick={() => (collapsed ? onToggleCollapse() : onOpenRegister())}
        >
          <Icon name="plus" />
          <span className="btn-text">{t(lang, "register")}</span>
        </button>
      </div>
    </aside>
  );
}
