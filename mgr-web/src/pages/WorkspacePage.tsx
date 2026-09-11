// WorkspacePage - the golden-layout workspace (design §2, ported from
// web/src/App.tsx, the deprecated workbench SPA). Left: the sandbox tree;
// right: panes. Every pane is bound to a sandbox: componentState carries
// { service, sandbox, seq } and the tab title is "<label>@<name>", so tabs
// of multiple sandboxes mix freely in one layout (D2).
//
// Mechanisms ported verbatim from web/src/App.tsx (only the sandbox
// dimension is new):
//   - ONE GoldenLayout instance in a ref, mounted into a container div;
//     built exactly once (glRef guard) after the boot decision - later
//     sandbox-list / manifest refreshes never rebuild it;
//   - single component factory that mounts a React root per golden-layout
//     container and unmounts on `beforeComponentRelease` (closed/dragged-out
//     panes clean up their effects - the xterm pty WS included);
//   - per-(sandbox, service) sequence pools: "Terminal@dev1", then
//     "Terminal@dev1 (2)"; closing frees the number for reuse (R5);
//   - layout persistence under `mgr.layout` (localStorage), 500ms debounce,
//     headerHeight 40 re-applied on restore (gl-kumo.css lays out a 40px
//     strip and golden-layout writes the height as an inline style);
//   - popout subwindows BYPASS golden-layout's built-in child path (it would
//     wipe document.body and defer init): the child consumes its config from
//     localStorage under the `gl-window` URL param, strips the param, and
//     joins the parent's popIn protocol via window.__glInstance. Parent and
//     child are both the mgr.localhost origin, so the mechanism carries over
//     unchanged. App renders the lone workspace (no admin shell) when
//     IS_POPOUT_CHILD is set;
//   - iframe drag-capture: an `is-dragging` class on the layout root while a
//     splitter/tab drag is in progress reveals IframePane's .drag-overlay;
//   - tab glyphs patched through a MutationObserver (golden-layout recreates
//     .lm_tab nodes on stack moves); titles parse as "<label>@<name> (n)"
//     (sandbox names are slugs and never contain "@", so the LAST "@" is the
//     separator even when a label carries one).
//
// mgr-web deviations from the ported code, both deliberate:
//   - on unmount (every page switch, unlike the workbench SPA whose App
//     never unmounts) the pending debounced save is FLUSHED so the last
//     <500ms of layout changes survives navigation;
//   - the default layout opens the FIRST RUNNING sandbox's terminal (the
//     workbench unconditionally opened its own terminal); with no running
//     sandbox the workspace shows an empty prompt and re-arms - starting a
//     sandbox from the tree then builds the default pane.

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
import "../gl-kumo.css";

import {
  deleteSandboxButton,
  getSandboxManifest,
  listSandboxes,
  probeSandboxPort,
  registerSandboxButton,
  sandboxAction,
} from "../api";
import { t, type Lang } from "../i18n";
import { Icon } from "../icons";
import type { Sandbox } from "../types";
import { RegisterDialog } from "./workspace/RegisterDialog";
import { SandboxTree, serviceIcon, type ManifestState } from "./workspace/SandboxTree";
import { IframePane } from "./workspace/panes/IframePane";
import { XtermPane } from "./workspace/panes/XtermPane";
import { isServiceEntry, ON_DEMAND_SERVICE_IDS, type RegisterButtonInput, type ServiceEntry } from "./workspace/types";
import { CodeServerPane } from "./workspace/panes/CodeServerPane";

const PANE_COMPONENT_TYPE = "mgr-pane";
const TERMINAL_ID = "terminal";
const HEADER_HEIGHT = 40;
const LAYOUT_KEY = "mgr.layout";
const GL_WINDOW_PARAM = "gl-window";
/** S5/R2: workspace tree collapsed state (icons-only + hover flyout). */
const TREE_COLLAPSED_KEY = "mgr.treeCollapsed";
const POLL_MS = 4000;

/**
 * Current UI language for pane components that render OUTSIDE React's prop
 * flow: golden-layout's component factory creates pane contents imperatively
 * (createRoot in the factory closure), so CodeServerPane's start/placeholder
 * states receive their lang through this ref instead of a prop chain. A
 * module-level ref set on every WorkspacePage render is the same "latest
 * value for imperative code" pattern as manifestsRef; it is read at the
 * pane's CREATION time - an already-open pane keeps its creation language
 * across a mid-session switch (panes have never followed lang live: the
 * xterm connect/disconnect notices are fixed strings), new panes pick up
 * the current one.
 */
