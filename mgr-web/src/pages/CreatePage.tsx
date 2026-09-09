// CreatePage - sandbox creation wizard (design §4 page 2).
//
// Flow: name (slug, client-validated with the same rules as routes.rs
// validate_name) -> scenario checkboxes + version dropdowns (EnvPicker,
// always_on locked) -> CPU/memory inputs (optional; blank = unlimited) ->
// submit. POST /api/sandboxes returns a job id; the parent switches to the
// JobView for the build progress.
//
// The scenario catalog is passed down from App (fetched once, shared with
// the env editor); on null this page fetches it itself and reports back via
// onScenarios so the editor reuses the copy.

import { useEffect, useState } from "react";

import { createSandbox, listScenarios } from "../api";
import { t, type Lang } from "../i18n";
import type { SandboxEnv, Scenario } from "../types";
import { defaultEnv, EnvPicker } from "./EnvPicker";

const NAME_RE = /^[a-z0-9][a-z0-9-]{0,31}$/;

interface Props {
  lang: Lang;
  scenarios: Scenario[] | null;
  onScenarios: (s: Scenario[]) => void;
  onCancel: () => void;
  onSubmitted: (jobId: number) => void;
}

export function CreatePage({
  lang,
  scenarios,
  onScenarios,
  onCancel,
  onSubmitted,
}: Props): JSX.Element {
  const [name, setName] = useState("");
  const [env, setEnv] = useState<SandboxEnv | null>(null);
  const [cpus, setCpus] = useState("");
  const [memMb, setMemMb] = useState("");
  const [msg, setMsg] = useState<{ kind: "err" | "ok"; text: string } | null>(null);
  const [submitting, setSubmitting] = useState(false);

  // Load the catalog if the parent had none yet; seed the env defaults once.
  useEffect(() => {
    if (scenarios !== null) return;
    let cancelled = false;
    listScenarios()
      .then((r) => {
        if (!cancelled) onScenarios(r.scenarios);
      })
      .catch((e) => {
        if (!cancelled) setMsg({ kind: "err", text: `${t(lang, "loadFailed")}${errText(e)}` });
      });
    return () => {
      cancelled = true;
    };
  }, [scenarios, lang, onScenarios]);

  useEffect(() => {
    if (scenarios !== null && env === null) setEnv(defaultEnv(scenarios));
  }, [scenarios, env]);

  const nameErr = name === "" ? "" : NAME_RE.test(name) ? "" : t(lang, "wzNameErr");
  const cpusVal = cpus.trim() === "" ? null : Number(cpus);
  const memVal = memMb.trim() === "" ? null : Number(memMb);
  const resErr =
    (cpusVal !== null && (!Number.isFinite(cpusVal) || cpusVal <= 0)) ||
    (memVal !== null && (!Number.isFinite(memVal) || !Number.isInteger(memVal) || memVal < 128))
      ? t(lang, "wzResErr")
      : "";
  const canSubmit =
    scenarios !== null &&
    env !== null &&
    NAME_RE.test(name) &&
    resErr === "" &&
    !submitting;

  const submit = async () => {
    if (!canSubmit || env === null) return;
    setSubmitting(true);
    setMsg(null);
    try {
      const r = await createSandbox({
        name,
        env,
        cpus: cpusVal,
        mem_mb: memVal,
      });
      onSubmitted(r.job);
    } catch (e) {
      setSubmitting(false);
      const text = errText(e);
      setMsg({
        kind: "err",
        text: text.includes("already exists") ? t(lang, "wzNameTaken") : text,
      });
    }
  };

  return (
    <div className="page">
      <div className="page-head">
        <h1>{t(lang, "wzTitle")}</h1>
        <div className="page-actions">
          <button className="btn btn-secondary" onClick={onCancel}>
            {t(lang, "cancel")}
          </button>
        </div>
        <p className="sub">{t(lang, "wzSub")}</p>
      </div>
      <div className="wizard">
        <div className="field">
          <label>{t(lang, "wzName")}</label>
          <input
            className="mono"
            value={name}
            placeholder={t(lang, "wzNamePh")}
            aria-invalid={nameErr !== ""}
            autoFocus
            onChange={(e) => setName(e.target.value.trim())}
          />
          <span className="hint">{t(lang, "wzNameHint")}</span>
          {nameErr && <span className="field-error">{nameErr}</span>}
        </div>

        {scenarios === null || env === null ? (
          <div className="status">{t(lang, "loading")}</div>
        ) : (
          <EnvPicker lang={lang} scenarios={scenarios} env={env} onChange={setEnv} />
        )}

        <div className="field">
          <label>{t(lang, "wzResources")}</label>
          <span className="hint">{t(lang, "wzResHint")}</span>
        </div>
        <div className="field-row">
          <div className="field">
            <label>{t(lang, "wzCpus")}</label>
            <input
              value={cpus}
              placeholder={t(lang, "wzCpusPh")}
              inputMode="decimal"
              aria-invalid={resErr !== "" && cpus.trim() !== ""}
              onChange={(e) => setCpus(e.target.value)}
            />
          </div>
          <div className="field">
            <label>{t(lang, "wzMem")}</label>
            <input
              value={memMb}
              placeholder={t(lang, "wzMemPh")}
              inputMode="numeric"
              aria-invalid={resErr !== "" && memMb.trim() !== ""}
              onChange={(e) => setMemMb(e.target.value)}
            />
          </div>
        </div>
        {resErr && <span className="field-error">{resErr}</span>}

        <div className="dialog-actions">
          <button className="btn btn-primary" disabled={!canSubmit} onClick={() => void submit()}>
            {submitting ? t(lang, "wzSubmitting") : t(lang, "wzSubmit")}
          </button>
          {msg && (
            <span className={`wizard-msg ${msg.kind === "err" ? "err" : "ok"}`}>{msg.text}</span>
          )}
        </div>
      </div>
    </div>
  );
}

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
