// EditPage - environment-config editor for one sandbox (design §4 page 3).
//
// S7 (prototype redesign): .page-head gets the h1+sub.actions shape (sandbox
// name as mono sub), inputs/selects move to the component layer (.input,
// .field.invalid/.err), loading uses .ml-loading instead of the old
// full-height .status.
// Loads the sandbox (GET /api/sandboxes/:name), shows the same EnvPicker as
// the create wizard seeded with the CURRENT env (so always_on versions and
// enabled scenarios are reflected), plus editable resource inputs. Submit =
// PUT /api/sandboxes/:name -> recreate job (jobs.rs force-recreates with
// volumes kept); the parent switches to the JobView.
//
// The cc-switch refactor dropped the model-profile assignment select: each
// sandbox owns its agent configs end-to-end now (prd D1), and the mgr-side
// PUT /api/sandboxes/:name/model_profile route is gone. Agent config lives
// in the WORKSPACE as the client-synthesized modelsConfig pane
// (10-10-models-workspace-pane R2) - this page is environment-only, so the
// former top-level env/models tab bar is gone too (a single tab carried no
// choice).

import { useEffect, useState } from "react";

import {
  getSandbox,
  listScenarios,
  putSandbox,
} from "../api";
import { t, type Lang } from "../i18n";
import type { SandboxEnv, Scenario, ServicesInput } from "../types";
import { EnvPicker } from "./EnvPicker";
import { ServicesPicker } from "./ServicesPicker";

interface Props {
  name: string;
  lang: Lang;
  scenarios: Scenario[] | null;
  onScenarios: (s: Scenario[]) => void;
  onCancel: () => void;
  onSubmitted: (jobId: number) => void;
}

