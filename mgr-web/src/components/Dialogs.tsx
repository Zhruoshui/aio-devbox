// Dialogs - shared in-app modal dialogs (R1): ConfirmDialog replaces the
// native confirm() and PromptDialog the native prompt(). Both follow the
// project's overlay contract (SandboxListPage's delete confirm + the focus
// management of pages/workspace/RegisterDialog.tsx): conditionally rendered
// `.overlay.open` > `.dialog`, Escape closes, scrim click closes, focus
// moves to the first control on open and returns to the opener on close.
// Destructive confirms render role="alertdialog", everything else
// role="dialog". Styling comes solely from components.css - no new CSS.

import { useEffect, useRef, useState } from "react";
import { t, type Lang } from "../i18n";

/** Shared dialog chrome: capture the opener on mount, focus the first
 * control after the dialog paints (focus() on a freshly-mounted tree needs
 * a frame), close on Escape, and restore focus to the opener when the
 * dialog unmounts (dialogs are conditionally rendered, so unmount IS the
 * close path). `selectOnFocus` additionally selects the input's contents
 * (PromptDialog: the default value is a suggestion, ready to overwrite). */
function useDialogChrome(
  onClose: () => void,
  firstRef: React.RefObject<HTMLElement | null>,
  selectOnFocus = false,
): void {
  // Keep the latest callback without re-running the mount-only effect (an
  // inline arrow from the caller changes identity every render; re-capturing
  // mid-dialog would steal focus management).
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  useEffect(() => {
    const opener = document.activeElement;
    let raf2 = 0;
    const raf1 = requestAnimationFrame(() => {
      raf2 = requestAnimationFrame(() => {
        firstRef.current?.focus();
        if (selectOnFocus) (firstRef.current as HTMLInputElement | null)?.select();
      });
    });
    const fb = window.setTimeout(() => {
      firstRef.current?.focus();
      if (selectOnFocus) (firstRef.current as HTMLInputElement | null)?.select();
    }, 120);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeRef.current();
    };
    document.addEventListener("keydown", onKey);
    return () => {
      cancelAnimationFrame(raf1);
      cancelAnimationFrame(raf2);
      window.clearTimeout(fb);
      document.removeEventListener("keydown", onKey);
      if (opener instanceof HTMLElement) opener.focus();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
}

interface ConfirmDialogProps {
  lang: Lang;
  title: string;
  /** Explanatory body (the native confirm()'s message). */
  desc?: string;
  /** Destructive confirm: alertdialog role + btn-danger confirm button. */
  danger?: boolean;
  confirmLabel?: string;
  onConfirm: () => void;
  onCancel: () => void;
}

export function ConfirmDialog({
  lang,
  title,
  desc,
  danger = false,
  confirmLabel,
  onConfirm,
  onCancel,
}: ConfirmDialogProps): JSX.Element {
  const confirmRef = useRef<HTMLButtonElement>(null);
  useDialogChrome(onCancel, confirmRef);
  return (
    <div className="overlay open" role="presentation" onClick={onCancel}>
      <div
        className="dialog"
        role={danger ? "alertdialog" : "dialog"}
        aria-modal="true"
        aria-labelledby="dlg-confirm-title"
        aria-describedby={desc ? "dlg-confirm-desc" : undefined}
        onClick={(e) => e.stopPropagation()}
      >
        <h2 id="dlg-confirm-title">{title}</h2>
        {desc && (
          <p className="desc" id="dlg-confirm-desc">
            {desc}
          </p>
        )}
        <div className="dialog-actions">
          <button className="btn btn-secondary" onClick={onCancel}>
            {t(lang, "cancel")}
          </button>
          <button
            ref={confirmRef}
            className={`btn ${danger ? "btn-danger" : "btn-primary"}`}
            onClick={onConfirm}
          >
            {confirmLabel ?? t(lang, "dialogConfirm")}
          </button>
        </div>
      </div>
    </div>
  );
}

interface PromptDialogProps {
  lang: Lang;
  title: string;
  /** Explanatory body above the input (e.g. "新 profile 名称:"). */
  desc?: string;
  defaultValue?: string;
  placeholder?: string;
  confirmLabel?: string;
  /** Receives the typed value (untrimmed - callers own validation); the
   * dialog always closes on submit, mirroring the native prompt. */
  onSubmit: (value: string) => void;
  onCancel: () => void;
}

export function PromptDialog({
  lang,
  title,
  desc,
  defaultValue = "",
  placeholder,
  confirmLabel,
  onSubmit,
  onCancel,
}: PromptDialogProps): JSX.Element {
  const [value, setValue] = useState(defaultValue);
  const inputRef = useRef<HTMLInputElement>(null);
  useDialogChrome(onCancel, inputRef, true);
  return (
    <div className="overlay open" role="presentation" onClick={onCancel}>
      <div
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="dlg-prompt-title"
        aria-describedby={desc ? "dlg-prompt-desc" : undefined}
        onClick={(e) => e.stopPropagation()}
      >
        <h2 id="dlg-prompt-title">{title}</h2>
        {desc && (
          <p className="desc" id="dlg-prompt-desc">
            {desc}
          </p>
        )}
        {/* A form makes Enter submit natively; Esc is handled by
         * useDialogChrome's document keydown. */}
        <form
          onSubmit={(e) => {
            e.preventDefault();
            onSubmit(value);
          }}
          noValidate
        >
          <div className="field">
            <input
              ref={inputRef}
              className="input"
              value={value}
              placeholder={placeholder}
              autoComplete="off"
              aria-label={title}
              onChange={(e) => setValue(e.target.value)}
            />
          </div>
          <div className="dialog-actions">
            <button type="button" className="btn btn-secondary" onClick={onCancel}>
              {t(lang, "cancel")}
            </button>
            <button type="submit" className="btn btn-primary">
              {confirmLabel ?? t(lang, "dialogConfirm")}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
