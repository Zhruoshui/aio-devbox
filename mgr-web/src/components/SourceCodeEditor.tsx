// SourceCodeEditor — the sandbox model-config tab's source view/editor
// (task 10-04-model-config-cc-switch Phase 4, prd R2 "源文件查看/编辑").
//
// Deliberately NOT react-simple-code-editor + prismjs (implement.md 4.4's
// original plan): the sandbox is offline and the npm registry is not
// reachable, so the dependency cannot be installed. Instead this is a
// plain monospace <textarea> with:
//   - a JSON validator for the JSON language (parse error surfaces inline,
//     non-blocking — the draft keeps the last good parse),
//   - a JSONC validator (10-13-models-config-ux R6: comments stripped
//     before the JSON parse — opencode's opencode.jsonc allows them), and
//   - a pass-through mode for TOML (no parser available offline; the text is
//     stored verbatim and the agent itself validates at runtime).
// Theming is all CSS (.sce-* in components.css) so the swap to a real
// editor later is a drop-in.

import { t, type Lang } from "../i18n";

export type SourceLanguage = "json" | "jsonc" | "toml";

/** Strip line (slash-slash) and block (slash-star) comments from JSONC
 *  text (cc-switch jsonc_document semantics). A small state machine so a
 *  double-slash INSIDE a string literal (e.g. an "https://…" url value)
 *  survives; escapes inside strings are tracked so a quoted backslash or
 *  quote doesn't flip the string state. */
export function stripJsoncComments(text: string): string {
  let out = "";
  let i = 0;
  // normal | string | escape | lineComment | blockComment
  let state: "normal" | "string" | "escape" | "lineComment" | "blockComment" =
    "normal";
  while (i < text.length) {
    const c = text[i];
    const next = text[i + 1];
    switch (state) {
      case "normal":
        if (c === "/" && next === "/") {
          state = "lineComment";
          i += 2;
        } else if (c === "/" && next === "*") {
          state = "blockComment";
          i += 2;
        } else if (c === '"') {
          state = "string";
          out += c;
          i++;
        } else {
          out += c;
          i++;
        }
        break;
      case "string":
        if (c === "\\") {
          state = "escape";
          out += c;
          i++;
        } else if (c === '"') {
          state = "normal";
          out += c;
          i++;
        } else {
          out += c;
          i++;
        }
        break;
      case "escape":
        // Any escaped char (incl. \" and \\) stays literal.
        state = "string";
        out += c;
        i++;
        break;
      case "lineComment":
        if (c === "\n") {
          state = "normal";
          out += c; // keep the newline (line/col in errors stay honest)
          i++;
        } else {
          i++;
        }
        break;
      case "blockComment":
        if (c === "*" && next === "/") {
          state = "normal";
          i += 2;
        } else {
          if (c === "\n") out += c; // keep newlines for error positions
          i++;
        }
        break;
    }
  }
  return out;
}

/** Validate a source string. Returns an error message ("" when valid).
 *  JSON is strictly parsed; JSONC is comment-stripped then parsed; TOML
 *  only rejects empty input (verbatim pass). */
export function validateSource(text: string, language: SourceLanguage): string {
  if (language === "json" || language === "jsonc") {
    try {
      JSON.parse(language === "jsonc" ? stripJsoncComments(text) : text);
      return "";
    } catch (e) {
      return e instanceof Error ? e.message : String(e);
    }
  }
  return text.trim() === "" ? "empty" : "";
}

/** The language chip label. */
function langLabel(language: SourceLanguage): string {
  return language === "json" ? "JSON" : language === "jsonc" ? "JSONC" : "TOML";
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
          <span className="sce-lang">{langLabel(language)}</span>
          <span className="muted txs">{t(lang, "sceViewHint")}</span>
        </div>
        <pre className="sce-pre">{text}</pre>
      </div>
    );
  }

  return (
    <div className="sce-edit">
      <div className="sce-bar">
        <span className="sce-lang">{langLabel(language)}</span>
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
