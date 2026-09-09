// App - mgr-web shell.
//
// Owns the sidebar navigation (sandboxes / images), theme + language state
// (same persistence pattern as the workbench SPA in web/src/App.tsx, with
// mgr-prefixed localStorage keys) and the current sub-view of the sandboxes
// page. No state library and no router library: views are a discriminated
// union in local state (state-management spec: state is local, flows down).
//
//   page "sandboxes": list | create (wizard) | edit:<name> | job:<id>
//   page "images":    the image registry table
//
// Long operations (create/edit/delete) return a job id; the view then
// switches to <JobView> which polls GET /api/jobs/:id and returns to the
// list when the job settles.

import { useEffect, useState } from "react";

import { t, type Lang } from "./i18n";
import { Icon, IconSprite } from "./icons";
import { SandboxListPage } from "./pages/SandboxListPage";
import { CreatePage } from "./pages/CreatePage";
import { EditPage } from "./pages/EditPage";
import { ImagesPage } from "./pages/ImagesPage";
import { JobView } from "./pages/JobView";
import type { Scenario } from "./types";
import "./styles.css";

type Theme = "dark" | "light";
type Page = "sandboxes" | "images";

/** Sub-view of the sandboxes page. Job views carry the job id plus which
 * flow produced them (for the right heading + done message). */
export type SbxView =
  | { view: "list" }
  | { view: "create" }
  | { view: "edit"; name: string }
  | { view: "job"; jobId: number; flow: "create" | "recreate" | "delete" };

const THEME_KEY = "mgr.theme";
const LANG_KEY = "mgr.lang";

export function App(): JSX.Element {
  const [page, setPage] = useState<Page>("sandboxes");
  const [sbxView, setSbxView] = useState<SbxView>({ view: "list" });
  const [theme, setTheme] = useState<Theme>(
    () => (localStorage.getItem(THEME_KEY) === "light" ? "light" : "dark"),
  );
  const [lang, setLang] = useState<Lang>(
    () => (localStorage.getItem(LANG_KEY) === "en" ? "en" : "zh-CN"),
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

  const nav = (p: Page) => {
    setPage(p);
    if (p === "sandboxes") setSbxView({ view: "list" });
  };

  return (
    <div className="app">
      <IconSprite />
      <aside className="sidebar" aria-label={t(lang, "brand")}>
        <div className="sb-head">
          <div className="sb-brand">
            <Icon name="cube" large />
            <span className="sb-title">{t(lang, "brand")}</span>
          </div>
        </div>
        <nav className="sb-list">
          <div>
            <p className="sb-group-label">Sandbox-mgr</p>
            <div className="sb-row">
              <button
                className={`launch-btn${page === "sandboxes" ? " active" : ""}`}
                onClick={() => nav("sandboxes")}
              >
                <Icon name="terminal" />
                <span className="launch-label">{t(lang, "navSandboxes")}</span>
              </button>
            </div>
            <div className="sb-row">
              <button
                className={`launch-btn${page === "images" ? " active" : ""}`}
                onClick={() => nav("images")}
              >
                <Icon name="box" />
                <span className="launch-label">{t(lang, "navImages")}</span>
              </button>
            </div>
          </div>
        </nav>
        <div className="sb-foot">
          <button
            className="icon-btn"
            title={theme === "dark" ? t(lang, "toLight") : t(lang, "toDark")}
            aria-label={theme === "dark" ? t(lang, "toLight") : t(lang, "toDark")}
            onClick={() => setTheme((m) => (m === "dark" ? "light" : "dark"))}
          >
            <Icon name={theme === "dark" ? "sun" : "moon"} />
          </button>
          <button
            className="icon-btn"
            title={t(lang, "switchLang")}
            aria-label={t(lang, "switchLang")}
            onClick={() => setLang((l) => (l === "zh-CN" ? "en" : "zh-CN"))}
          >
            <Icon name="globe" />
          </button>
        </div>
      </aside>
      <main className="main">
        {page === "sandboxes" ? (
          <SandboxesPage
            view={sbxView}
            onView={setSbxView}
            lang={lang}
            scenarios={scenarios}
            onScenarios={setScenarios}
          />
        ) : (
          <ImagesPage lang={lang} />
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
}: {
  view: SbxView;
  onView: (v: SbxView) => void;
  lang: Lang;
  scenarios: Scenario[] | null;
  onScenarios: (s: Scenario[]) => void;
}): JSX.Element {
  switch (view.view) {
    case "list":
      return (
        <SandboxListPage
          lang={lang}
          onCreate={() => onView({ view: "create" })}
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
