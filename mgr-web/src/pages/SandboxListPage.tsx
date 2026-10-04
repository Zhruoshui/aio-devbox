// SandboxListPage - the sandboxes admin page, redesigned per the 09-11
// prototype (docs/Web-Prototype/sandbox-list.html).
//
// Structure: toolbar (status segmented filter + name search) over a card
// grid. Each card is the prototype's `.card.sbx`: header (live dot + name +
// status badges + "open workspace"), body (entry URL, a facts grid —
// image/resources/created for native rows, registered for adopted ones —,
// installed services chips, runtime container states), footer actions
// (start/stop/restart/edit/delete).
//
// Status filtering follows the prototype's multi-membership semantics: an
// adopted-and-running card matches BOTH the "running" and "external stack"
// segments. "stopped" is the live-not-running bucket (stopped/gone/unknown —
// anything compose doesn't report as running). Search is a plain name
// substring match, case-insensitive.
//
// Data contracts are unchanged: `live` merges compose ps state at read time,
// the page auto-refreshes every 4s, start/stop/restart are synchronous
// POSTs, delete is a confirm dialog (volumes checkbox for native rows —
// compose down -v; adopted rows are a synchronous un-registration, dialog
// says so, no JobView - types.ts DeleteReply branches on {ok} vs {job}).
//
// The model-profile quick-assign chip + popover are GONE (cc-switch refactor,
// prd D1): each sandbox owns its agent configs end-to-end, and the mgr-side
// PUT /api/sandboxes/:name/model_profile route is deleted. Agent config
// moved to the sandbox detail page (Phase 4 of the refactor).

import { useCallback, useEffect, useState } from "react";

import { deleteSandbox, listSandboxes, sandboxAction } from "../api";
import { withMgrPort } from "./workspace/paneUrl";
import { t, type Lang } from "../i18n";
import { Icon } from "../icons";
import { isJobReply, type Sandbox } from "../types";

const POLL_MS = 4000;

type Filter = "all" | "running" | "stopped" | "adopted";

/** Whether a sandbox matches the active status segment (prototype's
 * data-status multi-membership: adopted-running matches both segments). */
function matchesFilter(sb: Sandbox, f: Filter): boolean {
  switch (f) {
    case "all":
      return true;
    case "running":
      return sb.live === "running";
    case "stopped":
      return sb.live !== "running";
    case "adopted":
      return sb.adopted;
  }
}

interface Props {
  lang: Lang;
  /** Switch to the workspace focused on this sandbox (进入, D2). */
  onEnter: (name: string) => void;
  onCreate: () => void;
  onAdopt: () => void;
  onEdit: (name: string) => void;
  onJob: (jobId: number, flow: "create" | "recreate" | "delete") => void;
}

