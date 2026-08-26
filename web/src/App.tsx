// App - workspace shell.
//
// Fetches GET /api/manifest and renders a collapsible left sidebar of LAUNCHER
// buttons + a golden-layout workspace on the right. Each click on a button
// creates a NEW instance (click again = another instance); instances are tabs
// that can be dragged into split/tiled layouts. Closing happens via the tab's
// close icon (golden-layout native), which releases the pane component and -
// for agent panes - ends the pty (XtermPane cleanup closes the WS).
//
//   type === "web"   -> IframePane (iframe embedding a containerized service)
//   type === "agent" -> XtermPane  (xterm.js over the /api/term/ws pty WS)
//
// Button visibility is server-driven by `enabled` (web: TCP-reachable;
// agent: command_exists on PATH), so a button only appears when the capability
// is actually present - no dead panes. User-registered buttons are created via
// POST /api/buttons (persisted in buttons.toml on the workspace volume).
//
// golden-layout is imperative; this component owns ONE GoldenLayout instance
// in a ref, mounted into a container <div>. Pane components are registered
// with a single factory that mounts a React root (createRoot) into the
// golden-layout-provided container.element. Roots are unmounted on
// `beforeComponentRelease` so panes torn down by close/drag-out are cleaned up.
//
// The service payload travels as componentState (opaque JsonValue); it is
// decoded back to a typed ServiceEntry through ONE type guard
// (readServiceState) rather than inline casts - the manifest contract has a
// single owner on each side of the boundary (cross-layer-thinking-guide).

import { createRoot, type Root } from "react-dom/client";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  GoldenLayout,
  LayoutConfig,
  ResolvedLayoutConfig,
  type ComponentContainer,
  type JsonValue,
} from "golden-layout";
import "golden-layout/dist/css/goldenlayout-base.css";
import "./gl-kumo.css";

import type { Manifest, ServiceEntry } from "./types";
import { t, type Lang } from "./i18n";
import { Icon, IconSprite, serviceIcon } from "./icons";
import { Sidebar } from "./Sidebar";
import { Statusbar } from "./Statusbar";
import { useStats } from "./useStats";
import { RegisterDialog } from "./RegisterDialog";
import { IframePane } from "./panes/IframePane";
import { ModelsPane } from "./panes/ModelsPane";
import { XtermPane } from "./panes/XtermPane";
import "./styles.css";

type Status = "loading" | "error" | "ready";
type Theme = "dark" | "light";

const PANE_COMPONENT_TYPE = "aio-pane";
const TERMINAL_ID = "terminal";
const COLLAPSE_KEY = "aio.sidebar.collapsed";
const THEME_KEY = "aio.theme";
const LANG_KEY = "aio.lang";
const HEADER_HEIGHT = 40;
const GL_WINDOW_PARAM = "gl-window";
const LAYOUT_KEY = "aio.layout";

/**
 * Popout child windows carry their layout in localStorage under the
 * `gl-window` URL param (written by the parent's BrowserPopout). Consume it
 * here, before any GoldenLayout is constructed: the library's built-in
 * subwindow path would wipe document.body (killing the React root) and defer
 * init() past our loadLayout call. Instead we strip the param, load the saved
 * config ourselves and render a lone workspace (see the SUB_WINDOW branch in
 * App's render).
 */
function consumeSubWindowLayout():
  | { config: LayoutConfig; title?: string }
  | undefined {
  const params = new URLSearchParams(window.location.search);
  const key = params.get(GL_WINDOW_PARAM);
  if (key === null) return undefined;
  const raw = localStorage.getItem(key);
  localStorage.removeItem(key);
  params.delete(GL_WINDOW_PARAM);
  const search = params.toString();
  window.history.replaceState(
    null,
    "",
    `${window.location.pathname}${search ? `?${search}` : ""}${window.location.hash}`,
  );
  if (raw === null) return undefined;
  try {
    const resolved = ResolvedLayoutConfig.unminifyConfig(JSON.parse(raw));
    const config: LayoutConfig = {
      ...LayoutConfig.fromResolved(resolved),
      // gl-kumo.css lays out a 40px strip; golden-layout writes the header
      // height as an inline style, so it must travel in the config.
      dimensions: { headerHeight: HEADER_HEIGHT },
    };
    const root = config.root;
    return { config, title: root?.type === "component" ? root.title : undefined };
  } catch {
    return undefined;
  }
}
const SUB_WINDOW = consumeSubWindowLayout();

