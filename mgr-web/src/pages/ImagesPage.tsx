// ImagesPage - env-hash image registry (design §4 page 5): one row per
// recorded image with combo description, live size, build time,
// referring-sandbox count (refcount) and an expandable build-log tail.
// S3 (D3): the page gains image MANAGEMENT - per-row delete of an
// unreferenced image's tag group (base/app/code-server) and a one-shot
// "clean up" that removes every unreferenced group + the builder cache.
// Both run as mgr jobs (202 -> poll GET /api/jobs/:id inline), so progress
// stays on the page; the row delete is disabled while refcount>0 (with the
// reason in the title).
//
// S7 (prototype redesign): the table moves to the shared .table inside a
// .card (right-aligned numeric cells via .r); the head/actions already use
// the component layer (.page-head/.sec-acts/.dialog).
//
// R3: the combo cell becomes a button opening a STRUCTURED detail dialog
// (build-time scenarios + versions, always-on set, service switches) from the
// new /api/images `env`/`services` fields. R1 (this task): that detail moved
// from an inline expansion slot into ImageDetailDialog, so the row's single
// expansion slot belongs to the build log alone.

import { Fragment, useCallback, useEffect, useRef, useState } from "react";

import {
  cleanupImages,
  deleteImage,
  getFullImageLog,
  getJob,
  listImages,
  listScenarios,
} from "../api";
import { t, type Lang } from "../i18n";
import { Icon } from "../icons";
import type { Image, Scenario } from "../types";
import { fmtBytes, ImageDetailDialog } from "./ImageDetailDialog";

/** The combo column is now purely a click-to-open affordance for the
 * ImageDetailDialog — the full combo lives there, so the row shows nothing
 * but a neutral "view details" label. The previous `comboShort` truncation
 * was dropped on 09-23. */
type OpenPanel = "" | `log:${string}`;

/** R3: one row's loaded FULL log (the create job's on-disk file, or the
 * build_log column for pre-R3 rows). Loaded on first expand, then cached for
 * the lifetime of the page (a build log does not change under a finished
 * image). */
interface RowLog {
  loading: boolean;
  error: string;
  /** false = the source was the capped fallback, not the complete file. */
  complete: boolean;
  data: string;
}

