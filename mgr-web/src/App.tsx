// App - mgr-web shell.
//
// Owns the sidebar navigation (workspace / sandboxes / images / models /
// usage), theme scheme + language state (same persistence pattern as the
// workbench SPA in web/src/App.tsx, with mgr-prefixed localStorage keys;
// the scheme comes from the themes.ts registry and drives BOTH <html
// data-theme> and <html data-mode>) and the current sub-view of the
// sandboxes page. No state library and no router
// library: views are a discriminated union in local state (state-management
// spec: state is local, flows down).
//
//   page "workspace":  the golden-layout workspace (WorkspacePage) - the
//                     DEFAULT landing page (unified UI, prd D2); "进入沙箱"
//                     on the list page switches here focused on that sandbox
//   page "sandboxes": list | create (wizard) | adopt (wizard) | edit:<name>
//                    | job:<id>
//   page "images":    the image registry table
//   page "models":    model config (port of web/src/panes/models, Phase 4c)
//   page "usage":     multi-sandbox usage over GET /api/usage
//
// A golden-layout popout child window (opened from the workspace, same
// mgr.localhost origin) renders as a LONE workspace without this shell -
// the parent's BrowserPopout writes the child's layout under the gl-window
// URL param, which WorkspacePage consumes at module load (IS_POPOUT_CHILD).
//
// Long operations (create/edit/delete) return a job id; the view then
// switches to <JobView> which polls GET /api/jobs/:id and returns to the
// list when the job settles. Adopt is synchronous - it returns to the list
// directly.

import { useEffect, useRef, useState, type RefObject } from "react";

import { t, type Lang, type StringKey } from "./i18n";
import { Icon, IconSprite, type IconName } from "./icons";
import { SandboxListPage } from "./pages/SandboxListPage";
import { AdoptPage } from "./pages/AdoptPage";
import { CreatePage } from "./pages/CreatePage";
import { EditPage } from "./pages/EditPage";
import { ImagesPage } from "./pages/ImagesPage";
import { JobView } from "./pages/JobView";
import { ModelsPage } from "./pages/models/ModelsPage";
import { UsagePage } from "./pages/UsagePage";
import { IS_POPOUT_CHILD, WorkspacePage } from "./pages/WorkspacePage";
import { THEMES, resolveThemeKey, type ThemeDef } from "./themes";
import type { Scenario } from "./types";
import "./styles.css";

type Page = "workspace" | "sandboxes" | "images" | "models" | "usage";

/** Sub-view of the sandboxes page. Job views carry the job id plus which
 * flow produced them (for the right heading + done message). */
export type SbxView =
  | { view: "list" }
  | { view: "create" }
  | { view: "adopt" }
  | { view: "edit"; name: string }
  | { view: "job"; jobId: number; flow: "create" | "recreate" | "delete" };

const THEME_KEY = "mgr.theme";
const LANG_KEY = "mgr.lang";
/**
 * Prototype redesign (09-11): the 216px sidebar is replaced by a 48px rail
 * + a workspace-only side panel. The panel's hidden state is persisted under
 * mgr.panelHidden (the prototype's own key). The old S5/R1
 * mgr.sidebarCollapsed key is retired — its state is subsumed by the panel.
 */
const PANEL_HIDDEN_KEY = "mgr.panelHidden";

/** Rail navigation: matches the prototype's mgr-shell.js NAV array
 * (workspace/grid, sandboxes/terminal, images/layers, models/sliders,
 * usage/chart). Each entry carries its i18n key (keyof Strings). */
const RAIL_NAV: { page: Page; icon: IconName; labelKey: StringKey }[] = [
  { page: "workspace", icon: "grid", labelKey: "navWorkspace" },
  { page: "sandboxes", icon: "terminal", labelKey: "navSandboxes" },
  { page: "images", icon: "layers", labelKey: "navImages" },
  { page: "models", icon: "sliders", labelKey: "navModels" },
  { page: "usage", icon: "chart", labelKey: "navUsage" },
];

