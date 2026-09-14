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

import { Fragment, useCallback, useEffect, useRef, useState } from "react";

import { cleanupImages, deleteImage, getJob, listImages } from "../api";
import { t, type Lang } from "../i18n";
import { Icon } from "../icons";
import type { Image } from "../types";

export function ImagesPage({ lang }: { lang: Lang }): JSX.Element {
  const [images, setImages] = useState<Image[] | null>(null);
  const [error, setError] = useState("");
  const [open, setOpen] = useState(""); // env_hash whose log is expanded
  // S3: which env_hash is being deleted right now (its row shows a spinner)
  const [deleting, setDeleting] = useState("");
  // S3: a one-shot cleanup job in flight (progress surface at the top)
  const [cleaning, setCleaning] = useState<{ job: number; done: boolean; error: string } | null>(null);
  const [confirm, setConfirm] = useState<{ kind: "delete" | "cleanup"; envHash?: string; tag?: string } | null>(null);
  const pollRef = useRef<ReturnType<typeof setTimeout> | null>(null);

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

  const fmtBytes = (b: number): string => {
    if (b >= 1 << 30) return `${(b / (1 << 30)).toFixed(1)} GiB`;
    if (b >= 1 << 20) return `${(b / (1 << 20)).toFixed(1)} MiB`;
    if (b >= 1024) return `${(b / 1024).toFixed(1)} KiB`;
    return `${b} B`;
  };

  return (
    <div className="page">
      <div className="page-head">
        <h1>{t(lang, "imgTitle")}</h1>
        <p className="sub">{t(lang, "imgSub")}</p>
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
                <th>{t(lang, "imgSize")}</th>
                <th>{t(lang, "imgBuiltAt")}</th>
                <th className="r">{t(lang, "imgRefcount")}</th>
                <th>{t(lang, "imgLog")}</th>
                <th />
              </tr>
            </thead>
          <tbody>
            {images.map((img) => {
              const built = img.built_at !== null ? new Date(img.built_at * 1000) : null;
              const isOpen = open === img.env_hash;
              const referenced = img.refcount > 0;
              const busy = deleting === img.env_hash;
              return (
                <Fragment key={img.env_hash}>
                  <tr>
                    <td className="mono">{img.tag}</td>
                    <td className="mono" title={img.env_hash}>
                      {img.env_hash.slice(0, 12)}
                    </td>
                    <td className="mono" title={img.combo ?? img.env_hash}>
                      {img.combo ?? img.env_hash.slice(0, 12)}
                    </td>
                    <td className="r">
                      {img.size_bytes !== null ? fmtBytes(img.size_bytes) : t(lang, "imgSizeUnknown")}
                    </td>
                    <td>{built !== null ? built.toLocaleString() : t(lang, "imgNotBuilt")}</td>
                    <td className="r">{img.refcount}</td>
                    <td>
                      <button
                        className="img-log-toggle"
                        aria-expanded={isOpen}
                        disabled={img.build_log === ""}
                        onClick={() => setOpen(isOpen ? "" : img.env_hash)}
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
                  {isOpen && (
                    <tr>
                      <td className="img-log-cell" colSpan={8}>
                        <pre className="img-log">{img.build_log}</pre>
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
    </div>
  );
}

