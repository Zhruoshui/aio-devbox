// SandboxTree - the workspace page's left rail (design §2.2): one node per
// registered sandbox (live-state badge, inline start button when stopped),
// whose children are that sandbox's manifest buttons fetched lazily through
// mgr's per-sandbox proxy on first expand / manual refresh.
//
// PRESENTATIONAL - all data (sandbox list, manifest cache, expanded set)
// and fetches live in WorkspacePage and flow down as props (the same split
// as web/src's App + Sidebar; state-management spec: state is local, flows
// down). Buttons are LAUNCHERS: every click opens a NEW pane instance bound
// to that sandbox (onLaunch); instances close via the tab's close icon, not
// here.
//
// Grouping flattens web/Sidebar's four groups (web / system / tui / custom)
// into one list per sandbox in that order - the workbench's grouping
// semantics are preserved (user-registered buttons appear under "custom"
// semantics = deletable rows with a trailing remove button, never in the
// web group; 09-02-web-button-ux-fix R1).
//
// Stopped sandboxes: the whole subtree is disabled (buttons grey out; the
// proxy would just 502), and the node row carries a start button (the
// existing POST /api/sandboxes/:name/start). The tree never auto-starts
// anything (D6: stopped greys out, start is explicit).

import type { ReactNode } from "react";

import { t, type Lang } from "../../i18n";
import { Icon } from "../../icons";
import type { Sandbox } from "../../types";
import { ON_DEMAND_SERVICE_IDS, type ServiceEntry } from "./types";

/** One sandbox's manifest fetch state. `services` keeps the LAST decoded
 * manifest across refreshes/errors (stale-while-revalidate), null = never
 * fetched. */
export interface ManifestState {
  status: "idle" | "loading" | "ok" | "error";
  services: ServiceEntry[] | null;
  error: string;
}

interface Props {
  lang: Lang;
  sandboxes: Sandbox[];
  manifests: Record<string, ManifestState>;
  expanded: ReadonlySet<string>;
  /** Sandbox name whose start action is in flight ("" = none). */
  starting: string;
  /** Sandbox name the tree should keep highlighted (goWorkspace). */
  focus: string | null;
  /** Rendered at the bottom of the tree column (reset-layout lives here). */
  footer?: ReactNode;
  onToggle: (name: string) => void;
  onLaunch: (sandbox: string, service: ServiceEntry) => void;
  onStart: (name: string) => void;
  onRegister: (name: string) => void;
  onDeleteButton: (sandbox: string, id: string) => void;
}

/** Live-state -> badge text key (same keys as the list page's badges). */
function liveKey(live: string): "stRunning" | "stStopped" | "stGone" | "stUnknown" {
  switch (live) {
    case "running":
      return "stRunning";
    case "stopped":
      return "stStopped";
    case "gone":
      return "stGone";
    default:
      return "stUnknown";
  }
}

/** Live-state -> badge tone class (same palette as the list page). */
function liveCls(live: string): string {
  switch (live) {
    case "running":
      return "badge-ok";
    case "stopped":
      return "badge-neutral";
    default:
      return "badge-warn";
  }
}

/** A sandbox's launchable buttons, in the workbench group order
 * (web -> tui -> custom) flattened into one list. Only ENABLED entries are
 * shown (the manifest's server-driven visibility: web buttons probe TCP,
 * agent buttons check command_exists) - with one deliberate exception:
 * ON_DEMAND services (D4: code-server) are shown even when disabled,
 * because "disabled" (nothing listening on app:8200) is exactly the state
 * the pane's start machine is FOR. type "page" entries (the sandbox's
 * modelsConfig pane) are deliberately EXCLUDED - mgr-web's own Models page
 * is that surface, and Phase 6 removes the entry from services.toml. */
function buttonsOf(services: ServiceEntry[]): ServiceEntry[] {
  const visible = services.filter(
    (s) => (s.enabled || ON_DEMAND_SERVICE_IDS.has(s.id)) && s.type !== "page",
  );
  return [
    ...visible.filter((s) => s.type === "web" && !s.deletable),
    ...visible.filter((s) => s.type === "agent" && !s.deletable),
    ...visible.filter((s) => s.deletable),
  ];
}