export function App(): JSX.Element {
  const containerRef = useRef<HTMLDivElement>(null);
  const glRef = useRef<GoldenLayout | null>(null);
  // React roots per golden-layout component container (unmount on release).
  const rootsRef = useRef(new WeakMap<ComponentContainer, Root>());
  // Per-service instance counter for tab titles: "Terminal", "Terminal (2)", ...
  const seqRef = useRef<Record<string, number>>({});
  // Latest manifest for the tab-icon observer (the gl effect runs once).
  const servicesRef = useRef<ServiceEntry[]>([]);

  const [status, setStatus] = useState<Status>("loading");
  const [errorMsg, setErrorMsg] = useState("");
  const [services, setServices] = useState<ServiceEntry[]>([]);
  const [collapsed, setCollapsed] = useState<boolean>(
    () => localStorage.getItem(COLLAPSE_KEY) === "1",
  );
  // Kumo dark mode is the default; index.html applies the stored value
  // pre-paint to avoid a light flash, and this state is its single owner.
  const [theme, setTheme] = useState<Theme>(
    () => (localStorage.getItem(THEME_KEY) === "light" ? "light" : "dark"),
  );
  const [lang, setLang] = useState<Lang>(
    () => (localStorage.getItem(LANG_KEY) === "en" ? "en" : "zh-CN"),
  );
  const [registerOpen, setRegisterOpen] = useState(false);

  // Container-resource polling; also doubles as the backend heartbeat that
  // drives the statusbar dot. The hook seeds online=true, which is correct:
  // the statusbar only renders once status === "ready" (the manifest fetch
  // already succeeded), and the first /api/stats poll corrects it within 3s.
  // Popout children run the hook too (harmless one-fetch/3s overhead).
  const { stats, online } = useStats();

  // Reset the persisted layout: clear the stored config and reload so the GL
  // effect re-runs the default single-terminal path (deterministic; no GL
  // hot-rebuild).
  const resetLayout = useCallback(() => {
    localStorage.removeItem(LAYOUT_KEY);
    window.location.reload();
  }, []);

  // Persist sidebar collapse state.
  useEffect(() => {
    localStorage.setItem(COLLAPSE_KEY, collapsed ? "1" : "0");
  }, [collapsed]);

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

  // The tab-icon observer (gl effect below) reads the manifest through a ref
  // because that effect runs exactly once.
  useEffect(() => {
    servicesRef.current = services;
  }, [services]);

  // Leading tab icons (the Kumo reference tabs carry a service glyph);
  // gl-kumo.css draws them with masks off data-icon. Titles are "<label>" or
  // "<label> (<n>)". Re-runs when the manifest lands - popout windows build
  // their layout before the fetch resolves.
  const patchTabs = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;
    el.querySelectorAll<HTMLElement>(".lm_tab").forEach((tab) => {
      const title = tab.querySelector(".lm_title")?.textContent ?? "";
      const label = title.replace(/ \(\d+\)$/, "");
      const svc = servicesRef.current.find((s) => s.label === label);
      tab.dataset.icon = svc ? serviceIcon(svc.id, svc.type) : "terminal";
    });
  }, []);
  useEffect(() => {
    patchTabs();
  }, [services, patchTabs]);

  const fetchManifest = useCallback(async (): Promise<ServiceEntry[]> => {
    const r = await fetch("/api/manifest");
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    return (await r.json() as Manifest).services;
  }, []);

  // Initial load.
  useEffect(() => {
    let cancelled = false;
    fetchManifest()
      .then((svcs) => {
        if (cancelled) return;
        setServices(svcs);
        setStatus("ready");
      })
      .catch((e) => {
        if (cancelled) return;
        setErrorMsg(e instanceof Error ? e.message : String(e));
        setStatus("error");
      });
    return () => {
      cancelled = true;
    };
  }, [fetchManifest]);

  // Refresh: re-fetch the manifest. Open instances are left alone (their
  // sessions stay alive); a service that disappeared simply loses its button.
  const refresh = useCallback(() => {
    fetchManifest().then(setServices).catch(() => {
      /* keep current state on refresh failure */
    });
  }, [fetchManifest]);

  // Re-fetch when the window regains focus (catches runtime tool installs /
  // profile changes without a manual refresh). Ignore very rapid refocuses.
  const lastRefresh = useRef(0);
  useEffect(() => {
    const onFocus = () => {
      const now = Date.now();
      if (now - lastRefresh.current > 2000) {
        lastRefresh.current = now;
        refresh();
      }
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refresh]);

  // Launch ONE new instance of a service as a golden-layout component. The
  // default placement puts it in the focused stack (or the root stack), so a
  // click adds a tab next to what the user is working on; dragging the tab
  // header splits/tiles the workspace.
  const launch = useCallback((service: ServiceEntry) => {
    const gl = glRef.current;
    if (!gl) return;
    const n = (seqRef.current[service.id] ?? 0) + 1;
    seqRef.current[service.id] = n;
    const title = n === 1 ? service.label : `${service.label} (${n})`;
    gl.newComponent(PANE_COMPONENT_TYPE, { service, seq: n }, title);
  }, []);

  // Build golden-layout once the manifest is ready (immediately in popout
  // child windows, whose pane state is self-contained). Runs once (guarded by
  // glRef) - manifest refreshes must NOT rebuild the layout. In popout
  // children the dep is frozen: a status-driven rebuild would destroy the
  // instance the parent's BrowserPopout already wired its popIn listener to.
  const glStatus = SUB_WINDOW ? "ready" : status;
  useEffect(() => {
    const el = containerRef.current;
    if (!el || glRef.current) return;
    if (!SUB_WINDOW) {
      if (glStatus !== "ready") return;
      if (services.filter((s) => s.enabled).length === 0) return; // empty state
    }

    const gl = new GoldenLayout(el);
    gl.resizeWithContainerAutomatically = true;

    gl.registerComponentFactoryFunction(
      PANE_COMPONENT_TYPE,
      (container: ComponentContainer, state: JsonValue | undefined) => {
        const service = readServiceState(state);
        if (!service) return undefined;
        const root = createRoot(container.element);
        rootsRef.current.set(container, root);
        root.render(<PaneForService service={service} />);

        // golden-layout emits beforeComponentRelease before tearing down a
        // component (tab close / drag-out / layout destroy). Unmount the React
        // tree so its effects (xterm WS, iframe, observers) are cleaned up.
        container.on("beforeComponentRelease", () => {
          const r = rootsRef.current.get(container);
          if (r) {
            r.unmount();
            rootsRef.current.delete(container);
          }
        });
        return undefined;
      },
    );

    if (SUB_WINDOW) {
      // Popout child: load the popped component as the whole layout and join
      // the parent's popIn protocol (BrowserPopout polls window.__glInstance).
      gl.loadLayout(SUB_WINDOW.config);
      if (SUB_WINDOW.title) document.title = SUB_WINDOW.title;
      (window as unknown as { __glInstance?: unknown }).__glInstance = gl;
    } else {
      // Restore the persisted layout (splits/tabs) if present; any failure
      // (missing key, corrupt JSON, an unminifiable config - e.g. one saved
      // with an open popout) falls through to the default single-terminal
      // layout below.
      let restored = false;
      const raw = localStorage.getItem(LAYOUT_KEY);
      if (raw !== null) {
        try {
          const resolved = ResolvedLayoutConfig.unminifyConfig(JSON.parse(raw));
          const config: LayoutConfig = {
            ...LayoutConfig.fromResolved(resolved),
            dimensions: { headerHeight: HEADER_HEIGHT },
          };
          gl.loadLayout(config);
          resyncSeq(config.root, seqRef.current);
          restored = true;
        } catch {
          /* corrupt archive = behave as if never saved */
        }
      }
      if (!restored) {
        // Default layout: a single stack (one page) holding one terminal
        // instance when the terminal is enabled - which it always is (bash
        // exists).
        const enabled = services.filter((s) => s.enabled);
        const terminal = enabled.find((s) => s.id === TERMINAL_ID) ?? enabled[0];
        seqRef.current[terminal.id] = 1;
        const config: LayoutConfig = {
          root: {
            type: "stack",
            content: [
              {
                type: "component",
                componentType: PANE_COMPONENT_TYPE,
                componentState: { service: terminal, seq: 1 },
                title: terminal.label,
              },
            ],
          },
          settings: { reorderEnabled: true },
          // golden-layout writes the header height as an INLINE style from this
          // config value (CSS alone cannot override it); gl-kumo.css lays out
          // the 40px strip to match the Kumo reference tab bar.
          dimensions: { headerHeight: HEADER_HEIGHT },
        };
        gl.loadLayout(config);
      }

      // Persist layout changes (tab drag/split/close) with a 500ms debounce.
      // SUB_WINDOW must NOT attach this: the popout child runs this same
      // effect, and its stateChanged would overwrite the parent's archive
      // with the child's single-pane layout.
      let saveTimer: ReturnType<typeof setTimeout> | undefined;
      gl.on("stateChanged", () => {
        if (saveTimer) clearTimeout(saveTimer);
        saveTimer = setTimeout(() => {
          try {
            localStorage.setItem(
              LAYOUT_KEY,
              JSON.stringify(ResolvedLayoutConfig.minifyConfig(gl.saveLayout())),
            );
          } catch {
            /* save failure is non-fatal; next change retries */
          }
        }, 500);
      });
    }
    glRef.current = gl;

    // golden-layout recreates .lm_tab nodes whenever a component moves
    // between stacks, so re-patch data-icon through an observer (patchTabs
    // itself is shared with the manifest effect above).
    const tabObserver = new MutationObserver((records) => {
      const tabAdded = records.some((r) =>
        Array.from(r.addedNodes).some(
          (n) =>
            n instanceof HTMLElement &&
            (n.classList.contains("lm_tab") || n.querySelector(".lm_tab") !== null),
        ),
      );
      if (tabAdded) patchTabs();
    });
    tabObserver.observe(el, { childList: true, subtree: true });
    patchTabs();

    // iframe drag-capture: while a splitter or tab/header drag is in progress,
    // set an `is-dragging` class on the layout root so CSS reveals the
    // transparent overlay over iframes (IframePane) and they stop swallowing
    // pointer events. pointerup anywhere ends the drag.
    const onPointerDown = (e: PointerEvent) => {
      const target = e.target as HTMLElement | null;
      if (target?.closest(".lm_splitter, .lm_header, .lm_tab")) {
        el.classList.add("is-dragging");
      }
    };
    const onPointerUp = () => {
      el.classList.remove("is-dragging");
    };
    el.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("pointerup", onPointerUp);

    return () => {
      el.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("pointerup", onPointerUp);
      tabObserver.disconnect();
      gl.destroy();
      glRef.current = null;
    };
    // `services` intentionally not a dep: the layout is built once from the
    // first manifest; refreshes only update the sidebar.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [glStatus]);

  const enabledServices = services.filter((s) => s.enabled);

  // Popout child window: a lone workspace plus a dock-back button that emits
  // golden-layout's popIn event (the parent re-adds the pane and closes us).
  if (SUB_WINDOW) {
    return (
      <div className="app app-popout">
        <IconSprite />
        <div className="gl-root" ref={containerRef} />
        <button
          className="icon-btn popin-btn"
          title={t(lang, "popin")}
          aria-label={t(lang, "popin")}
          onClick={() => glRef.current?.emit("popIn")}
        >
          <Icon name="dock" />
        </button>
      </div>
    );
  }

  if (status === "loading") {
    return (
      <>
        <IconSprite />
        <div className="status">{t(lang, "loading")}</div>
      </>
    );
  }
  if (status === "error") {
    return (
      <>
        <IconSprite />
        <div className="status error">
          {t(lang, "loadFailed")}
          {errorMsg}
        </div>
      </>
    );
  }
  if (enabledServices.length === 0) {
    return (
      <>
        <IconSprite />
        <div className="status">{t(lang, "noButtons")}</div>
      </>
    );
  }

  return (
    <div className="app">
      <IconSprite />
      <Sidebar
        services={services}
        collapsed={collapsed}
        lang={lang}
        onToggleCollapse={() => setCollapsed((c) => !c)}
        onLaunch={launch}
        onRefresh={refresh}
        onOpenRegister={() => setRegisterOpen(true)}
        onDelete={(id) => void deleteButton(id)}
      />
      <main className="main">
        <div className="gl-root" ref={containerRef} />
        <Statusbar
          services={services}
          lang={lang}
          theme={theme}
          online={online}
          stats={stats}
          onResetLayout={resetLayout}
          onToggleTheme={() => setTheme((m) => (m === "dark" ? "light" : "dark"))}
          onToggleLang={() => setLang((l) => (l === "zh-CN" ? "en" : "zh-CN"))}
        />
      </main>
      <RegisterDialog
        open={registerOpen}
        lang={lang}
        onClose={() => setRegisterOpen(false)}
        onRegister={registerButton}
      />
    </div>
  );

  // Register a user button via POST /api/buttons, then refresh so it appears
  // (command_exists is probed on the next manifest fetch).
  async function registerButton(label: string, cmd: string): Promise<boolean> {
    try {
      const r = await fetch("/api/buttons", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ label, cmd }),
      });
      if (!r.ok) return false;
      refresh();
      return true;
    } catch {
      return false;
    }
  }

  async function deleteButton(id: string): Promise<void> {
    try {
      const r = await fetch(`/api/buttons/${encodeURIComponent(id)}`, {
        method: "DELETE",
      });
      if (!r.ok && r.status !== 404) return;
    } catch {
      return;
    }
    refresh();
  }
}