const langRef: { current: Lang } = { current: "zh-CN" };

/**
 * Popout child windows carry their layout in localStorage under the
 * `gl-window` URL param (written by the parent's BrowserPopout). Consume it
 * here, before any GoldenLayout is constructed: the library's built-in
 * subwindow path would wipe document.body (killing the React root) and defer
 * init() past our loadLayout call. Instead we strip the param, load the saved
 * config ourselves and render a lone workspace (the SUB_WINDOW branch in
 * WorkspacePage's render; App skips the admin shell).
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

/** True in a golden-layout popout child window: App renders the lone
 * workspace without the admin shell (same reason web's App had a SUB_WINDOW
 * render branch). */
export const IS_POPOUT_CHILD = SUB_WINDOW !== undefined;

/** How the workspace boots: restore the persisted layout, open the default
 * terminal pane, or show the empty prompt. Decided once; "empty" re-arms
 * when a running sandbox appears. */
type Boot =
  | { kind: "loading" }
  | { kind: "archive"; config: LayoutConfig }
  | { kind: "default"; pane: { service: ServiceEntry; sandbox: string } }
  | { kind: "empty" };

interface Props {
  lang: Lang;
  /** Sandbox to expand + highlight (goWorkspace from the list page). */
  focus: string | null;
  /** Navigate to the sandbox list (empty-state affordance). */
  onManage: () => void;
}

