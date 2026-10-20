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

/** Detect duplicate keys in (comment-stripped) JSON text. JSON.parse
 *  silently keeps the LAST occurrence — a hand-edit that re-states a key
 *  would quietly drop the earlier value, so surface it as a hard error.
 *  A stack of seen-key sets: entering `{` pushes, leaving pops; a JSON
 *  string followed by `:` inside an object is a key. Returns the offender
 *  ("\"key\"") or null. */
export function findDuplicateJsonKeys(text: string): string | null {
  const seen: Set<string>[] = [new Set()];
  let i = 0;
  let inString = false;
  let inEscape = false;
  while (i < text.length) {
    const c = text[i];
    if (inString) {
      if (inEscape) inEscape = false;
      else if (c === "\\") inEscape = true;
      else if (c === '"') inString = false;
      i++;
      continue;
    }
    if (c === '"') {
      // Read the whole string, then peek past whitespace for ':' (a key).
      let j = i + 1;
      let s = "";
      while (j < text.length && text[j] !== '"') {
        if (text[j] === "\\") {
          s += text[j] + (text[j + 1] ?? "");
          j += 2;
          continue;
        }
        s += text[j];
        j++;
      }
      let k = j + 1;
      while (k < text.length && /\s/.test(text[k])) k++;
      if (text[k] === ":") {
        const top = seen[seen.length - 1];
        if (top.has(s)) return `"${s}"`;
        top.add(s);
      }
      i = j + 1;
      continue;
    }
    if (c === "{" || c === "[") seen.push(new Set());
    else if (c === "}" || c === "]") seen.pop();
    i++;
  }
  return null;
}

/** Lightweight TOML syntax check (offline — no parser dependency). Not a
 *  full grammar: it catches the common hand-edit slips — a line that is
 *  neither blank/comment/section/key=value, an unterminated same-line
 *  string, and unclosed brackets/braces across lines (multi-line arrays).
 *  String CONTENT is skipped (brackets/keys inside quotes never count);
 *  triple-quoted multi-line strings span lines until their delimiter.
 *  Returns "" when OK, else an error with the 1-based line number. */
export function validateToml(text: string): string {
  const lines = text.split("\n");
  // Bracket depth carried across lines (multi-line arrays, inline tables).
  let depth = 0;
  // Set while inside a multi-line string opened but not yet closed.
  let mlString: string | null = null; // '"""' | "'''"
  for (let n = 0; n < lines.length; n++) {
    const raw = lines[n];
    const lineNo = n + 1;
    if (mlString !== null) {
      if (raw.includes(mlString)) mlString = null;
      continue;
    }
    // Walk the line: track strings, cut comments, keep a string-stripped
    // projection for the shape checks, count brackets outside strings.
    const startDepth = depth;
    let out = "";
    let inStr: string | null = null; // '"' | "'" | '"""' | "'''"
    for (let i = 0; i < raw.length; i++) {
      const c = raw[i];
      const three = raw.slice(i, i + 3);
      if (inStr === null && (three === '"""' || three === "'''")) {
        inStr = three;
        i += 2;
        continue;
      }
      if (inStr === null && (c === '"' || c === "'")) {
        inStr = c;
        continue;
      }
      if (inStr !== null) {
        if (inStr.length === 3 && raw.slice(i, i + 3) === inStr) {
          // multi-line string closed on the same line
          inStr = null;
          i += 2;
          continue;
        }
        if (c === "\\" && inStr === '"') {
          i++; // skip the escaped char
          continue;
        }
        if (c === inStr) {
          inStr = null;
          continue;
        }
        continue; // string content: never reaches `out` or the depth count
      }
      if (c === "#") break; // comment: drop the rest of the line
      out += c;
      if (c === "[" || c === "{") depth++;
      else if (c === "}") depth--;
      else if (c === "]") {
        depth--;
        if (depth < 0) return `line ${lineNo}: unbalanced "]"`;
      }
    }
    if (inStr !== null) {
      if (inStr.length === 1) return `line ${lineNo}: unterminated string`;
      mlString = inStr; // opened here, closes on a later line
      continue;
    }
    const line = out.trim();
    if (line === "") continue;
    // Shape check only for lines that live entirely at depth 0 — a line
    // INSIDE a multi-line array (a value row or the closing bracket) has
    // its own TOML grammar we don't model.
    if (startDepth !== 0 || depth !== 0) continue;
    const isHeader = /^\[\[?[^[\]]*\]\]?$/.test(line);
    const hasKeyValue = /^[A-Za-z0-9_\-."'][^=]*=/.test(line);
    if (!isHeader && !hasKeyValue) {
      return `line ${lineNo}: expected "key = value" or [section], got: ${line.slice(0, 40)}`;
    }
  }
  if (depth !== 0) return `unclosed bracket (depth ${depth} at end of file)`;
  if (mlString !== null) return `unterminated multi-line string (${mlString})`;
  return "";
}

/** Validate a source string. Returns an error message ("" when valid).
 *  JSON is strictly parsed and duplicate keys rejected; JSONC is
 *  comment-stripped first; TOML gets a lightweight syntax check (10-13
 *  follow-up: 语法层面检查). */
export function validateSource(text: string, language: SourceLanguage): string {
  if (language === "json" || language === "jsonc") {
    const stripped = language === "jsonc" ? stripJsoncComments(text) : text;
    const dup = findDuplicateJsonKeys(stripped);
    if (dup !== null) return `duplicate key ${dup} (JSON keeps only the last one)`;
    try {
      JSON.parse(stripped);
      return "";
    } catch (e) {
      return e instanceof Error ? e.message : String(e);
    }
  }
  if (text.trim() === "") return "empty";
  return validateToml(text);
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
