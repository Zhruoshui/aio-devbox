// ImagesPage - env-hash image registry (design §4 page 5): one row per
// recorded image with tag, build time, referring-sandbox count (refcount) and
// an expandable build-log tail (db images.build_log - recorded by the create
// job's base-image build). Manual delete is deliberately NOT offered here
// (design §3.6: "旧镜像引用归零不自动删（镜像列表页手动清理）" defers to
// `docker rmi` on the host; nothing in the current API deletes images).

import { Fragment, useEffect, useState } from "react";

import { listImages } from "../api";
import { t, type Lang } from "../i18n";
import type { Image } from "../types";

export function ImagesPage({ lang }: { lang: Lang }): JSX.Element {
  const [images, setImages] = useState<Image[] | null>(null);
  const [error, setError] = useState("");
  const [open, setOpen] = useState(""); // env_hash whose log is expanded

  useEffect(() => {
    let cancelled = false;
    listImages()
      .then((r) => {
        if (!cancelled) setImages(r.images);
      })
      .catch((e) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <div className="page">
      <div className="page-head">
        <h1>{t(lang, "imgTitle")}</h1>
        <p className="sub">{t(lang, "imgSub")}</p>
      </div>

      {error && <div className="status error">{t(lang, "loadFailed")}{error}</div>}
      {images !== null && images.length === 0 && <div className="status">{t(lang, "imgEmpty")}</div>}

      {images !== null && images.length > 0 && (
        <table className="img-table">
          <thead>
            <tr>
              <th>{t(lang, "imgTag")}</th>
              <th>env hash</th>
              <th>{t(lang, "imgBuiltAt")}</th>
              <th style={{ textAlign: "right" }}>{t(lang, "imgRefcount")}</th>
              <th>{t(lang, "imgLog")}</th>
            </tr>
          </thead>
          <tbody>
            {images.map((img) => {
              const built = img.built_at !== null ? new Date(img.built_at * 1000) : null;
              const isOpen = open === img.env_hash;
              return (
                // One image = a row pair (summary + expandable log); the key
                // belongs on the Fragment, not the inner <tr>s, or React
                // warns and mis-diffs when the log row toggles.
                <Fragment key={img.env_hash}>
                  <tr>
                    <td className="mono">{img.tag}</td>
                    <td className="mono" title={img.env_hash}>
                      {img.env_hash.slice(0, 12)}
                    </td>
                    <td>{built !== null ? built.toLocaleString() : t(lang, "imgNotBuilt")}</td>
                    <td className="num">{img.refcount}</td>
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
                  </tr>
                  {isOpen && (
                    <tr>
                      <td className="img-log-cell" colSpan={5}>
                        <pre className="img-log">{img.build_log}</pre>
                      </td>
                    </tr>
                  )}
                </Fragment>
              );
            })}
          </tbody>
        </table>
      )}
    </div>
  );
}