export function App(): JSX.Element {
  // The workspace is the default landing page: the sandbox manager is now
  // the single UI (prd D1/D2), the admin pages are secondary destinations.
  const [page, setPage] = useState<Page>("workspace");
  const [sbxView, setSbxView] = useState<SbxView>({ view: "list" });
  // Sandbox the workspace should expand + highlight (goWorkspace from the
  // list page's "进入" button); null = plain workspace landing.
  const [wsFocus, setWsFocus] = useState<string | null>(null);
  // Theme scheme (09-20-theme-schemes): a full ThemeDef from the registry,
  // not a binary light/dark. resolveThemeKey maps any legacy/retired
  // THEME_KEY value ("light"/"dark"/"kumo-*"/"catppuccin-mocha") to the
  // tokyo-night default.
  const [theme, setTheme] = useState<ThemeDef>(() =>
    resolveThemeKey(localStorage.getItem(THEME_KEY)),
  );
  const [lang, setLang] = useState<Lang>(
    () => (localStorage.getItem(LANG_KEY) === "en" ? "en" : "zh-CN"),
  );
  // Rail theme picker (open menu + its trigger rect for fixed positioning).
  const [themeMenu, setThemeMenu] = useState<DOMRect | null>(null);
  const themeBtnRef = useRef<HTMLButtonElement>(null);
  // Prototype redesign: the sandbox-tree side panel (workspace only).
  const [panelHidden, setPanelHidden] = useState<boolean>(
    () => localStorage.getItem(PANEL_HIDDEN_KEY) === "1",
  );
  // Scenario catalog, fetched once for the create wizard / env editor.
  const [scenarios, setScenarios] = useState<Scenario[] | null>(null);

  // Apply + persist theme (09-20-theme-schemes): <html data-theme> drives
  // the scheme's token block in styles.css; <html data-mode> is derived
  // from the scheme's static mode and keeps color-scheme + the legacy
  // [data-mode] hooks working. XtermPane's MutationObserver watches both
  // attributes for live retint.
  useEffect(() => {
    document.documentElement.dataset.theme = theme.key;
    document.documentElement.dataset.mode = theme.mode;
    localStorage.setItem(THEME_KEY, theme.key);
  }, [theme]);

  // Apply + persist language (html lang for assistive tech).
  useEffect(() => {
    document.documentElement.lang = lang;
    localStorage.setItem(LANG_KEY, lang);
  }, [lang]);

  // Prototype redesign: persist the workspace panel's hidden state.
  useEffect(() => {
    localStorage.setItem(PANEL_HIDDEN_KEY, panelHidden ? "1" : "0");
  }, [panelHidden]);

  /** Toggle the workspace sandbox-tree panel. The rail's "workspace" button
   * flip-flops this when already on the workspace page (prototype
   * workspace.html wsNav behavior); the panel head's own collapse button was
   * removed (R5 — the tree's collapse button covers it). */
  const togglePanel = () => setPanelHidden((h) => !h);
  /** Rail "workspace" click: if already on workspace, toggle the panel;
   * otherwise navigate (and reveal the panel). */
  const goOrToggleWorkspace = () => {
    if (page === "workspace") {
      togglePanel();
    } else {
      setWsFocus(null);
      setPage("workspace");
      setPanelHidden(false);
    }
  };

  const nav = (p: Page) => {
    setPage(p);
    if (p === "workspace") setWsFocus(null);
    if (p === "sandboxes") setSbxView({ view: "list" });
  };

  /** "进入沙箱" (D2): switch to the workspace focused on that sandbox. */
  const goWorkspace = (name: string) => {
    setWsFocus(name);
    setPage("workspace");
  };

  // Golden-layout popout child: a lone workspace, no admin shell (the child
  // carries its whole layout in the gl-window param; see WorkspacePage).
  if (IS_POPOUT_CHILD) {
    return (
      <div className="app app-popout">
        <IconSprite />
        <WorkspacePage lang={lang} focus={null} onManage={() => setPage("sandboxes")} />
      </div>
    );
  }

  return (
    <div className={`app${panelHidden ? " panel-hidden" : ""}`}>
      <IconSprite />
      <nav className="rail" aria-label={t(lang, "brand")}>
        <a
          className="rail-brand"
          href="#"
          onClick={(e) => {
            e.preventDefault();
            nav("workspace");
          }}
          title={t(lang, "brand")}
          aria-label={t(lang, "brand")}
        >
          <Icon name="cube" large />
        </a>
        <div className="rail-nav">
          {RAIL_NAV.map(({ page: p, icon, labelKey }) => {
            const active = page === p;
            const label = t(lang, labelKey);
            const onClick = p === "workspace" ? goOrToggleWorkspace : () => nav(p);
            return (
              <button
                key={p}
                className={`rail-btn${active ? " active" : ""}`}
                data-tip={label}
                aria-label={label}
                aria-current={active ? "page" : undefined}
                onClick={onClick}
              >
                <Icon name={icon} />
              </button>
            );
          })}
        </div>
        <div className="rail-foot">
          <button
            ref={themeBtnRef}
            className="rail-btn"
            data-tip={t(lang, "themePick")}
            aria-label={t(lang, "themePick")}
            aria-haspopup="menu"
            aria-expanded={themeMenu !== null}
            onClick={() =>
              setThemeMenu((open) =>
                open === null && themeBtnRef.current
                  ? themeBtnRef.current.getBoundingClientRect()
                  : null,
              )
            }
          >
            <Icon name="palette" />
          </button>
          <button
            className="rail-btn"
            data-tip={t(lang, "switchLang")}
            aria-label={t(lang, "switchLang")}
            onClick={() => setLang((l) => (l === "zh-CN" ? "en" : "zh-CN"))}
          >
            <Icon name="globe" />
          </button>
        </div>
      </nav>
      <main className={`main${page !== "workspace" ? " scroll" : ""}`}>
        {page === "workspace" ? (
          <WorkspacePage
            lang={lang}
            focus={wsFocus}
            onManage={() => nav("sandboxes")}
            onEditSandbox={(name) => {
              setSbxView({ view: "edit", name });
              setPage("sandboxes");
            }}
          />
        ) : page === "sandboxes" ? (
          <SandboxesPage
            view={sbxView}
            onView={setSbxView}
            lang={lang}
            scenarios={scenarios}
            onScenarios={setScenarios}
            onEnter={goWorkspace}
          />
        ) : page === "images" ? (
          <ImagesPage lang={lang} />
        ) : page === "models" ? (
          <ModelsPage
            lang={lang}
            onGoWorkspace={goWorkspace}
            onGoList={() => nav("sandboxes")}
          />
        ) : (
          <UsagePage lang={lang} />
        )}
      </main>
      {themeMenu !== null && (
        <ThemeMenu
          lang={lang}
          anchor={themeMenu}
          triggerRef={themeBtnRef}
          current={theme}
          onPick={(def) => {
            setTheme(def);
            setThemeMenu(null);
            themeBtnRef.current?.focus();
          }}
          onClose={() => {
            setThemeMenu(null);
            themeBtnRef.current?.focus();
          }}
        />
      )}
    </div>
  );
}

