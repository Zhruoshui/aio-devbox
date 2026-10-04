// ImageDetailDialog - R1: the images page's combo detail moves out of the
// table's single per-row expansion slot (which now belongs to the build log
// alone) into a centered modal. Chrome (.overlay.open > .dialog, Escape /
// scrim / close button close, focus moves in on open and back to the opener
// on unmount) is literally components/Dialogs.tsx's shared useDialogChrome
// hook, so it cannot drift from ConfirmDialog; styling comes from the shared
// `.dialog` classes plus the .img-detail* block already used by the old
// inline panel.
//
// Content = a head metadata block (tag / full env_hash / size / built_at /
// refcount) + the R3 structured detail (build-time scenarios + versions,
// always-on set, service switches). Rows recorded before R3 (env/services
// null) show the imgDetailEmpty note instead of empty sections.

import { useRef } from "react";

import { useDialogChrome } from "../components/Dialogs";
import { t, type Lang } from "../i18n";
import type { Image, Scenario } from "../types";

/** Byte formatter shared with the images table's size column - the dialog
 * repeats the value in its metadata block, so it lives here and ImagesPage
 * imports it rather than keeping two copies in sync. */
export function fmtBytes(b: number): string {
  if (b >= 1 << 30) return `${(b / (1 << 30)).toFixed(1)} GiB`;
  if (b >= 1 << 20) return `${(b / (1 << 20)).toFixed(1)} MiB`;
  if (b >= 1024) return `${(b / 1024).toFixed(1)} KiB`;
  return `${b} B`;
}

interface Props {
  img: Image;
  /** Scenario catalog (GET /api/scenarios) - resolves ids to names and feeds
   * the always-on set. A failed catalog load degrades to raw ids, never
   * blocks the dialog. */
  scenarios: Scenario[];
  lang: Lang;
  onClose: () => void;
}

export function ImageDetailDialog({ img, scenarios, lang, onClose }: Props): JSX.Element {
  // Chrome (opener capture, initial focus, Escape, focus restore) is the
  // SAME hook ConfirmDialog/PromptDialog use - no second copy of the
  // contract to drift out of sync.
  const closeRef = useRef<HTMLButtonElement>(null);
  useDialogChrome(onClose, closeRef);

  return (
    <div className="overlay open" role="presentation" onClick={onClose}>
      <div
        className="dialog img-detail-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="dlg-img-detail-title"
        onClick={(e) => e.stopPropagation()}
      >
        <h2 id="dlg-img-detail-title">{t(lang, "imgDetail")}</h2>
        <ImageDetailMeta img={img} lang={lang} />
        <ImageDetailBody img={img} scenarios={scenarios} lang={lang} />
        <div className="dialog-actions">
          <button ref={closeRef} className="btn btn-secondary" onClick={onClose}>
            {t(lang, "imgDetailClose")}
          </button>
        </div>
      </div>
    </div>
  );
}

/** Head block: the identity/size/timestamp facts of the row that the table
 * can only show truncated (env_hash) or compactly. */
function ImageDetailMeta({ img, lang }: { img: Image; lang: Lang }): JSX.Element {
  const built = img.built_at !== null ? new Date(img.built_at * 1000) : null;
  const row = (label: string, value: string, mono = false): JSX.Element => (
    <p className="img-detail-row">
      <span className="img-detail-name">{label}</span>
      <span className={`img-detail-meta${mono ? " mono" : ""}`}>{value}</span>
    </p>
  );
  return (
    <div className="img-detail-meta-block">
      {row(t(lang, "imgTag"), img.tag, true)}
      {row(t(lang, "imgEnvHash"), img.env_hash, true)}
      {row(
        t(lang, "imgSize"),
        img.size_bytes !== null ? fmtBytes(img.size_bytes) : t(lang, "imgSizeUnknown"),
      )}
      {row(t(lang, "imgBuiltAt"), built !== null ? built.toLocaleString() : t(lang, "imgNotBuilt"))}
      {row(t(lang, "imgRefcount"), String(img.refcount))}
    </div>
  );
}

/** R3 structured combo detail: scenarios + versions, always-on set, service
 * switches. env/services are null on pre-R3 rows - those get the fallback
 * note instead of empty sections. */
function ImageDetailBody({
  img,
  scenarios,
  lang,
}: {
  img: Image;
  scenarios: Scenario[];
  lang: Lang;
}): JSX.Element {
  const env = img.env;
  const services = img.services;
  if (env === null || services === null) {
    return <p className="img-detail-note">{t(lang, "imgDetailEmpty")}</p>;
  }
  const byId = new Map(scenarios.map((s) => [s.id, s]));
  const alwaysOn = scenarios.filter((s) => s.always_on && !["pi", "pi-web"].includes(s.id));
  const svc = (on: boolean): string => (on ? "✓" : "✗");
  return (
    <div className="img-detail">
      <p className="img-detail-sec">{t(lang, "imgScenarios")}</p>
      {env.scenarios.length === 0 && <p className="img-detail-note">{t(lang, "imgNoSelection")}</p>}
      {env.scenarios.map((id) => {
        const s = byId.get(id);
        const label = env.versions[id];
        return (
          <p key={id} className="img-detail-row">
            <span className="img-detail-name">{s?.name ?? id}</span>
            <span className="img-detail-meta mono">
              {s && s.name !== id ? id : ""}
              {label ? ` · ${label}` : ""}
            </span>
          </p>
        );
      })}
      <p className="img-detail-sec">{t(lang, "imgAlwaysOn")}</p>
      {alwaysOn.map((s) => {
        const label = env.versions[s.id] ?? s.default_version ?? s.versions[0] ?? "";
        return (
          <p key={s.id} className="img-detail-row">
            <span className="img-detail-name">{s.name}</span>
            <span className="img-detail-meta mono">{label}</span>
          </p>
        );
      })}
      <p className="img-detail-sec">{t(lang, "imgServices")}</p>
      <p className="img-detail-row mono">
        code-server {svc(services.code_server)} · vnc {svc(services.vnc)} · pi{" "}
        {svc(env.scenarios.includes("pi"))} · pi-web {svc(env.scenarios.includes("pi-web"))}
      </p>
    </div>
  );
}