export function WorkspacePage({ lang, focus, onManage }: Props): JSX.Element {
  langRef.current = lang;
  const containerRef = useRef<HTMLDivElement>(null);
  const glRef = useRef<GoldenLayout | null>(null);
  // React roots per golden-layout component container (unmount on release).
  const rootsRef = useRef(new WeakMap<ComponentContainer, Root>());
  // Per-(sandbox, service) in-use sequence numbers for tab titles. A pool is
  // the Set of numbers held by that pair's currently-open tabs; a closed
  // instance returns its number, so the next launch reuses the smallest free
  // positive integer (R5 recycle semantics).
  const seqRef = useRef<Record<string, Set<number>>>({});

  const [sandboxes, setSandboxes] = useState<Sandbox[] | null>(null);
  const [listError, setListError] = useState("");
  const [manifests, setManifests] = useState<Record<string, ManifestState>>({});
  const manifestsRef = useRef<Record<string, ManifestState>>({});
  const inflightRef = useRef(new Set<string>());
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const [starting, setStarting] = useState("");
  const [startErr, setStartErr] = useState("");
  const [registerFor, setRegisterFor] = useState<string | null>(null);
  const [boot, setBoot] = useState<Boot>({ kind: "loading" });
  // S5/R2: workspace tree collapsed (icons-only + hover flyout). Independent
  // of the app sidebar collapse (R4) — own key, own state.
  const [treeCollapsed, setTreeCollapsed] = useState<boolean>(
    () => localStorage.getItem(TREE_COLLAPSED_KEY) === "1",
  );

  // ── sandbox list: 4s poll (same pattern as SandboxListPage) ───────

  const fetchList = useCallback(async (): Promise<void> => {
    try {
      const r = await listSandboxes();
      setSandboxes(r.sandboxes);
      setListError("");
    } catch (e) {
      // Keep the last list; the poll retries in 4s.
      setListError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void fetchList();
    const timer = setInterval(() => void fetchList(), POLL_MS);
    return () => clearInterval(timer);
  }, [fetchList]);

  // S5/R2: persist the tree collapsed state.
  useEffect(() => {
    localStorage.setItem(TREE_COLLAPSED_KEY, treeCollapsed ? "1" : "0");
  }, [treeCollapsed]);

  // ── tab glyphs ────────────────────────────────────────────────────

  const findService = useCallback((sandbox: string, label: string): ServiceEntry | undefined => {
    const m = manifestsRef.current[sandbox];
    if (!m || m.services === null) return undefined;
    return m.services.find((s) => s.label === label);
  }, []);

  // Leading tab icons (gl-kumo.css draws them with masks off data-icon).
  // Titles are "<label>@<name>" or "<label>@<name> (n)"; strip the seq
  // suffix, then split at the LAST "@" (sandbox names are slugs without @).
  const patchTabs = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;
    el.querySelectorAll<HTMLElement>(".lm_tab").forEach((tab) => {
      const title = tab.querySelector(".lm_title")?.textContent ?? "";
      const stripped = title.replace(/ \(\d+\)$/, "");
      const at = stripped.lastIndexOf("@");
      const label = at === -1 ? stripped : stripped.slice(0, at);
      const sandbox = at === -1 ? "" : stripped.slice(at + 1);
      const svc = at === -1 ? undefined : findService(sandbox, label);
      tab.dataset.icon = svc ? serviceIcon(svc.id, svc.type) : "terminal";
    });
  }, [findService]);

  // ── per-sandbox manifests: lazy (first expand / refresh / after start) ─

  const loadManifest = useCallback((name: string, force = false): void => {
    if (inflightRef.current.has(name)) return;
    if (!force) {
      const cur = manifestsRef.current[name];
      if (cur !== undefined && cur.status !== "idle") return;
    }
    inflightRef.current.add(name);
    setManifests((prev) => ({
      ...prev,
      [name]: { status: "loading", services: prev[name]?.services ?? null, error: "" },
    }));
    getSandboxManifest(name)
      .then((m) => {
        setManifests((prev) => ({ ...prev, [name]: { status: "ok", services: m.services, error: "" } }));
      })
      .catch((e) => {
        setManifests((prev) => ({
          ...prev,
          [name]: {
            status: "error",
            services: prev[name]?.services ?? null,
            error: e instanceof Error ? e.message : String(e),
          },
        }));
      })
      .finally(() => {
        inflightRef.current.delete(name);
      });
  }, []);

  // Keep a ref in sync for the once-only GL effect + tab patcher, and
  // re-patch tab glyphs when a manifest lands (restored-layout tabs were
  // created before their sandbox's manifest was ever fetched).
  useEffect(() => {
    manifestsRef.current = manifests;
    patchTabs();
  }, [manifests, patchTabs]);

  // Auto-load: every expanded + RUNNING sandbox with an untouched manifest
  // entry gets fetched (first expand via focus, or a start that flipped live
  // to running). Stopped sandboxes are skipped - the proxy would 502.
  useEffect(() => {
    if (sandboxes === null) return;
    for (const sb of sandboxes) {
      if (!expanded.has(sb.name) || sb.live !== "running") continue;
      const m = manifests[sb.name];
      if (m === undefined || m.status === "idle") loadManifest(sb.name);
    }
  }, [sandboxes, expanded, manifests, loadManifest]);

  const onToggle = (name: string): void => {
    const opening = !expanded.has(name);
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
    // Re-expanding is the manual refresh gesture (design §2.2): always
    // revalidate when the sandbox is up; the cached buttons stay visible
    // while the fetch is in flight (stale-while-revalidate).
    if (opening) {
      const sb = sandboxes?.find((s) => s.name === name);
      if (sb && sb.live === "running") loadManifest(name, true);
    }
  };

  // goWorkspace(name): expand + highlight the sandbox (manifest via the
  // auto-load effect above).
  useEffect(() => {
    if (focus === null || SUB_WINDOW) return;
    setExpanded((prev) => (prev.has(focus) ? prev : new Set(prev).add(focus)));
  }, [focus]);

  const onStart = async (name: string): Promise<void> => {
    if (starting !== "") return;
    setStarting(name);
    setStartErr("");
    try {
      await sandboxAction(name, "start");
      await fetchList();
    } catch (e) {
      setStartErr(e instanceof Error ? e.message : String(e));
    } finally {
      setStarting("");
    }
  };

  const onDeleteButton = (sandbox: string, id: string): void => {
    void deleteSandboxButton(sandbox, id)
      .catch(() => undefined)
      .then(() => loadManifest(sandbox, true));
  };

  const doRegister = async (input: RegisterButtonInput): Promise<boolean> => {
    if (registerFor === null) return false;
    try {
      await registerSandboxButton(registerFor, input);
    } catch {
      return false;
    }
    loadManifest(registerFor, true);
    return true;
  };

  /** Stable probe callback per sandbox (the dialog's debounced effect keys
   * on it - an inline closure would re-arm the debounce on every render). */
  const probeFor = useCallback(
    (sandbox: string) =>
      (port: number): Promise<{ listening: boolean }> =>
        probeSandboxPort(sandbox, port),
    [],
  );

  // Reset the persisted layout: clear the stored config and reload so the GL
  // effect re-runs the default path (deterministic; no GL hot-rebuild).
  const resetLayout = useCallback(() => {
    localStorage.removeItem(LAYOUT_KEY);
    window.location.reload();
  }, []);

  // ── boot decision: archive | default terminal | empty ─────────────

  useEffect(() => {
    if (SUB_WINDOW || sandboxes === null) return;
    if (boot.kind === "archive" || boot.kind === "default") return;
    // Restore the persisted layout if present and parseable; a corrupt
    // archive self-heals away ("behave as if never saved", web semantics).
    const raw = localStorage.getItem(LAYOUT_KEY);
    if (raw !== null) {
      try {
        const resolved = ResolvedLayoutConfig.unminifyConfig(JSON.parse(raw));
        setBoot({
          kind: "archive",
          config: {
            ...LayoutConfig.fromResolved(resolved),
            dimensions: { headerHeight: HEADER_HEIGHT },
          },
        });
        return;
      } catch {
        localStorage.removeItem(LAYOUT_KEY);
      }
    }
    // Default layout: the FIRST running sandbox's terminal pane (design
    // §2.3); with no running sandbox the empty prompt stays up and this
    // decision re-arms on the next poll (a tree-side start can flip it).
    const running = sandboxes.find((s) => s.live === "running");
    if (!running) {
      setBoot({ kind: "empty" });
      return;
    }
    let cancelled = false;
    getSandboxManifest(running.name)
      .then((m) => {
        if (cancelled) return;
        const agents = m.services.filter((s) => s.enabled && s.type === "agent");
        const terminal = agents.find((s) => s.id === TERMINAL_ID) ?? agents[0];
        setBoot({
          kind: "default",
          pane: { service: terminal ?? syntheticTerminal(), sandbox: running.name },
        });
      })
      .catch(() => {
        if (cancelled) return;
        setBoot({ kind: "default", pane: { service: syntheticTerminal(), sandbox: running.name } });
      });
    return () => {
      cancelled = true;
    };
    // `boot.kind` re-arms the decision while unresolved (loading/empty);
    // once decided, the guard above pins it. Rebuilding the layout on later
    // boot/list changes would destroy live panes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sandboxes, boot.kind]);

  // ── launch: one NEW pane instance of (sandbox, service) ───────────

  const launch = useCallback((sandbox: string, service: ServiceEntry) => {
    const gl = glRef.current;
    if (!gl) return;
    const key = seqKey(sandbox, service.id);
    const pool = getSeqPool(seqRef.current, key);
    let n = 1;
    while (pool.has(n)) n++;
    pool.add(n);
    const title =
      n === 1 ? `${service.label}@${sandbox}` : `${service.label}@${sandbox} (${n})`;
    gl.newComponent(PANE_COMPONENT_TYPE, { service, sandbox, seq: n }, title);
  }, []);

  // ── golden-layout: built exactly once, after the boot decision ────

  useEffect(() => {
    const el = containerRef.current;
    if (!el || glRef.current) return;
    if (!SUB_WINDOW && (boot.kind === "loading" || boot.kind === "empty")) return;

    const gl = new GoldenLayout(el);
    gl.resizeWithContainerAutomatically = true;

    gl.registerComponentFactoryFunction(
      PANE_COMPONENT_TYPE,
      (container: ComponentContainer, state: JsonValue | undefined) => {
        const pane = readPaneState(state);
        if (!pane) return undefined;
        const key = seqKey(pane.sandbox, pane.service.id);
        const root = createRoot(container.element);
        rootsRef.current.set(container, root);
        root.render(<PaneForService service={pane.service} sandbox={pane.sandbox} />);

        // Claim THIS instance's sequence slot on every component creation
        // (Set.add is idempotent). launch() pre-adds the number it picked and
        // collectInUseSeq covers restore; the re-add here is what keeps the
        // pool consistent when golden-layout re-creates a pane from
        // persisted componentState on popIn - the dragged-out instance freed
        // its number on release, and without re-claiming it the next launch
        // would reuse it and duplicate the title (R5).
        if (pane.seq !== undefined) {
          getSeqPool(seqRef.current, key).add(pane.seq);
        }

        // golden-layout emits beforeComponentRelease before tearing down a
        // component (tab close / drag-out / layout destroy). Unmount the
        // React tree so its effects (xterm WS, iframe, observers) are cleaned
        // up, and return THIS instance's sequence number to the pool (R2).
        // Each container captures its own seq in the closure, so only its
        // own number is freed - never a sibling instance's (R5).
        container.on("beforeComponentRelease", () => {
          const r = rootsRef.current.get(container);
          if (r) {
            r.unmount();
            rootsRef.current.delete(container);
          }
          if (pane.seq !== undefined) {
            seqRef.current[key]?.delete(pane.seq);
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
    } else if (boot.kind === "archive") {
      gl.loadLayout(boot.config);
      collectInUseSeq(boot.config.root, seqRef.current);
    } else if (boot.kind === "default") {
      // Default layout: a single stack holding one terminal instance of the
      // first running sandbox (resolved by the boot decider).
      const { service, sandbox } = boot.pane;
      getSeqPool(seqRef.current, seqKey(sandbox, service.id)).add(1);
      gl.loadLayout({
        root: {
          type: "stack",
          content: [
            {
              type: "component",
              componentType: PANE_COMPONENT_TYPE,
              componentState: { service, sandbox, seq: 1 },
              title: `${service.label}@${sandbox}`,
            },
          ],
        },
        settings: { reorderEnabled: true },
        // golden-layout writes the header height as an INLINE style from
        // this config value (CSS alone cannot override it); gl-kumo.css
        // lays out the 40px strip to match the Kumo reference tab bar.
        dimensions: { headerHeight: HEADER_HEIGHT },
      });
    }

    // Persist layout changes (tab drag/split/close) with a 500ms debounce.
    // SUB_WINDOW must NOT attach this: the popout child runs this same
    // effect, and its stateChanged would overwrite the parent's archive
    // with the child's single-pane layout.
    let saveTimer: ReturnType<typeof setTimeout> | undefined;
    const save = (): void => {
      try {
        localStorage.setItem(
          LAYOUT_KEY,
          JSON.stringify(ResolvedLayoutConfig.minifyConfig(gl.saveLayout())),
        );
      } catch {
        /* save failure is non-fatal; next change retries */
      }
    };
    if (!SUB_WINDOW) {
      gl.on("stateChanged", () => {
        if (saveTimer) clearTimeout(saveTimer);
        saveTimer = setTimeout(save, 500);
      });
    }
    glRef.current = gl;

    // golden-layout recreates .lm_tab nodes whenever a component moves
    // between stacks, so re-patch data-icon through an observer (patchTabs
    // itself is shared with the manifests effect above).
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

    // iframe drag-capture: while a splitter or tab/header drag is in
    // progress, set an `is-dragging` class on the layout root so CSS reveals
    // the transparent overlay over iframes (IframePane) and they stop
    // swallowing pointer events. pointerup anywhere ends the drag.
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
      if (saveTimer) clearTimeout(saveTimer);
      // mgr-web unmounts the workspace page on every nav switch (the
      // workbench SPA's App never unmounted): flush any pending debounced
      // save so the last <500ms of layout changes survives navigation.
      if (!SUB_WINDOW) save();
      gl.destroy();
      glRef.current = null;
    };
    // `boot` decides once; the layout is built from that decision and must
    // NOT be rebuilt afterwards (manifest/list refreshes included).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [boot.kind]);

  // ── render ────────────────────────────────────────────────────────

  // Popout child window: a lone workspace plus a dock-back button that emits
  // golden-layout's popIn event (the parent re-adds the pane and closes us).
  if (SUB_WINDOW) {
    return (
      <>
        <div className="gl-root" ref={containerRef} />
        <button
          className="icon-btn popin-btn"
          title={t(lang, "popin")}
          aria-label={t(lang, "popin")}
          onClick={() => glRef.current?.emit("popIn")}
        >
          <Icon name="dock" />
        </button>
      </>
    );
  }

  return (
    <div className="ws">
      <SandboxTree
        lang={lang}
        sandboxes={sandboxes ?? []}
        manifests={manifests}
        expanded={expanded}
        starting={starting}
        focus={focus}
        collapsed={treeCollapsed}
        onCollapseToggle={() => setTreeCollapsed((c) => !c)}
        onToggle={onToggle}
        onLaunch={launch}
        onStart={(name) => void onStart(name)}
        onRegister={setRegisterFor}
        onDeleteButton={onDeleteButton}
        footer={
          <div className="ws-tree-foot">
            {startErr && (
              <p className="sb-empty" style={{ color: "var(--danger)" }}>
                {t(lang, "actionFailed")}
                {startErr}
              </p>
            )}
            <button className="btn btn-ghost btn-sm" onClick={resetLayout}>
              <Icon name="reset" />
              {t(lang, "wsResetLayout")}
            </button>
          </div>
        }
      />
      <main className="ws-main">
        {boot.kind === "archive" || boot.kind === "default" ? (
          <div className="gl-root" ref={containerRef} />
        ) : listError ? (
          <div className="ws-empty ws-error">
            {t(lang, "loadFailed")}
            {listError}
          </div>
        ) : boot.kind === "empty" ? (
          <div className="ws-empty">
            <p>{t(lang, "wsEmpty")}</p>
            <button className="btn btn-secondary btn-sm" onClick={onManage}>
              <Icon name="cube" />
              {t(lang, "navSandboxes")}
            </button>
          </div>
        ) : (
          <div className="ws-empty">{t(lang, "loading")}</div>
        )}
      </main>
      {registerFor !== null && (
        <RegisterDialog
          sandbox={registerFor}
          lang={lang}
          onClose={() => setRegisterFor(null)}
          onProbe={probeFor(registerFor)}
          onRegister={doRegister}
        />
      )}
    </div>
  );
}

/** Render the generic pane for a service by its type. "page" entries are
 * filtered out of the tree (mgr-web's Models page replaces the sandbox-local
 * pane); that branch only guards a foreign componentState decoded from a
 * restored layout. ON_DEMAND services (D4: codeServer) get the start-state
 * machine pane instead of the plain iframe - a disabled manifest entry is
 * not a dead button but the pane's whole reason to exist. */
function PaneForService({
  service,
  sandbox,
}: {
  service: ServiceEntry;
  sandbox: string;
}): JSX.Element {
  if (service.type === "web" && ON_DEMAND_SERVICE_IDS.has(service.id)) {
    return <CodeServerPane service={service} sandbox={sandbox} lang={langRef.current} />;
  }
  if (service.type === "web") return <IframePane service={service} sandbox={sandbox} />;
  if (service.type === "agent") return <XtermPane service={service} sandbox={sandbox} />;
  return <div className="ws-empty">{service.label}</div>;
}

/**
 * Decode the golden-layout componentState back to the pane's service + its
 * sandbox + its sequence number. Single decoder for the manifest payload on
 * the pane side - callers must not cast `state.service` inline
 * (cross-layer-thinking-guide: one owner). `seq` is validated
 * (`typeof === "number"`) and only present when actually persisted, so a
 * legacy/foreign state can't poison the pool.
 */
function readPaneState(
  state: JsonValue | undefined,
): { service: ServiceEntry; sandbox: string; seq?: number } | undefined {
  if (!state || typeof state !== "object") return undefined;
  const maybe = state as { service?: unknown; sandbox?: unknown; seq?: unknown };
  if (!isServiceEntry(maybe.service)) return undefined;
  if (typeof maybe.sandbox !== "string") return undefined;
  const seq = typeof maybe.seq === "number" ? maybe.seq : undefined;
  return { service: maybe.service, sandbox: maybe.sandbox, seq };
}

/** Sequence-pool key: instances are numbered per (sandbox, service) - the
 * same service in two sandboxes counts independently. */
function seqKey(sandbox: string, serviceId: string): string {
  return `${sandbox}/${serviceId}`;
}

/**
 * Get the in-use sequence pool for a (sandbox, service) pair, creating it
 * lazily. A pool is the Set of sequence numbers held by that pair's
 * currently-open tabs; the next launch takes the smallest positive integer
 * NOT in it.
 */
function getSeqPool(seq: Record<string, Set<number>>, key: string): Set<number> {
  let pool = seq[key];
  if (!pool) {
    pool = new Set();
    seq[key] = pool;
  }
  return pool;
}

/**
 * After restoring a saved layout, rebuild each pair's in-use sequence pool
 * from the restored componentStates (R3). Numbers are ADDED - never max'd -
 * so the pool is exactly the set of currently-open instance numbers and
 * slots freed by closed instances in the persisted layout stay reusable.
 */
function collectInUseSeq(node: LayoutConfig["root"], seq: Record<string, Set<number>>): void {
  if (!node) return;
  if (node.type === "component") {
    const pane = readPaneState(node.componentState);
    if (pane?.seq !== undefined) {
      getSeqPool(seq, seqKey(pane.sandbox, pane.service.id)).add(pane.seq);
    }
    return;
  }
  node.content.forEach((c) => collectInUseSeq(c, seq));
}

/**
 * Fallback terminal entry when the default sandbox's manifest is briefly
 * unreachable through the proxy (cold start): the terminal service's shape
 * is fixed by app/services.toml, and a working terminal pane beats an empty
 * workspace while the tree's own fetch retries on the next expand.
 */
function syntheticTerminal(): ServiceEntry {
  return { id: TERMINAL_ID, type: "agent", enabled: true, label: "Terminal", deletable: false, cmd: "" };
}