export function ImagesPage({ lang }: { lang: Lang }): JSX.Element {
  const [images, setImages] = useState<Image[] | null>(null);
  const [scenarios, setScenarios] = useState<Scenario[]>([]);
  const [error, setError] = useState("");
  const [open, setOpen] = useState<OpenPanel>("");
  // R1: the row whose combo detail dialog is open (null = no dialog).
  const [detailFor, setDetailFor] = useState<Image | null>(null);
  // S3: which env_hash is being deleted right now (its row shows a spinner)
  const [deleting, setDeleting] = useState("");
  // S3: a one-shot cleanup job in flight (progress surface at the top)
  const [cleaning, setCleaning] = useState<{ job: number; done: boolean; error: string } | null>(null);
  const [confirm, setConfirm] = useState<{ kind: "delete" | "cleanup"; envHash?: string; tag?: string } | null>(null);
  // R3: per-row full build logs, keyed by env_hash (missing = not loaded).
  const [logs, setLogs] = useState<Record<string, RowLog>>({});
  const pollRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Mirror of `logs` for toggleLog's "already loaded?" read: the fetch's
  // .then closure would otherwise capture the state from ITS render, and a
  // refresh mid-load (the list polls) re-triggers the whole fetch.
  const logsRef = useRef<Record<string, RowLog>>({});

  const stopPoll = useCallback(() => {
    if (pollRef.current) {
      clearTimeout(pollRef.current);
      pollRef.current = null;
    }
  }, []);

  const refresh = useCallback(() => {
    listImages()
      .then((r) => {
        setImages(r.images);
        setError("");
      })
      .catch((e) => {
        setError(e instanceof Error ? e.message : String(e));
      });
  }, []);

  useEffect(() => {
    refresh();
    // R3: scenario catalog for the detail panel (name/version resolution).
    // A failure only degrades the panel (ids shown raw), never the page.
    listScenarios()
      .then((r) => setScenarios(r.scenarios))
      .catch(() => setScenarios([]));
    return stopPoll;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [refresh]);

  // Poll one job until terminal, then refresh + clear the in-flight marker.
  const pollJob = useCallback(
    (jobId: number) => {
      const step = async (): Promise<void> => {
        try {
          const j = await getJob(jobId);
          if (j.status === "running") {
            pollRef.current = setTimeout(() => void step(), 1200);
            return;
          }
          if (j.status === "error") {
            setError(`${t(lang, "imgJobError")}: ${j.error ?? ""}`);
          }
        } catch (e) {
          // "job not found" after an mgr restart: wipe the in-flight state.
          setCleaning((c) => (c && c.job === jobId ? { ...c, done: true } : c));
          setDeleting("");
          refresh();
          return;
        }
        setCleaning((c) => (c && c.job === jobId ? { ...c, done: true } : c));
        setDeleting("");
        refresh();
      };
      void step();
    },
    [lang, refresh],
  );

  const doDelete = async (img: Image): Promise<void> => {
    setConfirm(null);
    setDeleting(img.env_hash);
    setError("");
    try {
      const r = await deleteImage(img.env_hash);
      pollJob(r.job);
    } catch (e) {
      setDeleting("");
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const doCleanup = async (): Promise<void> => {
    setConfirm(null);
    setError("");
    try {
      const r = await cleanupImages();
      setCleaning({ job: r.job, done: false, error: "" });
      pollJob(r.job);
    } catch (e) {
      setCleaning(null);
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  /** R3: expand a row's log, loading the FULL output from the log endpoint on
   * first open (the row's `build_log` column is only a capped fallback).
   * Loaded rows are cached - a finished image's log never changes. */
  const toggleLog = (img: Image): void => {
    const want = `log:${img.env_hash}` as OpenPanel;
    const willOpen = open !== want;
    setOpen(willOpen ? want : "");
    if (!willOpen || logsRef.current[img.env_hash]) return;
    const put = (entry: RowLog): void => {
      logsRef.current = { ...logsRef.current, [img.env_hash]: entry };
      setLogs(logsRef.current);
    };
    put({ loading: true, error: "", complete: false, data: "" });
    getFullImageLog(img.env_hash)
      .then((chunk) => {
        put({ loading: false, error: "", complete: chunk.complete, data: chunk.data });
      })
      .catch((e) => {
        put({
          loading: false,
          error: e instanceof Error ? e.message : String(e),
          complete: false,
          data: "",
        });
      });
  };

  return (
    <div className="page">
      <div className="page-head">
        <h1>{t(lang, "imgTitle")}</h1>
        <div className="sec-acts">
          <button
            className="btn btn-secondary"
            title={t(lang, "imgCleanupTitle")}
            disabled={cleaning !== null || images === null || images.length === 0}
            onClick={() => setConfirm({ kind: "cleanup" })}
          >
            {cleaning !== null ? t(lang, "imgCleanupRunning") : t(lang, "imgCleanup")}
          </button>
        </div>
      </div>

      {cleaning !== null && !cleaning.done && (
        <div className="status">{t(lang, "imgCleanupRunning")}</div>
      )}

      {error && <div className="status error">{t(lang, "imgJobError")}: {error}</div>}

      {images !== null && images.length === 0 && <div className="status">{t(lang, "imgEmpty")}</div>}

      {images !== null && images.length > 0 && (
        <div className="card card-pad">
          <table className="table">
            <thead>
              <tr>
                <th>{t(lang, "imgTag")}</th>
                <th>env hash</th>
                <th>{t(lang, "imgCombo")}</th>
                <th className="r">{t(lang, "imgSize")}</th>
                <th>{t(lang, "imgBuiltAt")}</th>
                <th className="r">{t(lang, "imgRefcount")}</th>
                <th>{t(lang, "imgLog")}</th>
                <th />
              </tr>
            </thead>
          <tbody>
            {images.map((img) => {
              const built = img.built_at !== null ? new Date(img.built_at * 1000) : null;
              const referenced = img.refcount > 0;
              const busy = deleting === img.env_hash;
              const logOpen = open === `log:${img.env_hash}`;
              const rowLog: RowLog | undefined = logs[img.env_hash];
              return (
                <Fragment key={img.env_hash}>
                  <tr>
                    <td className="mono">{img.tag}</td>
                    <td className="mono" title={img.env_hash}>
                      {img.env_hash.slice(0, 12)}
                    </td>
                    <td className="mono">
                      <button
                        className="img-log-toggle"
                        aria-haspopup="dialog"
                        onClick={() => setDetailFor(img)}
                      >
                        {t(lang, "imgComboView")}
                      </button>
                    </td>
                    <td className="r">
                      {img.size_bytes !== null ? fmtBytes(img.size_bytes) : t(lang, "imgSizeUnknown")}
                    </td>
                    <td>{built !== null ? built.toLocaleString() : t(lang, "imgNotBuilt")}</td>
                    <td className="r">{img.refcount}</td>
                    <td>
                      <button
                        className="img-log-toggle"
                        aria-expanded={logOpen}
                        // Loadable when EITHER source exists: the on-disk
                        // log behind job_id or the build_log column tail
                        // (job_id is undefined against an older backend).
                        disabled={img.build_log === "" && !img.job_id}
                        onClick={() => toggleLog(img)}
                      >
                        {t(lang, "imgLog")}
                      </button>
                    </td>
                    <td>
                      <button
                        className="icon-btn"
                        aria-label={t(lang, "imgDelete")}
                        title={
                          referenced
                            ? t(lang, "imgDeleteDisableTitle").replace("{n}", String(img.refcount))
                            : t(lang, "imgDeleteTitle")
                        }
                        disabled={referenced || busy}
                        onClick={() => setConfirm({ kind: "delete", envHash: img.env_hash, tag: img.tag })}
                      >
                        {busy ? <span>{t(lang, "imgDeleteRunning")}</span> : <Icon name="trash" />}
                      </button>
                    </td>
                  </tr>
                  {logOpen && (
                    <tr>
                      <td className="img-log-cell" colSpan={8}>
                        {rowLog === undefined || rowLog.loading ? (
                          <p className="img-detail-note">{t(lang, "imgLogLoading")}</p>
                        ) : rowLog.error ? (
                          <>
                            <p className="img-detail-note">
                              {t(lang, "loadFailed")}
                              {rowLog.error}
                            </p>
                            {/* The row's own column tail still beats nothing. */}
                            <pre className="img-log">{img.build_log}</pre>
                          </>
                        ) : (
                          <pre className="img-log">{rowLog.data}</pre>
                        )}
                        {rowLog !== undefined && !rowLog.loading && !rowLog.complete && (
                          <p className="img-detail-note">{t(lang, "imgLogTailOnly")}</p>
                        )}
                      </td>
                    </tr>
                  )}
                </Fragment>
              );
            })}
          </tbody>
          </table>
        </div>
      )}

      {/* S3: delete / cleanup confirmation */}
      {confirm !== null && (
        <div className="overlay open" role="presentation" onClick={() => setConfirm(null)}>
          <div
            className="dialog"
            role="dialog"
            aria-modal="true"
            onClick={(e) => e.stopPropagation()}
          >
            <h2>
              {confirm.kind === "delete"
                ? t(lang, "imgDeleteTitle")
                : t(lang, "imgCleanupTitle")}
            </h2>
            <p className="sub">
              {confirm.kind === "delete"
                ? t(lang, "imgDeleteConfirm").replace("{tag}", confirm.tag ?? "")
                : t(lang, "imgCleanupConfirm")}
            </p>
            <div className="dialog-actions">
              <button className="btn btn-secondary" onClick={() => setConfirm(null)}>
                {t(lang, "cancel")}
              </button>
              <button
                className="btn btn-danger"
                onClick={() =>
                  confirm.kind === "delete"
                    ? void doDelete(images!.find((i) => i.env_hash === confirm.envHash)!)
                    : void doCleanup()
                }
              >
                {t(lang, "confirmDelete")}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* R1: combo detail modal (replaces the old inline expansion slot) */}
      {detailFor !== null && (
        <ImageDetailDialog
          img={detailFor}
          scenarios={scenarios}
          lang={lang}
          onClose={() => setDetailFor(null)}
        />
      )}
    </div>
  );
}
