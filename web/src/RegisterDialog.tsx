// RegisterDialog - modal form for user-registered buttons (POST /api/buttons
// via the onRegister callback from App).
//
// Two button types (issue #1): "agent" (terminal command, the original form)
// and "web" (dev server port preview - swaps the cmd field for a port input).
// The type toggle is a segmented control; switching keeps the label but only
// the active type's field is validated/submitted.
//
// Kumo dialog guidance (docs/open-design/cloudflare_kumo_ui.md): keep the
// dialog mounted and drive visibility through open state so entry/exit motion
// completes; visible field labels; per-field errors associated with their
// input (aria-invalid + role="alert"); secondary cancel + single primary
// submit. Focus moves to the first field on open and is restored to the
// opener when closed; Escape and backdrop clicks close.

import { useEffect, useRef, useState } from "react";

import { fmt, t, type Lang } from "./i18n";
import { Icon } from "./icons";
import type { RegisterButtonInput, RegisterButtonType } from "./types";

interface Props {
  open: boolean;
  lang: Lang;
  onClose: () => void;
  onRegister: (input: RegisterButtonInput) => Promise<boolean>;
}

export function RegisterDialog({ open, lang, onClose, onRegister }: Props): JSX.Element {
  const [type, setType] = useState<RegisterButtonType>("agent");
  const [label, setLabel] = useState("");
  const [cmd, setCmd] = useState("");
  const [port, setPort] = useState("");
  const [labelErr, setLabelErr] = useState(false);
  const [cmdErr, setCmdErr] = useState(false);
  const [portErr, setPortErr] = useState(false);
  const [failed, setFailed] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  // Probe hint (web type only): is anything listening on the typed port?
  // "unknown" = not probed yet (initial / invalid format / request in flight).
  // Non-blocking UX: a dead port shows a warning but registration stays
  // allowed (09-02-web-button-ux-fix R2).
  const [probe, setProbe] = useState<"unknown" | "listening" | "dead">("unknown");
  const [probedPort, setProbedPort] = useState<number | null>(null);
  // Opener element is captured on open so focus can be restored on close.
  const openerRef = useRef<Element | null>(null);
  const firstFieldRef = useRef<HTMLInputElement>(null);

  // Open transition: reset fields, remember the opener, focus the first
  // field. The overlay fades in over visibility+opacity, and focus() on a
  // still-hidden element is a no-op - so flush the style change first (the
  // visibility transition then computes visible immediately) before focusing.
  useEffect(() => {
    if (!open) return;
    setType("agent");
    setLabel("");
    setCmd("");
    setPort("");
    setLabelErr(false);
    setCmdErr(false);
    setPortErr(false);
    setFailed(false);
    setProbe("unknown");
    setProbedPort(null);
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
  }, [open, label, cmd, port]);

  // Debounced liveness probe (web type): 500ms after the port settles into a
  // format-valid value, GET /api/buttons/probe and surface listening/dead.
  // Only the latest request's result is adopted (stale responses are dropped
  // via the cleanup flag), and the result carries the port it probed so a
  // fast subsequent edit never renders a mismatched hint.
  useEffect(() => {
    if (type !== "web" || !open) return;
    const p = Number(port.trim());
    if (!Number.isInteger(p) || p < 1 || p > 65535 || p === 8088) {
      setProbe("unknown");
      setProbedPort(null);
      return;
    }
    setProbe("unknown");
    let stale = false;
    const timer = window.setTimeout(async () => {
      try {
        const r = await fetch(`/api/buttons/probe?port=${p}`);
        if (!r.ok || stale) return;
        const body: { listening: boolean } = await r.json();
        if (stale) return;
        setProbe(body.listening ? "listening" : "dead");
        setProbedPort(p);
      } catch {
        // Network/gateway hiccup: stay "unknown", no hint rendered.
      }
    }, 500);
    return () => {
      stale = true;
      window.clearTimeout(timer);
    };
  }, [port, type, open]);

  function close(): void {
    onClose();
    const el = openerRef.current;
    if (el instanceof HTMLElement) el.focus();
  }

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (submitting) return;
    const l = label.trim();
    const okL = !!l;
    setLabelErr(!okL);

    let okField = true;
    if (type === "agent") {
      const c = cmd.trim();
      okField = !!c;
      setCmdErr(!okField);
      setPortErr(false);
    } else {
      const p = Number(port.trim());
      // Mirror the server-side rules: 1-65535, and 8088 is the workbench's
      // own port (proxying it would recurse).
      okField = Number.isInteger(p) && p >= 1 && p <= 65535 && p !== 8088;
      setPortErr(!okField);
      setCmdErr(false);
    }
    if (!okL) {
      firstFieldRef.current?.focus();
      return;
    }
    if (!okField) return;

    setSubmitting(true);
    const input: RegisterButtonInput =
      type === "agent"
        ? { label: l, type: "agent", cmd: cmd.trim() }
        : { label: l, type: "web", port: Number(port.trim()) };
    const ok = await onRegister(input);
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
            <div className="seg" role="radiogroup" aria-label={t(lang, "dialogTitle")}>
              {(["agent", "web"] as const).map((tp) => (
                <button
                  key={tp}
                  type="button"
                  role="radio"
                  aria-checked={type === tp}
                  className={`seg-btn${type === tp ? " active" : ""}`}
                  onClick={() => setType(tp)}
                >
                  {t(lang, tp === "agent" ? "typeAgent" : "typeWeb")}
                </button>
              ))}
            </div>
          </div>
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
          {type === "agent" ? (
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
            </div>
          ) : (
            <div className="field">
              <label htmlFor="f-port">{t(lang, "fieldPort")}</label>
              <input
                id="f-port"
                name="port"
                inputMode="numeric"
                pattern="[0-9]*"
                placeholder={t(lang, "fieldPortPh")}
                autoComplete="off"
                value={port}
                aria-invalid={portErr}
                onChange={(e) => {
                  setPort(e.target.value);
                  if (portErr) setPortErr(false);
                }}
              />
              <span className="hint">{t(lang, "fieldPortHint")}</span>
              {probe !== "unknown" && !portErr && probedPort === Number(port.trim()) && (
                <span
                  className={`probe-hint${probe === "dead" ? " warn" : " ok"}`}
                  role="status"
                >
                  {probe === "dead"
                    ? fmt(lang, "probeDead", probedPort)
                    : fmt(lang, "probeListening", probedPort)}
                </span>
              )}
              <span className={`field-error${portErr ? " show" : ""}`} role="alert">
                {t(lang, "errPort")}
              </span>
            </div>
          )}
          <div className="dialog-actions">
            <button type="button" className="btn btn-secondary" onClick={close}>
              {t(lang, "cancel")}
            </button>
            <button type="submit" className="btn btn-primary" disabled={submitting}>
              {submitting ? <Icon name="refresh" /> : <Icon name="plus" />}
              {t(lang, "submit")}
            </button>
          </div>
          {failed && (
            <span className="field-error show" role="alert">
              {t(lang, "errFailed")}
            </span>
          )}
        </form>
      </div>
    </div>
  );
}