/** Render the generic pane for a service by its type. */
function PaneForService({ service }: { service: ServiceEntry }): JSX.Element {
  if (service.type === "web") return <IframePane service={service} />;
  if (service.type === "page") return <ModelsPane service={service} />;
  return <XtermPane service={service} />;
}

/**
 * Decode the golden-layout componentState back to a typed ServiceEntry.
 * Single decoder for the manifest payload on the pane side - callers must not
 * cast `state.service` inline (cross-layer-thinking-guide: one owner).
 */
function readServiceState(state: JsonValue | undefined): ServiceEntry | undefined {
  if (!state || typeof state !== "object") return undefined;
  const maybe = state as { service?: unknown };
  return isServiceEntry(maybe.service) ? maybe.service : undefined;
}

function isServiceEntry(v: unknown): v is ServiceEntry {
  if (typeof v !== "object" || v === null) return false;
  const s = v as Record<string, unknown>;
  return (
    typeof s.id === "string" &&
    (s.type === "web" || s.type === "agent" || s.type === "page") &&
    typeof s.enabled === "boolean"
  );
}

/**
 * After restoring a saved layout, re-sync the per-service instance counters
 * from the restored componentStates. Without this, launching a new instance
 * would restart at seq 1 and title-clash with the restored "Terminal" tab
 * ("Terminal" vs "Terminal (2)" both meaning the second instance).
 */
function resyncSeq(node: LayoutConfig["root"], seq: Record<string, number>): void {
  if (!node) return;
  if (node.type === "component") {
    const state = node.componentState as { service?: unknown; seq?: unknown } | undefined;
    if (state && isServiceEntry(state.service) && typeof state.seq === "number") {
      seq[state.service.id] = Math.max(seq[state.service.id] ?? 0, state.seq);
    }
    return;
  }
  const children = (node as { content?: LayoutConfig["root"][] }).content;
  children?.forEach((c) => resyncSeq(c, seq));
}