// ── theme picker (09-20-theme-schemes, design.md §2) ───────────────

/** The rail palette button's dropdown: one entry per registered scheme with
 * a 5-dot swatch preview (accent / fg / chart-1 / chart-2 / surface), the
 * i18n label and a check on the active theme. Click picks immediately (no
 * "apply" step). Same dismissal contract as NodeMenu: Escape and outside
 * pointerdown close, focus returns to the opener — with one addition: the
 * trigger button is EXEMPT from outside-close, so its own click toggles
 * the menu instead of close-then-instantly-reopen. Fixed-positioned (the
 * .menu precedent) so the 48px rail can't clip it. */
function ThemeMenu({
  lang,
  anchor,
  triggerRef,
  current,
  onPick,
  onClose,
}: {
  lang: Lang;
  anchor: DOMRect;
  triggerRef: RefObject<HTMLButtonElement | null>;
  current: ThemeDef;
  onPick: (def: ThemeDef) => void;
  onClose: () => void;
}): JSX.Element {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") onClose();
    };
    const onPointerDown = (e: PointerEvent): void => {
      const target = e.target as Node;
      if (triggerRef.current?.contains(target)) return; // trigger toggles via its own onClick
      if (ref.current !== null && !ref.current.contains(target)) onClose();
    };
    document.addEventListener("keydown", onKey);
    document.addEventListener("pointerdown", onPointerDown);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("pointerdown", onPointerDown);
    };
  }, [onClose, triggerRef]);

  // Fixed position clamped to the viewport (NodeMenu pattern; the menu is
  // ~200px wide). Height scales with the registry so every theme fits
  // without zooming out; on short viewports .theme-menu's max-height +
  // overflow-y take over and the list scrolls.
  const menuH = THEMES.length * 34 + 24; // rows + padding + scrollbar slack
  const left = Math.min(anchor.right + 4, window.innerWidth - 212);
  const top = Math.max(8, Math.min(anchor.top, window.innerHeight - menuH - 8));

  return (
    <div ref={ref} className="menu theme-menu open" role="menu" style={{ position: "fixed", left, top }}>
      {THEMES.map((def) => (
        <button
          key={def.key}
          role="menuitemradio"
          aria-checked={def.key === current.key}
          className="theme-menu-item"
          onClick={() => onPick(def)}
        >
          <span className="theme-swatch" aria-hidden="true">
            {def.swatch.map((c, i) => (
              <span key={i} style={{ background: c }} />
            ))}
          </span>
          <span className="theme-menu-label">{t(lang, def.labelKey)}</span>
          {def.key === current.key && <Icon name="check" />}
        </button>
      ))}
    </div>
  );
}

