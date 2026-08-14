// Sidebar - left rail of toggle buttons for each enabled service, plus a
// "register button" form and a manual refresh. Collapsible to an icon rail.

import { useState } from "react";

import type { ServiceEntry } from "./types";

interface Props {
  services: ServiceEntry[];
  openTabs: string[];
  collapsed: boolean;
  onToggleCollapse: () => void;
  onToggle: (id: string) => void;
  onRefresh: () => void;
  onRegister: (label: string, cmd: string) => Promise<boolean>;
  onDelete: (id: string) => Promise<void>;
}

export function Sidebar({
  services,
  openTabs,
  collapsed,
  onToggleCollapse,
  onToggle,
  onRefresh,
  onRegister,
  onDelete,
}: Props): JSX.Element {
  const [formOpen, setFormOpen] = useState(false);
  const [label, setLabel] = useState("");
  const [cmd, setCmd] = useState("");
  const [err, setErr] = useState("");

  const enabled = services.filter((s) => s.enabled);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    const l = label.trim();
    const c = cmd.trim();
    if (!l || !c) {
      setErr("name and command required");
      return;
    }
    const ok = await onRegister(l, c);
    if (ok) {
      setLabel("");
      setCmd("");
      setErr("");
      setFormOpen(false);
    } else {
      setErr("registration failed");
    }
  };

  return (
    <aside className={`sidebar${collapsed ? " collapsed" : ""}`}>
      <div className="sb-head">
        <button className="sb-icon" title={collapsed ? "Expand" : "Collapse"} onClick={onToggleCollapse}>
          {collapsed ? "»" : "«"}
        </button>
        {!collapsed && <button className="sb-icon" title="Refresh" onClick={onRefresh}>↻</button>}
      </div>

      <nav className="sb-list">
        {enabled.map((s) => {
          const open = openTabs.includes(s.id);
          return (
            <div key={s.id} className={`sb-btn-row${open ? " open" : ""}`}>
              <button
                className="sb-btn"
                title={s.label}
                onClick={() => onToggle(s.id)}
                aria-pressed={open}
              >
                <span className="sb-glyph">{glyph(s)}</span>
                {!collapsed && <span className="sb-label">{s.label}</span>}
              </button>
              {!collapsed && s.deletable && (
                <button
                  className="sb-del"
                  title={`Remove ${s.label}`}
                  onClick={() => onDelete(s.id)}
                >
                  ✕
                </button>
              )}
            </div>
          );
        })}
        {enabled.length === 0 && !collapsed && (
          <p className="sb-empty">No buttons. Start a profile or install a tool.</p>
        )}
      </nav>

      <div className="sb-foot">
        {formOpen && !collapsed && (
          <form className="sb-form" onSubmit={submit}>
            <input
              className="sb-input"
              placeholder="name"
              value={label}
              maxLength={64}
              onChange={(e) => setLabel(e.target.value)}
            />
            <input
              className="sb-input"
              placeholder="command"
              value={cmd}
              maxLength={64}
              onChange={(e) => setCmd(e.target.value)}
            />
            {err && <span className="sb-err">{err}</span>}
            <button type="submit" className="sb-add">Add</button>
          </form>
        )}
        <button
          className="sb-icon sb-add-btn"
          title="Register a button"
          onClick={() => (collapsed ? onToggleCollapse() : setFormOpen((v) => !v))}
        >
          +
        </button>
      </div>
    </aside>
  );
}

/** A simple glyph per known button; falls back to the first letter. */
function glyph(s: ServiceEntry): string {
  switch (s.id) {
    case "codeServer":
      return "</>";
    case "vnc":
      return "🌐";
    case "terminal":
      return ">_";
    default:
      return s.label.charAt(0).toUpperCase() || "?";
  }
}
