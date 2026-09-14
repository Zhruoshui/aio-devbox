// App - mgr-web shell.
//
// Owns the sidebar navigation (workspace / sandboxes / images / models /
// usage), theme + language state (same persistence pattern as the workbench
// SPA in web/src/App.tsx, with mgr-prefixed localStorage keys) and the
// current sub-view of the sandboxes page. No state library and no router
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

import { useEffect, useState } from "react";

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
import type { Scenario } from "./types";
import "./styles.css";

type Theme = "dark" | "light";
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
  const [theme, setTheme] = useState<Theme>(
    () => (localStorage.getItem(THEME_KEY) === "light" ? "light" : "dark"),
  );
  const [lang, setLang] = useState<Lang>(
    () => (localStorage.getItem(LANG_KEY) === "en" ? "en" : "zh-CN"),
  );
  // Prototype redesign: the sandbox-tree side panel (workspace only).
  const [panelHidden, setPanelHidden] = useState<boolean>(
    () => localStorage.getItem(PANEL_HIDDEN_KEY) === "1",
  );
  // Scenario catalog, fetched once for the create wizard / env editor.
  const [scenarios, setScenarios] = useState<Scenario[] | null>(null);

  // Apply + persist theme (data-mode is Kumo's native mode hook).
  useEffect(() => {
    document.documentElement.dataset.mode = theme;
    localStorage.setItem(THEME_KEY, theme);
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
   * workspace.html wsNav behavior); the panel's own head button too. */
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
            className="rail-btn"
            data-tip={theme === "dark" ? t(lang, "toLight") : t(lang, "toDark")}
            aria-label={theme === "dark" ? t(lang, "toLight") : t(lang, "toDark")}
            onClick={() => setTheme((m) => (m === "dark" ? "light" : "dark"))}
          >
            <Icon name={theme === "dark" ? "sun" : "moon"} />
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
            panelOnToggle={togglePanel}
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
