// App - workspace shell.
//
// Fetches GET /api/manifest and renders a collapsible left sidebar of toggle
// buttons + a tab-stack main area. Each button toggles a single tab
// (open/close); the active tab fills the main area.
//   type === "web"   -> IframePane (iframe embedding a containerized service)
//   type === "agent" -> XtermPane  (xterm.js over the /api/term/ws pty WS)
//
// Button visibility is server-driven by `enabled` (web: TCP-reachable;
// agent: command_exists on PATH), so a button only appears when the capability
// is actually present - no dead panes. User-registered buttons are created via
// POST /api/buttons (persisted in buttons.toml on the workspace volume).

import { useCallback, useEffect, useRef, useState } from "react";

import type { Manifest, ServiceEntry } from "./types";
import { Sidebar } from "./Sidebar";
import { TabStack } from "./TabStack";
import "./styles.css";

type Status = "loading" | "error" | "ready";

const TERMINAL_ID = "terminal";
const COLLAPSE_KEY = "aio.sidebar.collapsed";

export function App(): JSX.Element {
  const [status, setStatus] = useState<Status>("loading");
  const [errorMsg, setErrorMsg] = useState("");
  const [services, setServices] = useState<ServiceEntry[]>([]);
  const [openTabs, setOpenTabs] = useState<string[]>([]);
  const [activeTab, setActiveTab] = useState<string | null>(null);
  const [collapsed, setCollapsed] = useState<boolean>(
    () => localStorage.getItem(COLLAPSE_KEY) === "1",
  );

  // Persist sidebar collapse state.
  useEffect(() => {
    localStorage.setItem(COLLAPSE_KEY, collapsed ? "1" : "0");
  }, [collapsed]);

  const fetchManifest = useCallback(async (): Promise<ServiceEntry[]> => {
    const r = await fetch("/api/manifest");
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    return (await r.json() as Manifest).services;
  }, []);

  // Initial load: fetch + open the terminal tab by default (it is always
  // enabled - bash exists).
  useEffect(() => {
    let cancelled = false;
    fetchManifest()
      .then((svcs) => {
        if (cancelled) return;
        setServices(svcs);
        const term = svcs.find((s) => s.id === TERMINAL_ID && s.enabled);
        setOpenTabs(term ? [TERMINAL_ID] : []);
        setActiveTab(term ? TERMINAL_ID : null);
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

  // Refresh: re-fetch and reconcile open tabs (drop ones no longer enabled).
  const refresh = useCallback(() => {
    fetchManifest()
      .then((svcs) => {
        setServices(svcs);
        setOpenTabs((prev) => {
          const enabledIds = new Set(svcs.filter((s) => s.enabled).map((s) => s.id));
          const kept = prev.filter((id) => enabledIds.has(id));
          setActiveTab((act) => (act && kept.includes(act) ? act : kept[kept.length - 1] ?? null));
          return kept;
        });
      })
      .catch(() => {
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

  const toggleTab = useCallback((id: string) => {
    setOpenTabs((prev) => {
      if (prev.includes(id)) {
        const next = prev.filter((x) => x !== id);
        setActiveTab((act) => (act === id ? next[next.length - 1] ?? null : act));
        return next;
      }
      const next = [...prev, id];
      setActiveTab(id);
      return next;
    });
  }, []);

  const activateTab = useCallback((id: string) => setActiveTab(id), []);

  const closeTab = useCallback((id: string) => {
    setOpenTabs((prev) => {
      const next = prev.filter((x) => x !== id);
      setActiveTab((act) => (act === id ? next[next.length - 1] ?? null : act));
      return next;
    });
  }, []);

  // Register a user button via POST /api/buttons, then refresh so it appears
  // (command_exists is probed on the next manifest fetch).
  const registerButton = useCallback(
    async (label: string, cmd: string): Promise<boolean> => {
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
    },
    [refresh],
  );

  const deleteButton = useCallback(
    async (id: string): Promise<void> => {
      try {
        const r = await fetch(`/api/buttons/${encodeURIComponent(id)}`, {
          method: "DELETE",
        });
        if (!r.ok && r.status !== 404) return;
      } catch {
        return;
      }
      setOpenTabs((prev) => {
        if (!prev.includes(id)) return prev;
        const next = prev.filter((x) => x !== id);
        setActiveTab((act) => (act === id ? next[next.length - 1] ?? null : act));
        return next;
      });
      refresh();
    },
    [refresh],
  );

  if (status === "loading") return <div className="status">Loading workspace…</div>;
  if (status === "error")
    return <div className="status error">Failed to load manifest: {errorMsg}</div>;

  const openServices = openTabs
    .map((id) => services.find((s) => s.id === id))
    .filter((s): s is ServiceEntry => Boolean(s));

  return (
    <div className="app">
      <Sidebar
        services={services}
        openTabs={openTabs}
        collapsed={collapsed}
        onToggleCollapse={() => setCollapsed((c) => !c)}
        onToggle={toggleTab}
        onRefresh={refresh}
        onRegister={registerButton}
        onDelete={deleteButton}
      />
      <main className="main">
        <TabStack
          tabs={openServices}
          activeId={activeTab}
          onActivate={activateTab}
          onClose={closeTab}
        />
      </main>
    </div>
  );
}
