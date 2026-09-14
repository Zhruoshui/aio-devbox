// NodeMenu - the sandbox tree node's "more" menu (prototype workspace.html
// `.menu`, fixed-positioned so the side panel can't clip it). Actions:
// open a terminal (launches the sandbox's terminal service), view the
// sandbox in the list (onManage), start/stop/restart (sandboxAction, owned
// by WorkspacePage), edit config (App-level navigation) and register a
// custom button (RegisterDialog). Disabled states follow live state: a
// stopped sandbox can't open panes or stop again.
//
// A11y: role="menu" with buttons role="menuitem"; Escape and outside
// clicks close (focus returns to the opener). The menu is rendered through
// a portal-free fixed div at the page level (z-index above the panel).

import { useEffect, useRef } from "react";

import { t, type Lang } from "../../i18n";
import { Icon } from "../../icons";
import type { Sandbox } from "../../types";

/** Menu item glyphs (kept as a named union for the items array's typing). */
export type NodeMenuIcon = "terminal" | "dock" | "play" | "stop" | "restart" | "edit" | "plus";

interface Props {
  lang: Lang;
  sandbox: Sandbox;
  /** Anchor rect from the trigger button (fixed positioning base). */
  anchor: DOMRect;
  /** Whether an action (start/stop/restart) is currently in flight. */
  busy: boolean;
  onClose: () => void;
  onOpenTerminal: () => void;
  onViewInList: () => void;
  onAction: (action: "start" | "stop" | "restart") => void;
  onEdit: () => void;
  onRegister: () => void;
}

export function NodeMenu({
  lang,
  sandbox,
  anchor,
  busy,
  onClose,
  onOpenTerminal,
  onViewInList,
  onAction,
  onEdit,
  onRegister,
}: Props): JSX.Element {
  const ref = useRef<HTMLDivElement>(null);
  const running = sandbox.live === "running";

  // Close on Escape (return focus to opener via caller) and on outside
  // pointerdown — the same dismissal contract as the prototype's
  // document-level listeners, scoped to this menu's lifetime.
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") onClose();
    };
    const onPointerDown = (e: PointerEvent): void => {
      if (ref.current !== null && !ref.current.contains(e.target as Node)) onClose();
    };
    document.addEventListener("keydown", onKey);
    document.addEventListener("pointerdown", onPointerDown);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("pointerdown", onPointerDown);
    };
  }, [onClose]);

  const items: ({ kind: "hr" } | { kind: "item"; id: string; label: string; icon: NodeMenuIcon; disabled?: boolean })[] = [
    { kind: "item", id: "terminal", label: t(lang, "openTerminal"), icon: "terminal", disabled: !running },
    { kind: "item", id: "view", label: t(lang, "viewInList"), icon: "dock" },
    { kind: "hr" },
    { kind: "item", id: "start", label: t(lang, "start"), icon: "play", disabled: running || busy },
    { kind: "item", id: "stop", label: t(lang, "stop"), icon: "stop", disabled: !running || busy },
    { kind: "item", id: "restart", label: t(lang, "restart"), icon: "restart", disabled: !running || busy },
    ...(sandbox.adopted
      ? []
      : [
          { kind: "item" as const, id: "edit", label: t(lang, "editConfig"), icon: "edit" as const },
          { kind: "hr" as const },
          { kind: "item" as const, id: "register", label: t(lang, "register"), icon: "plus" as const, disabled: !running },
        ]),
  ];

  const run = (id: string): void => {
    switch (id) {
      case "terminal":
        onOpenTerminal();
        break;
      case "view":
        onViewInList();
        break;
      case "start":
      case "stop":
      case "restart":
        onAction(id);
        break;
      case "edit":
        onEdit();
        break;
      case "register":
        onRegister();
        break;
    }
    onClose();
  };

  // Fixed position clamped to the viewport (prototype: left/top near the
  // trigger, never off-screen; menu width ~180px, height bounded ~320px).
  const left = Math.min(anchor.left, window.innerWidth - 190);
  const top = Math.min(anchor.bottom + 4, window.innerHeight - 320);

  return (
    <div ref={ref} className="menu open" role="menu" style={{ position: "fixed", left, top }}>
      {items.map((item, i) =>
        item.kind === "hr" ? (
          <hr key={`hr-${i}`} />
        ) : (
          <button key={item.id} role="menuitem" disabled={item.disabled} onClick={() => run(item.id)}>
            <Icon name={item.icon} />
            {item.label}
          </button>
        ),
      )}
    </div>
  );
}