export function EditPage({
  name,
  lang,
  scenarios,
  onScenarios,
  onCancel,
  onSubmitted,
}: Props): JSX.Element {
  const [env, setEnv] = useState<SandboxEnv | null>(null);
  const [origEnv, setOrigEnv] = useState<SandboxEnv | null>(null);
  const [cpus, setCpus] = useState("");
  const [memMb, setMemMb] = useState("");
  const [origCpus, setOrigCpus] = useState<number | null>(null);
  const [origMem, setOrigMem] = useState<number | null>(null);
  const [msg, setMsg] = useState<{ kind: "err" | "ok"; text: string } | null>(null);
  const [loadErr, setLoadErr] = useState("");
  const [submitting, setSubmitting] = useState(false);
  /** S1: installed services read back from the sandbox (read-only display —
   * fixed by the image content at create time). null = payload without the
   * field (defensive against an older backend); the current API always
   * sends it, with pre-S1 rows reading all-on server-side. */
  const [installed, setInstalled] = useState<ServicesInput | null>(null);

  // Load the sandbox + scenario catalog in parallel; seed the form from the
  // stored env (the API's env shape IS the picker's value shape - types.ts).
  useEffect(() => {
    let cancelled = false;
    if (scenarios === null) {
      listScenarios()
        .then((r) => {
          if (!cancelled) onScenarios(r.scenarios);
        })
        .catch((e) => {
          if (!cancelled) setLoadErr(e instanceof Error ? e.message : String(e));
        });
    }
    getSandbox(name)
      .then((sb) => {
        if (cancelled) return;
        setEnv(sb.env);
        setOrigEnv(sb.env);
        setOrigCpus(sb.cpus);
        setOrigMem(sb.mem_mb);
        setCpus(sb.cpus !== null ? String(sb.cpus) : "");
        setMemMb(sb.mem_mb !== null ? String(sb.mem_mb) : "");
        // S1: installed services are read-only here (immutable since
        // create; pre-S1 rows read back all-on server-side).
        void setInstalled(sb.installed_services ?? null);
      })
      .catch((e) => {
        if (!cancelled) setLoadErr(e instanceof Error ? e.message : String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [name, scenarios, onScenarios]);

  // Outgoing resource values under the PUT merge contract (types.ts): a
  // number sets the limit; an emptied field sends 0 to CLEAR it (a plain
  // null would silently keep the old limit); unchanged null stays null.
  const cpusIn = cpus.trim() === "" ? null : Number(cpus);
  const memIn = memMb.trim() === "" ? null : Number(memMb);
  const cpusOut = cpusIn !== null ? cpusIn : origCpus !== null ? 0 : null;
  const memOut = memIn !== null ? memIn : origMem !== null ? 0 : null;
  const resErr =
    (cpusIn !== null && (!Number.isFinite(cpusIn) || cpusIn <= 0)) ||
    (memIn !== null && (!Number.isFinite(memIn) || !Number.isInteger(memIn) || memIn < 128))
      ? t(lang, "wzResErr")
      : "";
  const changed =
    env !== null &&
    origEnv !== null &&
    (JSON.stringify(sortEnv(env)) !== JSON.stringify(sortEnv(origEnv)) ||
      cpusOut !== origCpus ||
      memOut !== origMem);
  const canSubmit =
    scenarios !== null && env !== null && resErr === "" && changed && !submitting;

  const submit = async () => {
    if (!canSubmit || env === null) return;
    setSubmitting(true);
    setMsg(null);
    try {
      const r = await putSandbox(name, {
        env,
        cpus: cpusOut,
        mem_mb: memOut,
      });
      onSubmitted(r.job);
    } catch (e) {
      setSubmitting(false);
      setMsg({ kind: "err", text: e instanceof Error ? e.message : String(e) });
    }
  };

  if (loadErr !== "") {
    return (
      <div className="page">
        <div className="ml-error">
          {t(lang, "loadFailed")}
          {loadErr}
        </div>
      </div>
    );
  }

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>{t(lang, "edTitle")}</h1>
          <p className="sub mono">{name}</p>
        </div>
        <div className="page-actions">
          <button className="btn btn-secondary" onClick={onCancel}>
            {t(lang, "cancel")}
          </button>
        </div>
      </div>

      {/* Environment editor only (PUT /api/sandboxes/:name → recreate job).
       * The former "模型配置" tab moved to the workspace's modelsConfig
       * pane (10-10-models-workspace-pane); with one tab left the bar is
       * gone and the wizard renders directly. */}
      <div className="wizard">
        {scenarios === null || env === null ? (
          <div className="ml-loading">{t(lang, "loading")}</div>
        ) : (
          <>
            {installed !== null && (
              <>
                <div className="field">
                  <label>{t(lang, "wzServices")}</label>
                  <span className="hint">{t(lang, "edSvcFixed")}</span>
                </div>
                <ServicesPicker
                  lang={lang}
                  services={installed}
                  onChange={() => {}}
                  readonly
                />
              </>
            )}
            <div className="field">
              <label>{t(lang, "wzScenarios")}</label>
              <span className="hint">{t(lang, "wzScenariosHint")}</span>
            </div>
            <EnvPicker lang={lang} scenarios={scenarios} env={env} onChange={setEnv} />
          </>
        )}

        <div className="field">
          <label>{t(lang, "wzResources")}</label>
          <span className="hint">{t(lang, "wzResHint")}</span>
        </div>
        <div className="field-row" style={{ maxWidth: 480 }}>
          <div className={`field${resErr !== "" && cpus.trim() !== "" ? " invalid" : ""}`}>
            <label>{t(lang, "wzCpus")}</label>
            <input
              className="input mono"
              value={cpus}
              placeholder={t(lang, "wzCpusPh")}
              inputMode="decimal"
              aria-invalid={resErr !== "" && cpus.trim() !== ""}
              onChange={(e) => setCpus(e.target.value)}
            />
            <span className="err">{t(lang, "wzResErr")}</span>
          </div>
          <div className={`field${resErr !== "" && memMb.trim() !== "" ? " invalid" : ""}`}>
            <label>{t(lang, "wzMem")}</label>
            <input
              className="input mono"
              value={memMb}
              placeholder={t(lang, "wzMemPh")}
              inputMode="numeric"
              aria-invalid={resErr !== "" && memMb.trim() !== ""}
              onChange={(e) => setMemMb(e.target.value)}
            />
            <span className="err">{t(lang, "wzResErr")}</span>
          </div>
        </div>

        <div className="dialog-actions">
          <button className="btn btn-primary" disabled={!canSubmit} onClick={() => void submit()}>
            {submitting ? t(lang, "wzSubmitting") : t(lang, "edSave")}
          </button>
          {changed ? null : <span className="wizard-msg">{t(lang, "edSameEnv")}</span>}
          {msg && (
            <span className={`wizard-msg ${msg.kind === "err" ? "err" : "ok"}`}>{msg.text}</span>
          )}
        </div>
      </div>
    </div>
  );
}

/** Canonical env comparison (envhash.rs canonical_json semantics: sorted,
 * deduped scenario ids + key-ordered versions + sorted user_tools, 修正 3). */
function sortEnv(env: SandboxEnv): SandboxEnv {
  return {
    scenarios: [...env.scenarios].sort(),
    versions: Object.fromEntries(Object.entries(env.versions).sort(([a], [b]) => a.localeCompare(b))),
    user_tools: [...(env.user_tools ?? [])].sort(),
  };
}
