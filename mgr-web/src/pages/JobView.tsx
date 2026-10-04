// JobView - long-job progress page (design §4: "job 进度页（构建日志尾部实时
// 展示，失败展示 error）"). Polls GET /api/jobs/:id every 1.5s while running;
// the log panel shows the job's in-memory tail (mgr caps it at 64KB, jobs.rs
// LOG_TAIL), and "Jump to top" loads the FULL on-disk log
// (GET /api/jobs/:id/log, R3) so a build can be read end to end - including
// while it runs (the endpoint reads the file the runner is appending to).
// "Back to list" returns to the sandbox list, which refreshes on its own
// within one poll period.
//
// R3 scroll contract: the panel FOLLOWS the tail by default (each new poll
// scrolls to the bottom); scrolling up pauses the follow so the user can read
// earlier output while the build keeps appending; returning to the bottom
// resumes it. "Jump to top" loads the full log and pins the view to the
// start; "Back to bottom" appears while the follow is paused.
//
// S7 (prototype redesign): standard .page-head (h1 + job id/sandbox sub,
// status badge or spinner in .page-actions); the body keeps the .job column
// (banners + log tail + back).

import { useCallback, useEffect, useRef, useState } from "react";

import { getFullJobLog, getJob } from "../api";
import { t, type Lang } from "../i18n";
import type { Job } from "../types";

const POLL_MS = 1500;

/** Distance from the bottom (px) that still counts as "at the bottom" - a
 * couple of lines of slack so the follow does not flicker on sub-pixel
 * layout, and so the user does not have to land on the exact last pixel. */
