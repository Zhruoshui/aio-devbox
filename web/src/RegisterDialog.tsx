// RegisterDialog - modal form for user-registered buttons (POST /api/buttons
// via the onRegister callback from App).
//
// Kumo dialog guidance (docs/open-design/cloudflare_kumo_ui.md): keep the
// dialog mounted and drive visibility through open state so entry/exit motion
// completes; visible field labels; per-field errors associated with their
// input (aria-invalid + role="alert"); secondary cancel + single primary
// submit. Focus moves to the first field on open and is restored to the
// opener when closed; Escape and backdrop clicks close.

import { useEffect, useRef, useState } from "react";

import { t, type Lang } from "./i18n";
import { Icon } from "./icons";

interface Props {
  open: boolean;
  lang: Lang;
  onClose: () => void;
  onRegister: (label: string, cmd: string) => Promise<boolean>;
}

export function RegisterDialog({ open, lang, onClose, onRegister }: Props): JSX.Element {
  const [label, setLabel] = useState("");
  const [cmd, setCmd] = useState("");
  const [labelErr, setLabelErr] = useState(false);
  const [cmdErr, setCmdErr] = useState(false);
  const [failed, setFailed] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  // Opener element is captured on open so focus can be restored on close.
  const openerRef = useRef<Element | null>(null);
  const firstFieldRef = useRef<HTMLInputElement>(null);

  // Open transition: reset fields, remember the opener, focus the first
  // field. The overlay fades in over visibility+opacity, and focus() on a
  // still-hidden element is a no-op - so flush the style change first (the
  // visibility transition then computes visible immediately) before focusing.
  useEffect(() => {
    if (!open) return;
    setLabel("");
    setCmd("");
    setLabelErr(false);
    setCmdErr(false);
    setFailed(false);
    openerRef.current = document.activeElement;
    // The overlay fades in over visibility+opacity, and focus() on a
    // still-hidden element is a no-op. Wait two frames so the visibility
    // transition is underway (computed visibility flips to visible as soon as
    // it starts), with a timeout fallback for throttled rAF.
    let raf2 = 0;
    const raf1 = requestAnimationFrame(() => {
      raf2 = requestAnimationFrame(() => firstFieldRef.current?.focus());
    });
    const fb = window.setTimeout(() => firstFieldRef.current?.focus(), 120);
    return () => {
      cancelAnimationFrame(raf1);
      cancelAnimationFrame(raf2);
      window.clearTimeout(fb);
    };
  }, [open]);

  // Escape closes; also guards against stuck focus when closed.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, label, cmd]);

  function close(): void {
    onClose();
    const el = openerRef.current;
    if (el instanceof HTMLElement) el.focus();
  }

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (submitting) return;
    const l = label.trim();
    const c = cmd.trim();
    const okL = !!l;
    const okC = !!c;
    setLabelErr(!okL);
    setCmdErr(!okC);
    if (!okL) {
      firstFieldRef.current?.focus();
      return;
    }
    if (!okC) return;
    setSubmitting(true);
    const ok = await onRegister(l, c);
    setSubmitting(false);
    if (ok) {
      close();
    } else {
      setFailed(true);
    }
  };

  return (
    <div
      className={`overlay${open ? " open" : ""}`}
      aria-hidden={!open}
      onPointerDown={(e) => {
        if (e.target === e.currentTarget) close();
      }}
    >
      <div
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="dlg-title"
        data-od-id="register-dialog"
      >
        <h2 id="dlg-title">{t(lang, "dialogTitle")}</h2>
        <p className="sub">{t(lang, "dialogSub")}</p>
        <form onSubmit={submit} noValidate>
          <div className="field">
            <label htmlFor="f-label">{t(lang, "fieldLabel")}</label>
            <input
              id="f-label"
              ref={firstFieldRef}
              name="label"
              maxLength={64}
              placeholder={t(lang, "fieldLabelPh")}
              autoComplete="off"
              value={label}
              aria-invalid={labelErr}
              onChange={(e) => {
                setLabel(e.target.value);
                if (labelErr) setLabelErr(false);
              }}
            />
            <span className={`field-error${labelErr ? " show" : ""}`} role="alert">
              {t(lang, "errLabel")}
            </span>
          </div>
          <div className="field">
            <label htmlFor="f-cmd">{t(lang, "fieldCmd")}</label>
            <input
              id="f-cmd"
              name="cmd"
              maxLength={64}
              placeholder={t(lang, "fieldCmdPh")}
              autoComplete="off"
              value={cmd}
              aria-invalid={cmdErr}
              onChange={(e) => {
                setCmd(e.target.value);
                if (cmdErr) setCmdErr(false);
              }}
            />
            <span className="hint">{t(lang, "fieldCmdHint")}</span>
            <span className={`field-error${cmdErr ? " show" : ""}`} role="alert">
              {t(lang, "errCmd")}
            </span>
            {failed && (
              <span className="field-error show" role="alert">
                {t(lang, "errFailed")}
              </span>
            )}
          </div>
          <div className="dialog-actions">
            <button type="button" className="btn btn-secondary" onClick={close}>
              {t(lang, "cancel")}
            </button>
            <button type="submit" className="btn btn-primary" disabled={submitting}>
              {submitting ? <Icon name="refresh" /> : <Icon name="plus" />}
              {t(lang, "submit")}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
