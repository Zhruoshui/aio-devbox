// JobView - long-job progress page (design §4: "job 进度页（构建日志尾部实时
// 展示，失败展示 error）"). Polls GET /api/jobs/:id every 1.5s while running;
// the log panel shows the job's log tail (the backend caps it at 8KB,
// jobs.rs LOG_TAIL) and auto-scrolls to the bottom on update. On ok/error the
// polling stops and a done banner + "back to list" takes over; the sandbox
// list refreshes on return (its own poll catches up within 4s anyway).

import { useEffect, useRef, useState } from "react";

import { getJob } from "../api";
import { t, type Lang } from "../i18n";
import type { Job } from "../types";

const POLL_MS = 1500;

interface Props {
  jobId: number;
  flow: "create" | "recreate" | "delete";
  lang: Lang;
  onBack: () => void;
}

export function JobView({ jobId, flow, lang, onBack }: Props): JSX.Element {
  const [job, setJob] = useState<Job | null>(null);
  const [error, setError] = useState("");
  const logRef = useRef<HTMLPreElement>(null);

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let stopped = false;

    const poll = async () => {
      try {
        const j = await getJob(jobId);
        if (cancelled) return;
        setJob(j);
        setError("");
        if (j.status !== "running") {
          stopped = true;
          return;
        }
      } catch (e) {
        if (cancelled) return;
        setError(e instanceof Error ? e.message : String(e));
        // Transient poll failure (mgr restarting, etc.): keep trying while
        // we have not seen a terminal status. The job map is in-memory, so
        // a restarted mgr legitimately 400s "job not found" - then stop.
        if (/not found/i.test(e instanceof Error ? e.message : String(e))) {
          stopped = true;
        }
      }
      if (!stopped) timer = setTimeout(() => void poll(), POLL_MS);
    };

    void poll();
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
    };
  }, [jobId]);

  // Keep the newest log lines in view as the tail grows.
  useEffect(() => {
    const el = logRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [job?.log]);

  const titleKey =
    flow === "delete" ? "jobDeleting" : flow === "recreate" ? "jobRecreating" : "jobCreating";
  const done =
    job === null
      ? null
      : job.status === "ok"
        ? { ok: true as const }
        : job.status === "error"
          ? { ok: false as const }
          : null;

  return (
    <div className="page">
      <div className="job">
        <div className="job-head">
          <h1>{t(lang, titleKey)}</h1>
          <span className="job-status-line">
            {done === null ? (
              <>
                <span className="spin" aria-hidden="true" />
                <code>#{jobId}</code>
              </>
            ) : done.ok ? (
              <>
                <span className="badge badge-ok">
                  <span className="dot" />
                  {t(lang, "jobOk")}
                </span>
              </>
            ) : (
              <>
                <span className="badge badge-danger">
                  <span className="dot" />
                  {t(lang, "jobError")}
                </span>
              </>
            )}
            {job?.sandbox && <code>{job.sandbox}</code>}
          </span>
        </div>

        {done?.ok && <div className="job-ok">{t(lang, flow === "delete" ? "jobDoneDelete" : "jobDoneCreate")}</div>}

        {done && !done.ok && (
          <>
            <div className="job-error">{job?.error ?? ""}</div>
            <p className="job-status-line" style={{ margin: 0 }}>
              {t(lang, "jobFailedHint")}
            </p>
          </>
        )}

        {error && done === null && (
          <p className="job-status-line" style={{ margin: 0, color: "var(--danger)" }}>
            {t(lang, "loadFailed")}
            {error}
          </p>
        )}

        <div>
          <p className="job-log-label">{t(lang, "jobLog")}</p>
          <pre className="job-log" ref={logRef}>
            {job?.log ?? ""}
          </pre>
        </div>

        {done !== null && (
          <div className="job-actions">
            <button className="btn btn-primary" onClick={onBack}>
              <Icon14 name="chev-l" />
              {t(lang, "jobBack")}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

// Local 14px icon wrapper (the shared Icon renders 16px; this page wants the
// compact size for the back button without growing the shared component).
function Icon14({ name }: { name: "chev-l" }): JSX.Element {
  return (
    <svg className="icon" style={{ width: 14, height: 14 }} aria-hidden="true">
      <use href={`#i-${name}`} />
    </svg>
  );
}