const BOTTOM_SLACK = 40;

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
  // R3: the follow state lives in a ref (not state) - the scroll handler and
  // the log effect both read it, and a re-render per scroll event would be
  // pure noise.
  const followRef = useRef(true);
  // R3: the loaded full log (null = still showing the live tail).
  const [fullLog, setFullLog] = useState<string | null>(null);
  // Whether that load came from the on-disk file (true) or the capped
  // in-memory tail / build_log column fallback (false, the backend's
  // `complete`) - the label must not call a 64KB tail "the complete log".
  const [fullComplete, setFullComplete] = useState(false);
  const [loadingFull, setLoadingFull] = useState(false);
  // A failed full-log load is reported on its own channel: it must not be
  // conflated with the poll error (whose display is gated on the job still
  // being in flight) nor silently swallowed once the job is done.
  const [logError, setLogError] = useState("");
  // Mirrors followRef into state for the "back to bottom" button's visibility.
  const [following, setFollowing] = useState(true);
  // The status the LOADED full log was captured at ("running" = a mid-build
  // snapshot that must be re-read once the job settles; "done" = final), the
  // latest job status (readable from callbacks that must not re-create on
  // every poll), and the latest loader for the poll's terminal branch (that
  // effect keys on [jobId] alone, so it must not close over a per-render
  // callback). All live in refs for the same reason `followRef` does.
  const fullAt = useRef<"running" | "done" | null>(null);
  const statusRef = useRef<string | null>(null);
  statusRef.current = job?.status ?? null;
  const loadFullRef = useRef<() => Promise<void>>(async () => {});

  const setFollow = useCallback((v: boolean) => {
    followRef.current = v;
    setFollowing(v);
  }, []);

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
          // R3: the file is final now, so a full log the user loaded WHILE
          // the build was running is a stale snapshot - re-read it once, in
          // place, so the panel never presents a mid-build capture as the
          // finished output. Nothing is fetched for a job the user has not
          // asked about: rendering a multi-MB build from the start stays the
          // explicit "jump to top" action.
          if (fullAt.current === "running") void loadFullRef.current();
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

  // Keep the newest lines in view as the tail grows - but ONLY while the
  // follow is on: a user who scrolled up is reading, and yanking them back
  // to the bottom every 1.5s is exactly the bug R3 fixes.
  useEffect(() => {
    const el = logRef.current;
    if (el && followRef.current && fullLog === null) el.scrollTop = el.scrollHeight;
  }, [job?.log, fullLog]);

  // Track the scroll position: at (or near) the bottom -> follow; anywhere
  // else -> paused. This fires for user scrolls AND for our own programmatic
  // scroll-to-bottom, which is fine (it just re-confirms the follow).
  const onScroll = useCallback((): void => {
    const el = logRef.current;
    if (!el) return;
    const atBottom = el.scrollTop + el.clientHeight >= el.scrollHeight - BOTTOM_SLACK;
    if (atBottom !== followRef.current) setFollow(atBottom);
  }, [setFollow]);

  /** R3: load the job's whole log into the panel. `scrollToTop` separates the
   * explicit "jump to top" button from the in-place re-read when a snapshot
   * becomes stale. */
  const showFullLog = useCallback(
    async (scrollToTop: boolean): Promise<void> => {
      setLoadingFull(true);
      setLogError("");
      try {
        const chunk = await getFullJobLog(jobId);
        setFullLog(chunk.data);
        setFullComplete(chunk.complete);
        // Remember WHICH state this capture came from: a running job's file
        // keeps growing, so that load is a snapshot, not the final output.
        fullAt.current = statusRef.current === "running" ? "running" : "done";
        if (scrollToTop) {
          // Pausing the follow is the point of the button: new polls must
          // not pull the view back down.
          setFollow(false);
          // After the (bigger) content is committed, scroll to the start.
          requestAnimationFrame(() => {
            if (logRef.current) logRef.current.scrollTop = 0;
          });
        }
      } catch (e) {
        setLogError(e instanceof Error ? e.message : String(e));
      } finally {
        setLoadingFull(false);
      }
    },
    [jobId, setFollow],
  );
  loadFullRef.current = () => showFullLog(false);

  /** R3 "jump to top": the whole log, pinned to its start. Works while
   * running too - the endpoint reads the file the runner is appending to. */
  const jumpToTop = useCallback((): void => {
    void showFullLog(true);
  }, [showFullLog]);

  // The full log is loadable when the job wrote one, or when it is no longer
  // running (the endpoint then serves whatever it has - the memory tail).
  // A RUNNING job without a file has nothing the panel is not already
  // showing, so the button is disabled instead of paying a pointless round
  // trip that also pauses the follow.
  const canJumpTop = job !== null && (job.status !== "running" || !!job.log_path);

  /** R3 "back to bottom": resume following. Only a RUNNING job drops the
   * loaded full log - for a finished one it is strictly better (complete
   * history), so we keep it and just pin the view to the end. */
  const backToBottom = useCallback((): void => {
    if (statusRef.current === "running") {
      setFullLog(null);
      fullAt.current = null;
    }
    setFollow(true);
    requestAnimationFrame(() => {
      if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight;
    });
  }, [setFollow]);

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
      <div className="page-head">
        <div>
          <h1>{t(lang, titleKey)}</h1>
          <p className="sub mono">
            #{jobId}
            {job?.sandbox ? ` · ${job.sandbox}` : ""}
          </p>
        </div>
        <div className="page-actions">
          {done === null ? (
            <span className="spinner" aria-hidden="true" />
          ) : done.ok ? (
            <span className="badge badge-ok">
              <span className="dot" />
              {t(lang, "jobOk")}
            </span>
          ) : (
            <span className="badge badge-danger">
              <span className="dot" />
              {t(lang, "jobError")}
            </span>
          )}
        </div>
      </div>

      <div className="job">
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
          <div className="status error">{t(lang, "loadFailed")}{error}</div>
        )}

        <div>
          <div className="job-log-head">
            <p className="job-log-label">{t(lang, "jobLog")}</p>
            <div className="job-log-controls">
              <button
                className="btn btn-secondary btn-sm"
                disabled={loadingFull || !canJumpTop}
                onClick={jumpToTop}
                title={
                  canJumpTop ? t(lang, "jobLogTopTitle") : t(lang, "jobLogTopNoFile")
                }
              >
                {loadingFull ? t(lang, "jobLogLoading") : t(lang, "jobLogTop")}
              </button>
              {!following && (
                <button className="btn btn-secondary btn-sm" onClick={backToBottom}>
                  {t(lang, "jobLogBottom")}
                </button>
              )}
            </div>
          </div>
          <pre className="job-log" ref={logRef} onScroll={onScroll}>
            {fullLog ?? job?.log ?? ""}
          </pre>
          <p className="job-log-label">
            {logError
              ? `${t(lang, "loadFailed")}${logError}`
              : fullLog !== null
                ? t(lang, fullComplete ? "jobLogFull" : "jobLogTailOnly")
                : following
                  ? t(lang, "jobLogFollowing")
                  : t(lang, "jobLogPaused")}
          </p>
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
