// EditPage - environment-config editor for one sandbox (design §4 page 3),
// plus the model-profile ASSIGNMENT select (unified Phase 4, D8).
//
// Loads the sandbox (GET /api/sandboxes/:name), shows the same EnvPicker as
// the create wizard seeded with the CURRENT env (so always_on versions and
// enabled scenarios are reflected), plus editable resource inputs. Submit =
// PUT /api/sandboxes/:name -> recreate job (jobs.rs force-recreates with
// volumes kept); the parent switches to the JobView.
//
// The profile select rides the same submit button but is a SEPARATE wire
// call (PUT /api/sandboxes/:name/model_profile, a pure kv write the
// sandbox's 60s pull picks up - never a recreate). A profile-ONLY change
// skips the recreate entirely and returns to the list.
//
// Adopted sandboxes never reach this page (the card hides the edit button;
// the backend would reject them anyway - routes.rs put_sandbox). The
// assignment endpoint itself would accept an adopted row (inert without
// MGR_URL), but there is no path to it from the UI.

import { useEffect, useState } from "react";

import {
  getSandbox,
  listModelProfiles,
  listScenarios,
  putSandbox,
  putSandboxModelProfile,
  type ModelProfile,
} from "../api";
import { t, type Lang } from "../i18n";
import type { SandboxEnv, Scenario, ServicesInput } from "../types";
import { AgentAssignControl } from "../components/AgentAssignControl";
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

  // Model-profile assignment (D8): tri-state on the wire - unchanged sends
  // nothing, an id assigns, the explicit "" (unassigned option) sends null
  // to UNBIND (sandbox keeps its local models.json). Same explicit-null
  // discipline as the limits tri-state above.
  const [profiles, setProfiles] = useState<ModelProfile[] | null>(null);
  const [profileSel, setProfileSel] = useState<string | null>(null); // null = not loaded
  const [origProfile, setOrigProfile] = useState<string | null>(null);
  /** S2 agent subset of the assignment: null = ALL agents (also the legacy
   * payload meaning), [] = zero agents (sandbox keeps local), [...] explicit.
   * Mirrors profileSel's "not loaded" via the double-null dance on save. */
  const [agentsSel, setAgentsSel] = useState<string[] | null>(null);
  const [origAgents, setOrigAgents] = useState<string[] | null>(null);

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
        setOrigProfile(sb.model_profile);
        setProfileSel(sb.model_profile ?? "");
        setOrigAgents(sb.model_agents ?? null);
        setAgentsSel(sb.model_agents ?? null);
        // S1: installed services are read-only here (immutable since
        // create; pre-S1 rows read back all-on server-side).
        void setInstalled(sb.installed_services ?? null);
      })
      .catch((e) => {
        if (!cancelled) setLoadErr(e instanceof Error ? e.message : String(e));
      });
    listModelProfiles()
      .then((r) => {
        if (!cancelled) setProfiles(r.profiles);
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
  const envOrResChanged =
    env !== null &&
    origEnv !== null &&
    (JSON.stringify(sortEnv(env)) !== JSON.stringify(sortEnv(origEnv)) ||
      cpusOut !== origCpus ||
      memOut !== origMem);
  const profileChanged =
    profileSel !== null && profileSel !== (origProfile ?? "");
  /** S2: agents subset changed (deep compare — null "all" vs explicit
   * all-four differ on the wire, but produce the same render; treat them
   * as equal so a no-op save doesn't fire). */
  const agentsChanged = ((): boolean => {
    if (agentsSel === null) return false; // not loaded yet
    if (origAgents === null) return agentsSel.length !== 0;
    return (
      agentsSel.length !== origAgents.length ||
      agentsSel.some((a, i) => a !== origAgents[i])
    );
  })();
  const assignChanged = profileChanged || agentsChanged;
  const changed = envOrResChanged || assignChanged;
  const canSubmit =
    scenarios !== null && env !== null && resErr === "" && changed && !submitting;

  const submit = async () => {
    if (!canSubmit || env === null) return;
    setSubmitting(true);
    setMsg(null);
    try {
      // Profile/agents assignment first (pure kv write): even when the recreate
      // below fails, the assignment stands - it never needed the recreate.
      if (assignChanged) {
        // Wire profile: the current selection, or the ORIGINAL when only
        // agents changed (so an agents-only save keeps the existing
        // assignment instead of un-binding it).
        const wireProfile = profileChanged
          ? profileSel === ""
            ? null
            : profileSel
          : origProfile;
        // Full-replacement semantics: always send the intended subset
        // (origAgents when agents unchanged) — OMITTING it would make the
        // backend's serde default widen a subset back to "all" (ac4 trap).
        // null = all agents (also the legacy meaning).
        const wireAgents = agentsChanged ? agentsSel : origAgents;
        await putSandboxModelProfile(name, wireProfile, wireAgents);
      }
      if (!envOrResChanged) {
        // Profile-only change: nothing to recreate - back to the list (it
        // auto-refreshes and shows the new profile chip within 4s).
        onCancel();
        return;
      }
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
        <div className="status error">
          {t(lang, "loadFailed")}
          {loadErr}
        </div>
      </div>
    );
  }

  return (
    <div className="page">
      <div className="page-head">
        <h1>
          {t(lang, "edTitle")} — <code>{name}</code>
        </h1>
        <div className="page-actions">
          <button className="btn btn-secondary" onClick={onCancel}>
            {t(lang, "cancel")}
          </button>
        </div>
        <p className="sub">{t(lang, "edSub")}</p>
      </div>
      <div className="wizard">
        {scenarios === null || env === null ? (
          <div className="status">{t(lang, "loading")}</div>
        ) : (
          <>
            {installed !== null && (
              <ServicesPicker
                lang={lang}
                services={installed}
                onChange={() => {}}
                readonly
              />
            )}
            <EnvPicker lang={lang} scenarios={scenarios} env={env} onChange={setEnv} />
          </>
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

        {/* Model-profile assignment (D8): a pure kv write on submit - the
         * sandbox's 60s pull picks it up, NO recreate. "" = unassign (the
         * sandbox keeps its local models.json untouched). */}
        <div className="field">
          <label>{t(lang, "mpAssignTo")}</label>
          <span className="hint">{t(lang, "mpAssignHint")}</span>
          <select
            value={profileSel ?? ""}
            disabled={profiles === null}
            onChange={(e) => setProfileSel(e.target.value)}
          >
            <option value="">{t(lang, "mpUnassigned")}</option>
            {(profiles ?? []).map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
          {/* S2 agent subset: only meaningful once a profile is selected
           * (unassigned sandboxes render nothing regardless). */}
          {profileSel !== "" && profileSel !== null && (
            <div className="mp-agents" style={{ marginTop: 8 }}>
              <span className="hint">{t(lang, "mpAgentsHint")}</span>
              <AgentAssignControl
                lang={lang}
                value={agentsSel}
                onChange={setAgentsSel}
              />
              {agentsSel !== null && agentsSel.length === 0 && (
                <span className="field-error">{t(lang, "mpAgentsNone")}</span>
              )}
            </div>
          )}
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
 * deduped scenario ids + key-ordered versions). */
function sortEnv(env: SandboxEnv): SandboxEnv {
  return {
    scenarios: [...env.scenarios].sort(),
    versions: Object.fromEntries(Object.entries(env.versions).sort(([a], [b]) => a.localeCompare(b))),
  };
}
