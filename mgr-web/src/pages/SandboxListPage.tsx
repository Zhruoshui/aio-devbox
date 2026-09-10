// SandboxListPage - the sandboxes admin page (design §4 page 1).
//
// Cards per sandbox: name + status badges (DB intent `status` + live compose
// state `live`), resources, image tag, creation date, running services, and
// the entry button ("进入沙箱" - switches to the WORKSPACE page focused on
// that sandbox, prd D2: no more new-tab per-sandbox workbench; the unified
// workspace embeds every sandbox's panes). Actions: start/stop/restart
// (synchronous POSTs), edit config (parent switches to the env editor) and
// delete (confirm dialog with the volumes checkbox - volumes=1 runs compose
// down -v, A7).
//
// Adopted (external) stacks: same card minus the edit button; their delete is
// a synchronous UN-REGISTRATION (dialog says so, no volumes checkbox, no
// JobView - types.ts DeleteReply branches on {ok} vs {job}). The page header
// carries the adopt-wizard entry next to "new sandbox".
//
// The list auto-refreshes every 4s while mounted: `live` merges compose ps
// state at read time, and a sandbox created via the job view should appear
// (or transition running) without a manual reload (useStats-style polling in
// state-management.md's spirit: local hook, results into state).

import { useCallback, useEffect, useState } from "react";

import { deleteSandbox, listModelProfiles, listSandboxes, sandboxAction } from "../api";
import { withMgrPort } from "./workspace/paneUrl";
import { t, type Lang } from "../i18n";
import { Icon } from "../icons";
import { isJobReply, type Sandbox } from "../types";