// ── sandboxes page dispatcher ─────────────────────────────────────

function SandboxesPage({
  view,
  onView,
  lang,
  scenarios,
  onScenarios,
  onEnter,
}: {
  view: SbxView;
  onView: (v: SbxView) => void;
  lang: Lang;
  scenarios: Scenario[] | null;
  onScenarios: (s: Scenario[]) => void;
  /** Switch to the workspace focused on this sandbox (进入, D2). */
  onEnter: (name: string) => void;
}): JSX.Element {
  switch (view.view) {
    case "list":
      return (
        <SandboxListPage
          lang={lang}
          onEnter={onEnter}
          onCreate={() => onView({ view: "create" })}
          onAdopt={() => onView({ view: "adopt" })}
          onEdit={(name) => onView({ view: "edit", name })}
          onJob={(jobId, flow) => onView({ view: "job", jobId, flow })}
        />
      );
    case "create":
      return (
        <CreatePage
          lang={lang}
          scenarios={scenarios}
          onScenarios={onScenarios}
          onCancel={() => onView({ view: "list" })}
          onSubmitted={(jobId) => onView({ view: "job", jobId, flow: "create" })}
        />
      );
    case "adopt":
      return (
        <AdoptPage
          lang={lang}
          onCancel={() => onView({ view: "list" })}
          onAdopted={() => onView({ view: "list" })}
        />
      );
    case "edit":
      return (
        <EditPage
          name={view.name}
          lang={lang}
          scenarios={scenarios}
          onScenarios={onScenarios}
          onCancel={() => onView({ view: "list" })}
          onSubmitted={(jobId) => onView({ view: "job", jobId, flow: "recreate" })}
        />
      );
    case "job":
      return (
        <JobView
          jobId={view.jobId}
          flow={view.flow}
          lang={lang}
          onBack={() => onView({ view: "list" })}
        />
      );
  }
}
