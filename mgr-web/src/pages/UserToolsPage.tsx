// UserToolsPage - R2 + 修正 2/3: a single direct-edit text editor over the
// user-tools fragment (`usertools/fragment.Dockerfile`, repo root), the escape
// hatch for tools mise cannot manage.
//
// 修正 2 removed the structured form: the file IS the source of truth, so the
// page is one monospace textarea with a small-editor affordance set —
//   * Tab / Shift+Tab indent / dedent two spaces (textarea default moves focus),
//   * a status line (lines · bytes · modified/saved),
//   * a tool count parsed from the `# --- tool:` marker blocks, plus the L5
//     picker list the create wizard offers (修正 3).
//
// 修正 3: saving also syncs the per-tool fragment dirs (`usertools/<tool>/`)
// and the tool metadata, so the new tool set is selectable at sandbox creation
// immediately (the backend owns that sync; this page just re-GETs after save).
//
// The three hard rules stay visible above the editor (they also live in the
// file's own header). Rebuild remains a manual Makefile step (PRD non-goal:
// mgr never triggers a rebuild).

import { useCallback, useEffect, useRef, useState } from "react";

import { getUserTools, putUserTools } from "../api";
import { t, type Lang } from "../i18n";
import type { UserToolsFile } from "../types";

/** The rebuild command shown after every save (also hard rule 3 in the file's
 * header). Deliberately NOT a button: mgr does not trigger rebuilds. */
const REBUILD_CMD =
  "make gen && docker compose build app && docker compose up -d --force-recreate app";

/** Indent unit inserted by Tab / removed by Shift+Tab. */
const INDENT = "  ";

export function UserToolsPage({ lang }: { lang: Lang }): JSX.Element {
  const [file, setFile] = useState<UserToolsFile | null>(null);
  const [raw, setRaw] = useState("");
  const [error, setError] = useState("");
  const [saved, setSaved] = useState(false);
  const [busy, setBusy] = useState(false);
  const taRef = useRef<HTMLTextAreaElement>(null);

  const adopt = useCallback((f: UserToolsFile) => {
    setFile(f);
    setRaw(f.content);
  }, []);

  const load = useCallback(async () => {
    try {
      adopt(await getUserTools());
      setError("");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [adopt]);

  useEffect(() => {
    void load();
  }, [load]);

  const dirty = file !== null && raw !== file.content;
  const lines = raw === "" ? 0 : raw.split("\n").length;
  const bytes = new Blob([raw]).size;
  const tools = file?.tools ?? [];

  /** Tab inserts two spaces; Shift+Tab removes the two spaces immediately
   * before the cursor (when present). Neither lets the browser move focus. */
  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>): void => {
    const ta = e.currentTarget;
    if (e.key !== "Tab") return;
    e.preventDefault();
    const { selectionStart: s, selectionEnd: epos, value } = ta;
    if (!e.shiftKey) {
      const next = value.slice(0, s) + INDENT + value.slice(epos);
      setRaw(next);
      requestAnimationFrame(() => ta.setSelectionRange(s + INDENT.length, s + INDENT.length));
    } else {
      const before = value.slice(0, s);
      const removed = before.endsWith(INDENT) ? INDENT.length : 0;
      if (removed === 0) return;
      const next = before.slice(0, s - INDENT.length) + value.slice(epos);
      setRaw(next);
      requestAnimationFrame(() => ta.setSelectionRange(s - INDENT.length, epos - INDENT.length));
    }
    setSaved(false);
  };

  const save = async (): Promise<void> => {
    setBusy(true);
    try {
      await putUserTools(raw);
      // Re-read: the backend re-synced the per-tool dirs / meta; the page
      // always renders the file's actual state.
      adopt(await getUserTools());
      setError("");
      setSaved(true);
    } catch (e) {
      // The backend's 400 carries the offending line number (the /root rule
      // or a malformed marker block).
      setError(e instanceof Error ? e.message : String(e));
      setSaved(false);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>{t(lang, "navUserTools")}</h1>
          <p className="sub">{t(lang, "utSub")}</p>
        </div>
        <div className="page-actions">
          <button className="btn btn-secondary" disabled={busy} onClick={() => void load()}>
            {t(lang, "utRefresh")}
          </button>
          <button
            className="btn btn-primary"
            disabled={busy || file === null || !dirty}
            onClick={() => void save()}
          >
            {busy ? t(lang, "utSaving") : t(lang, "utSave")}
          </button>
        </div>
      </div>

      {error && <div className="status error">{error}</div>}
      {saved && !error && (
        <div className="status ok ut-saved">
          {t(lang, "utSaved")} {t(lang, "utRebuild")} <code className="mono">{REBUILD_CMD}</code>
        </div>
      )}

      {/* The three hard rules (also in the file's own header). */}
      <div className="card card-pad ut-note">
        <div className="ut-note-title">{t(lang, "utNoteTitle")}</div>
        <ul>
          <li>{t(lang, "utRule1")}</li>
          <li>{t(lang, "utRule2")}</li>
          <li>
            {t(lang, "utRule3")}{" "}
            <code className="mono">{REBUILD_CMD}</code>
          </li>
        </ul>
        {file && (
          <div className="ut-path">
            {t(lang, "utPathLabel")}: <code className="mono">{file.path}</code>
          </div>
        )}
        {file && (
          <div className="ut-tools">
            {tools.length === 0
              ? t(lang, "utToolsNone")
              : `${t(lang, "utToolsLabel").replace("{n}", String(tools.length))}: ${tools
                  .map((x) => x.name)
                  .join(", ")}`}
          </div>
        )}
      </div>

      {file === null ? (
        <p className="ut-note">{t(lang, "loading")}</p>
      ) : (
        <div className="ut-form">
          <p className="ut-hint">{t(lang, "utRawHint")}</p>
          <textarea
            ref={taRef}
            className="input mono ut-textarea"
            spellCheck={false}
            value={raw}
            aria-label={t(lang, "navUserTools")}
            onKeyDown={onKeyDown}
            onChange={(ev) => {
              setRaw(ev.target.value);
              setSaved(false);
            }}
          />
          <div className="ut-status mono">
            {t(lang, "utStatus")
              .replace("{lines}", String(lines))
              .replace("{bytes}", String(bytes))
              .replace("{state}", dirty ? t(lang, "utStatusDirty") : t(lang, "utStatusSaved"))}
          </div>
        </div>
      )}
    </div>
  );
}