const POLL_MS = 4000;

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
  // id -> display name for the cards' model-profile chip (D8); fetched once
  // per mount - profile renames without a page visit are not a real case.
  const [profileNames, setProfileNames] = useState<Record<string, string> | null>(null);

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

  useEffect(() => {
    let cancelled = false;
    listModelProfiles()
      .then((r) => {
        if (cancelled) return;
        const names: Record<string, string> = {};
        for (const p of r.profiles) names[p.id] = p.name;
        setProfileNames(names);
      })
      .catch(() => {
        /* advisory chip — an error surface here would be noise */
      });
    return () => {
      cancelled = true;
    };
  }, []);

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

  return (
    <div className="page">
      <div className="page-head">
        <h1>{t(lang, "navSandboxes")}</h1>
        <div className="page-actions">
          <button
            className="btn btn-ghost btn-sm"
            onClick={() => void fetchList()}
            aria-label={t(lang, "refresh")}
            title={t(lang, "refresh")}
          >
            <Icon name="refresh" />
          </button>
          <button className="btn btn-ghost" onClick={onAdopt}>
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

      {sandboxes !== null && sandboxes.length > 0 && (
        <div className="sbx-grid">
          {sandboxes.map((sb) => (
            <SandboxCard
              key={sb.name}
              sb={sb}
              lang={lang}
              profileNames={profileNames}
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
        </div>
      )}

      {/* Delete confirmation (A7: explicit confirm + volumes checkbox).
       * Adopted rows swap the copy: nothing of theirs is torn down, so no
       * volumes checkbox and an "unregister" action instead. */}
      {confirm && (
        <div className="overlay" role="presentation" onClick={() => setConfirm(null)}>
          <div
            className="dialog"
            role="dialog"
            aria-modal="true"
            aria-label={confirm.adopted ? t(lang, "confirmUnadoptTitle") : t(lang, "confirmDeleteTitle")}
            onClick={(e) => e.stopPropagation()}
          >
            <h2>
              {confirm.adopted ? t(lang, "confirmUnadoptTitle") : t(lang, "confirmDeleteTitle")} —{" "}
              <code>{confirm.name}</code>
            </h2>
            <p className="sub">
              {confirm.adopted ? t(lang, "confirmUnadoptSub") : t(lang, "confirmDeleteSub")}
            </p>
            {!confirm.adopted && (
              <label className="scn-row" style={{ border: 0, padding: 0, marginBottom: "var(--space-4)" }}>
                <input
                  className="check"
                  type="checkbox"
                  checked={confirmVolumes}
                  onChange={(e) => setConfirmVolumes(e.target.checked)}
                />
                <span style={{ fontSize: "var(--text-sm)", color: "var(--danger)" }}>
                  {t(lang, "deleteVolumes")}
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
  profileNames,
  busy,
  onEnter,
  onAction,
  onEdit,
  onDelete,
}: {
  sb: Sandbox;
  lang: Lang;
  /** id -> display name for the model-profile chip; null = not loaded yet. */
  profileNames: Record<string, string> | null;
  busy: boolean;
  onEnter: () => void;
  onAction: (a: "start" | "stop" | "restart") => void;
  onEdit: () => void;
  onDelete: () => void;
}): JSX.Element {
  const created = new Date(sb.created_at * 1000);
  return (
    <article className="sbx-card">
      <div className="sbx-head">
        <span className="sbx-name" title={sb.name}>
          {sb.name}
        </span>
        <StatusBadges sb={sb} lang={lang} />
        <a
          className="sbx-entry"
          href={withMgrPort(sb.entry_url)}
          title={t(lang, "enterWorkspace")}
          onClick={(e) => {
            e.preventDefault();
            onEnter();
          }}
        >
          <Icon name="dock" />
          {t(lang, "enter")}
        </a>
      </div>

      <div className="sbx-meta">
        <span>
          {t(lang, "sbCpus")}:{" "}
          <code>{sb.cpus !== null ? String(sb.cpus) : t(lang, "sbUnlimited")}</code>
        </span>
        <span>
          {t(lang, "sbMem")}:{" "}
          <code>
            {sb.mem_mb !== null ? `${sb.mem_mb}M` : t(lang, "sbUnlimited")}
          </code>
        </span>
        <span>
          {t(lang, "sbImage")}: <code>{sb.image}</code>
        </span>
        <span>
          {t(lang, "sbCreated")}: <code>{created.toLocaleDateString()}</code>
        </span>
        <span>
          {t(lang, "mpAssignTo")}:{" "}
          <code>
            {sb.model_profile === null
              ? t(lang, "sbProfileNone")
              : (profileNames?.[sb.model_profile] ?? sb.model_profile)}
          </code>
        </span>
      </div>

      {sb.adopted && (
        <p className="sbx-note">
          {t(lang, "sbAdopted")} — {t(lang, "sbExternalNote")}
        </p>
      )}

      <div className="sbx-services">
        {sb.services.length === 0 ? (
          <span>{t(lang, "stGone")}</span>
        ) : (
          sb.services.map((s) => (
            <span key={s.name} title={s.status}>
              {s.service}: {s.state}
            </span>
          ))
        )}
      </div>

      <div className="sbx-actions">
        <button
          className="btn btn-secondary btn-sm"
          disabled={busy || sb.live === "running"}
          onClick={() => onAction("start")}
        >
          <Icon name="play" />
          {t(lang, "start")}
        </button>
        <button
          className="btn btn-secondary btn-sm"
          disabled={busy || sb.live !== "running"}
          onClick={() => onAction("stop")}
        >
          <Icon name="stop" />
          {t(lang, "stop")}
        </button>
        <button
          className="btn btn-secondary btn-sm"
          disabled={busy}
          onClick={() => onAction("restart")}
        >
          <Icon name="restart" />
          {t(lang, "restart")}
        </button>
        {!sb.adopted && (
          <button className="btn btn-secondary btn-sm" disabled={busy} onClick={onEdit}>
            <Icon name="edit" />
            {t(lang, "editConfig")}
          </button>
        )}
        <button
          className="btn btn-danger-text btn-sm"
          disabled={busy}
          onClick={onDelete}
          style={{ marginLeft: "auto" }}
        >
          <Icon name="trash" />
          {t(lang, "delete")}
        </button>
      </div>
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
    adopted: { key: "stAdopted", cls: "badge-neutral" },
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
