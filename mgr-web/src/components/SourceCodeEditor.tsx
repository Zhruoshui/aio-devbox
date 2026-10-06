// SourceCodeEditor — the sandbox model-config tab's source view/editor
// (task 10-04-model-config-cc-switch Phase 4, prd R2 "源文件查看/编辑").
//
// Deliberately NOT react-simple-code-editor + prismjs (implement.md 4.4's
// original plan): the sandbox is offline and the npm registry is not
// reachable, so the dependency cannot be installed. Instead this is a
// plain monospace <textarea> with:
//   - a JSON validator for the JSON language (parse error surfaces inline,
//     non-blocking — the draft keeps the last good parse), and
//   - a pass-through mode for TOML (no parser available offline; the text is
//     stored verbatim and the agent itself validates at runtime).
// Theming is all CSS (.sce-* in components.css) so the swap to a real
// editor later is a drop-in.

import { t, type Lang } from "../i18n";

export type SourceLanguage = "json" | "toml";

/** Validate a source string. Returns an error message ("" when valid).
 *  JSON is strictly parsed; TOML only rejects empty input (verbatim pass). */
export function validateSource(text: string, language: SourceLanguage): string {
  if (language === "json") {
    try {
      JSON.parse(text);
      return "";
    } catch (e) {
      return e instanceof Error ? e.message : String(e);
    }
  }
  return text.trim() === "" ? "empty" : "";
}

interface Props {
  /** Current source text. */
  text: string;
  /** json | toml — drives validation + the language hint. */
  language: SourceLanguage;
  /** View mode renders a read-only <pre>-style surface; edit mode is an
   *  editable textarea with live validation + save/cancel actions. */
  editMode: boolean;
  onChange?: (text: string) => void;
  onSave?: () => void;
  onCancel?: () => void;
  /** Disable the save action (e.g. mid-save). */
  saveDisabled?: boolean;
  lang: Lang;
}

export function SourceCodeEditor({
  text,
  language,
  editMode,
  onChange,
  onSave,
  onCancel,
  saveDisabled = false,
  lang,
}: Props): JSX.Element {
  const error = editMode ? validateSource(text, language) : "";

  if (!editMode) {
    return (
      <div className="sce-view">
        <div className="sce-bar">
          <span className="sce-lang">{language === "json" ? "JSON" : "TOML"}</span>
          <span className="muted txs">{t(lang, "sceViewHint")}</span>
        </div>
        <pre className="sce-pre">{text}</pre>
      </div>
    );
  }

  return (
    <div className="sce-edit">
      <div className="sce-bar">
        <span className="sce-lang">{language === "json" ? "JSON" : "TOML"}</span>
        {error !== "" && (
          <span className="sce-err" role="alert">
            {t(lang, "sceParseError")}
            {error === "empty" ? t(lang, "sceEmpty") : error}
          </span>
        )}
      </div>
      <textarea
        className="sce-area"
        value={text}
        spellCheck={false}
        rows={14}
        onChange={(e) => onChange?.(e.target.value)}
      />
      <div className="dialog-actions">
        <button className="btn btn-secondary" onClick={onCancel}>
          {t(lang, "cancel")}
        </button>
        <button
          className="btn btn-primary"
          disabled={error !== "" || saveDisabled}
          onClick={onSave}
        >
          {t(lang, "save")}
        </button>
      </div>
    </div>
  );
}