export function SandboxListPage({ lang, onEnter, onCreate, onAdopt, onEdit, onJob }: Props): JSX.Element {
  const [sandboxes, setSandboxes] = useState<Sandbox[] | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(""); // sandbox name with an action in flight
  const [confirm, setConfirm] = useState<Sandbox | null>(null);
  const [confirmVolumes, setConfirmVolumes] = useState(true);
  const [actionErr, setActionErr] = useState("");
  // Toolbar state (prototype: segmented status filter + name search).
  const [filter, setFilter] = useState<Filter>("all");
  const [q, setQ] = useState("");

  const fetchList = useCallback(async () => {
    try {
      const r = await listSandboxes();
      setSandboxes(r.sandboxes);
      setError("");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void fetchList();
    const timer = setInterval(() => void fetchList(), POLL_MS);
    return () => clearInterval(timer);
  }, [fetchList]);

  const act = async (sb: Sandbox, action: "start" | "stop" | "restart") => {
    if (busy !== "") return;
    setBusy(sb.name);
    setActionErr("");
    try {
      await sandboxAction(sb.name, action);
      await fetchList();
    } catch (e) {
      setActionErr(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy("");
    }
  };

  const doDelete = async () => {
    if (!confirm) return;
    const target = confirm;
    setConfirm(null);
    try {
      const r = await deleteSandbox(target.name, confirmVolumes);
      if (isJobReply(r)) {
        // Native row: teardown runs as a job -> progress view.
        onJob(r.job, "delete");
      } else {
        // Adopted row: synchronous un-registration - stay here and refresh.
        setBusy(target.name);
        await fetchList();
        setBusy("");
      }
    } catch (e) {
      setBusy("");
      setActionErr(e instanceof Error ? e.message : String(e));
    }
  };

  // Filtered view + segment counts (same predicate for both, so the count
  // never disagrees with what the segment shows).
  const list = sandboxes ?? [];
  const query = q.trim().toLowerCase();
  const visible = list.filter(
    (sb) => matchesFilter(sb, filter) && (query === "" || sb.name.toLowerCase().includes(query)),
  );
  const counts: Record<Filter, number> = {
    all: list.length,
    running: list.filter((sb) => matchesFilter(sb, "running")).length,
    stopped: list.filter((sb) => matchesFilter(sb, "stopped")).length,
    adopted: list.filter((sb) => matchesFilter(sb, "adopted")).length,
  };
  const segs: { id: Filter; label: string }[] = [
    { id: "all", label: t(lang, "filterAll") },
    { id: "running", label: t(lang, "stRunning") },
    { id: "stopped", label: t(lang, "stStopped") },
    { id: "adopted", label: t(lang, "sbAdopted") },
  ];

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>{t(lang, "navSandboxes")}</h1>
        </div>
        <div className="page-actions">
          <button
            className="icon-btn lg"
            onClick={() => void fetchList()}
            aria-label={t(lang, "refresh")}
            title={t(lang, "refresh")}
          >
            <Icon name="refresh" />
          </button>
          <button className="btn btn-secondary" onClick={onAdopt}>
            <Icon name="box" />
            {t(lang, "adoptExisting")}
          </button>
          <button className="btn btn-primary" onClick={onCreate}>
            <Icon name="plus" />
            {t(lang, "newSandbox")}
          </button>
        </div>
        {actionErr && <p className="sub" style={{ color: "var(--danger)" }}>{t(lang, "actionFailed")}{actionErr}</p>}
      </div>

      {error && <div className="status error">{t(lang, "loadFailed")}{error}</div>}

      {sandboxes !== null && sandboxes.length === 0 && (
        <div className="status">{t(lang, "emptySandboxes")}</div>
      )}

      {list.length > 0 && (
        <>
          <div className="toolbar">
            <div className="segmented" role="group" aria-label={t(lang, "filterByStatus")}>
              {segs.map((s) => (
                <button key={s.id} aria-pressed={filter === s.id} onClick={() => setFilter(s.id)}>
                  {s.label} <span className="cnt">{counts[s.id]}</span>
                </button>
              ))}
            </div>
            <div className="input-wrap">
              <Icon name="search" />
              <input
                className="input sm"
                type="search"
                placeholder={t(lang, "searchNamePh")}
                aria-label={t(lang, "searchNamePh")}
                value={q}
                onChange={(e) => setQ(e.target.value)}
              />
            </div>
          </div>

          <section className="sbx-grid" aria-label={t(lang, "navSandboxes")}>
            {visible.map((sb) => (
              <SandboxCard
                key={sb.name}
                sb={sb}
                lang={lang}
                busy={busy === sb.name}
                onEnter={() => onEnter(sb.name)}
                onAction={(a) => void act(sb, a)}
                onEdit={() => onEdit(sb.name)}
                onDelete={() => {
                  setConfirmVolumes(true);
                  setActionErr("");
                  setConfirm(sb);
                }}
              />
            ))}
          </section>

          {visible.length === 0 && <p className="empty">{t(lang, "sbNoMatch")}</p>}
        </>
      )}

      {/* Delete confirmation (A7: explicit confirm + volumes checkbox).
       * Adopted rows swap the copy: nothing of theirs is torn down, so no
       * volumes checkbox and an "unregister" action instead. */}
      {confirm && (
        <div className="overlay open" role="presentation" onClick={() => setConfirm(null)}>
          <div
            className="dialog"
            role="alertdialog"
            aria-modal="true"
            aria-labelledby="del-dialog-title"
            aria-describedby="del-dialog-desc"
            onClick={(e) => e.stopPropagation()}
          >
            <div>
              <h2 id="del-dialog-title">
                {confirm.adopted ? t(lang, "confirmUnadoptTitle") : t(lang, "confirmDeleteTitle")}{" "}
                <span className="mono">{confirm.name}</span>
              </h2>
              <p className="desc" id="del-dialog-desc">
                {confirm.adopted ? t(lang, "confirmUnadoptSub") : t(lang, "confirmDeleteSub")}
              </p>
            </div>
            {!confirm.adopted && (
              <label className="tsm" style={{ display: "flex", gap: "var(--space-2)", alignItems: "flex-start" }}>
                <input
                  className="check"
                  type="checkbox"
                  style={{ marginTop: 2 }}
                  checked={confirmVolumes}
                  onChange={(e) => setConfirmVolumes(e.target.checked)}
                />
                <span style={{ color: "var(--danger)" }}>
                  {t(lang, "deleteVolumes")}
                  <br />
                  <span className="muted txs">{t(lang, "sbVolLoss")}</span>
                </span>
              </label>
            )}
            <div className="dialog-actions">
              <button className="btn btn-secondary" onClick={() => setConfirm(null)}>
                {t(lang, "cancel")}
              </button>
              <button className="btn btn-danger" onClick={() => void doDelete()}>
                <Icon name="trash" />
                {confirm.adopted ? t(lang, "confirmUnadopt") : t(lang, "confirmDelete")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// ── one card ──────────────────────────────────────────────────────

function SandboxCard({
  sb,
  lang,
  busy,
  onEnter,
  onAction,
  onEdit,
  onDelete,
}: {
  sb: Sandbox;
  lang: Lang;
  busy: boolean;
  onEnter: () => void;
  onAction: (a: "start" | "stop" | "restart") => void;
  onEdit: () => void;
  onDelete: () => void;
}): JSX.Element {
  const created = new Date(sb.created_at * 1000);
  const running = sb.live === "running";

  // Resources line: "4 核 · 8192 MB" or the unlimited placeholder.
  const res =
    sb.cpus === null && sb.mem_mb === null
      ? t(lang, "sbUnlimited")
      : `${sb.cpus !== null ? `${sb.cpus} ${t(lang, "sbCores")}` : "—"} · ${sb.mem_mb !== null ? `${sb.mem_mb} MB` : "—"}`;

  return (
    <article className={`card sbx${running ? "" : " stopped"}`}>
      <header className="sbx-head">
        <span className="sdot live" title={sb.status} />
        <h2 className="sbx-name" title={sb.name}>
          {sb.name}
        </h2>
        <StatusBadges sb={sb} lang={lang} />
        <button
          className="btn btn-secondary btn-sm"
          title={t(lang, "enterWorkspace")}
          onClick={onEnter}
        >
          {t(lang, "sbEnterWs")}
          <Icon name="arrowr" />
        </button>
      </header>

      <div className="sbx-body">
        <a className="sbx-url" href={withMgrPort(sb.entry_url)} title={t(lang, "sbxUrlTitle")}>
          <span className="mono">{sb.entry_url.replace(/^https?:\/\//, "").replace(/\/+$/, "")}</span>
          <Icon name="external" />
        </a>

        <dl className="facts">
          {sb.adopted ? (
            <div>
              <dt>{t(lang, "sbRegisteredAt")}</dt>
              <dd>{created.toLocaleDateString()}</dd>
            </div>
          ) : (
            <>
              <div>
                <dt>{t(lang, "sbImage")}</dt>
                <dd>
                  <span className="mono">{sb.image}</span>
                </dd>
              </div>
              <div>
                <dt>{t(lang, "sbResources")}</dt>
                <dd>{res}</dd>
              </div>
              <div>
                <dt>{t(lang, "sbCreated")}</dt>
                <dd>{created.toLocaleDateString()}</dd>
              </div>
            </>
          )}
        </dl>

        {sb.adopted && (
          <div className="notice notice-warn" style={{ fontSize: "var(--text-xs)", padding: "6px var(--space-2)" }}>
            <Icon name="info" />
            <span>{t(lang, "sbExternalNote")}</span>
          </div>
        )}

        {sb.installed_services && !sb.adopted && (
          <div className="line">
            <span className="lbl">{t(lang, "svcOn")}</span>
            {(
              [
                ["code_server", t(lang, "svcCode_server")],
                ["vnc", t(lang, "svcVnc")],
                ["pi", t(lang, "svcPi")],
                ["pi_web", t(lang, "svcPi_web")],
              ] as const
            ).map(([key, label]) => (
              <span
                key={key}
                className={`chip${sb.installed_services[key] ? "" : " off"}`}
                title={label + " · " + t(lang, sb.installed_services[key] ? "svcOn" : "svcOff")}
              >
                <span className={`sdot${sb.installed_services[key] ? "" : " stopped"}`} />
                {label}
              </span>
            ))}
          </div>
        )}

        <div className="line">
          <span className="lbl">{t(lang, "sbContainers")}</span>
          {sb.services.length === 0 ? (
            <span>{t(lang, "stGone")}</span>
          ) : (
            sb.services.map((s) => (
              <span key={s.name} className="svc" title={s.status}>
                <span className={`sdot${s.state === "running" ? "" : " stopped"}`} />
                {s.state === "running" ? s.service : `${s.service} · ${s.state}`}
              </span>
            ))
          )}
        </div>
      </div>

      <footer className="sbx-foot">
        <button className="btn btn-ghost btn-sm" disabled={busy || running} onClick={() => onAction("start")}>
          <Icon name="play" />
          {t(lang, "start")}
        </button>
        <button className="btn btn-ghost btn-sm" disabled={busy || !running} onClick={() => onAction("stop")}>
          <Icon name="stop" />
          {t(lang, "stop")}
        </button>
        <button className="btn btn-ghost btn-sm" disabled={busy} onClick={() => onAction("restart")}>
          <Icon name="restart" />
          {t(lang, "restart")}
        </button>
        {!sb.adopted && (
          <button className="btn btn-ghost btn-sm" disabled={busy} onClick={onEdit}>
            <Icon name="edit" />
            {t(lang, "editConfig")}
          </button>
        )}
        <button className="btn btn-danger-text btn-sm del" disabled={busy} onClick={onDelete}>
          <Icon name="trash" />
          {sb.adopted ? t(lang, "confirmUnadopt") : t(lang, "delete")}
        </button>
      </footer>
    </article>
  );
}

/** Combined status: DB intent (status) + live compose state (live). The DB
 * status is the source of truth for creating/error; live explains running
 * vs stopped vs gone for everything else. */
function StatusBadges({ sb, lang }: { sb: Sandbox; lang: Lang }): JSX.Element {
  const statusLabel: Record<string, { key: StatusKey; cls: string }> = {
    creating: { key: "stCreating", cls: "badge-info" },
    error: { key: "stError", cls: "badge-danger" },
    running: { key: "stRunning", cls: "badge-ok" },
    stopped: { key: "stStopped", cls: "badge-neutral" },
  };
  const liveLabel: Record<string, { key: StatusKey; cls: string }> = {
    running: { key: "stRunning", cls: "badge-ok" },
    stopped: { key: "stStopped", cls: "badge-neutral" },
    gone: { key: "stGone", cls: "badge-warn" },
    unknown: { key: "stUnknown", cls: "badge-warn" },
  };
  const s = statusLabel[sb.status];
  const l = liveLabel[sb.live];
  return (
    <span style={{ display: "inline-flex", gap: "var(--space-1)", flexShrink: 0 }}>
      {sb.adopted && <span className="badge badge-neutral">{t(lang, "sbAdopted")}</span>}
      {s && (
        <span className={`badge ${s.cls}`}>
          <span className="dot" />
          {t(lang, s.key)}
        </span>
      )}
      {l && (!s || sb.status !== sb.live) && (
        <span className={`badge ${l.cls}`}>
          <span className="dot" />
          {t(lang, l.key)}
        </span>
      )}
    </span>
  );
}

type StatusKey =
  | "stRunning"
  | "stStopped"
  | "stCreating"
  | "stError"
  | "stGone"
  | "stUnknown"
  | "stAdopted";