export function SandboxTree({
  lang,
  sandboxes,
  manifests,
  expanded,
  starting,
  focus,
  footer,
  onToggle,
  onLaunch,
  onStart,
  onRegister,
  onDeleteButton,
}: Props): JSX.Element {
  return (
    <aside className="ws-tree" aria-label={t(lang, "wsTreeLabel")}>
      <div className="sb-list">
        {sandboxes.length === 0 && <p className="sb-empty">{t(lang, "wsTreeEmpty")}</p>}
        {sandboxes.map((sb) => {
          const open = expanded.has(sb.name);
          const stopped = sb.live !== "running";
          const m = manifests[sb.name];
          const buttons = m && m.services !== null ? buttonsOf(m.services) : null;
          let hint: string | null = null;
          if (buttons === null) {
            if (m === undefined || m.status === "idle" || m.status === "loading") {
              hint = t(lang, "loading");
            } else if (m.status === "error") {
              hint = stopped
                ? t(lang, "wsStoppedHint")
                : `${t(lang, "wsTreeLoadFailed")}${m.error}`;
            }
          }
          return (
            <div key={sb.name} className={`ws-node${focus === sb.name ? " ws-focus" : ""}`}>
              <div className="ws-node-row">
                <button
                  className="ws-node-btn"
                  title={sb.name}
                  aria-expanded={open}
                  onClick={() => onToggle(sb.name)}
                >
                  <Icon name={open ? "chev-down" : "chev-r"} />
                  <span className="ws-node-name">{sb.name}</span>
                  <span className={`badge ${liveCls(sb.live)}`}>
                    <span className="dot" />
                    {t(lang, liveKey(sb.live))}
                  </span>
                </button>
                {stopped && (
                  <button
                    className={`icon-btn ws-start-btn${starting === sb.name ? " spin" : ""}`}
                    title={t(lang, "start")}
                    aria-label={`${t(lang, "start")} ${sb.name}`}
                    disabled={starting !== ""}
                    onClick={() => onStart(sb.name)}
                  >
                    <Icon name={starting === sb.name ? "refresh" : "play"} />
                  </button>
                )}
              </div>

              {open && (
                <div className="ws-children">
                  {hint !== null && <p className="sb-empty">{hint}</p>}
                  {buttons !== null && buttons.length === 0 && (
                    <p className="sb-empty">{t(lang, "sidebarEmpty")}</p>
                  )}
                  {buttons?.map((s) => (
                    <div key={s.id} className={`sb-row${stopped ? " ws-disabled" : ""}`}>
                      <button
                        className="launch-btn"
                        title={`${s.label}@${sb.name}${t(lang, "openInstanceSuffix")}`}
                        disabled={stopped}
                        onClick={() => onLaunch(sb.name, s)}
                      >
                        <Icon name={serviceIcon(s.id, s.type)} />
                        <span className="launch-label">{s.label}</span>
                      </button>
                      {s.deletable && !stopped && (
                        <button
                          className="del-btn"
                          title={`${t(lang, "removePrefix")}${s.label}`}
                          aria-label={`${t(lang, "removePrefix")}${s.label}`}
                          onClick={() => onDeleteButton(sb.name, s.id)}
                        >
                          <Icon name="x" />
                        </button>
                      )}
                    </div>
                  ))}
                  <div className="sb-row">
                    <button
                      className="launch-btn ws-register-btn"
                      disabled={stopped}
                      title={t(lang, "register")}
                      onClick={() => onRegister(sb.name)}
                    >
                      <Icon name="plus" />
                      <span className="launch-label">{t(lang, "register")}</span>
                    </button>
                  </div>
                </div>
              )}
            </div>
          );
        })}
      </div>
      {footer}
    </aside>
  );
}

/** Tree glyph for a manifest service: known ids get a specific glyph,
 * everything else falls back to a semantic icon by type (web -> browser,
 * page -> sliders, agent -> terminal). Same mapping as the tab glyphs in
 * gl-kumo.css (WorkspacePage patches data-icon with these names). */
export function serviceIcon(
  id: string,
  type: "web" | "agent" | "page",
): "code" | "browser" | "terminal" | "chat" | "sliders" {
  switch (id) {
    case "codeServer":
      return "code";
    case "vnc":
      return "browser";
    case "terminal":
      return "terminal";
    case "opencode":
      return "chat";
    case "modelsConfig":
      return "sliders";
    default:
      return type === "web" ? "browser" : type === "page" ? "sliders" : "terminal";
  }
}
