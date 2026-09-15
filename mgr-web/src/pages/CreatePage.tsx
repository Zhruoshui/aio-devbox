// CreatePage - sandbox creation wizard (design §4 page 2, redesigned per the
// 09-11 prototype create-sandbox.html).
//
// Layout: four numbered `.sec` sections (name -> services -> scenarios ->
// resources) with a sticky right-hand `.side` (step nav `.steps` + summary
// card `.sum`). The summary renders live: entry subdomain, service chips,
// scenarios & versions, resources, and the image-reuse hint; the step nav
// highlights via IntersectionObserver over the wizard's scroll container and
// clicks scroll smoothly to the section.
//
// Image-reuse hint (no new backend): the env_hash is sha256 of the assembled
// Dockerfile.base (envhash.rs) - not computable client-side - so the hint
// matches the wizard's env against (1) existing sandbox rows (exact env
// field; their `image` tag is what a same-env create reuses) and (2) image
// rows' `combo` descriptions (scenarios+versions parse; the app tag derives
// from the recorded base tag). Both key on scenarios+versions only, which is
// exactly the surface env_hash covers - the cs/vnc switches gate builds, not
// tags (jobs.rs).
//
// Flow unchanged: name (slug, client-validated like routes.rs
// validate_name) -> services (ServicesPicker, dependency linkage) ->
// scenarios + versions (EnvPicker, always_on locked) -> optional CPU/memory
// (blank = unlimited). POST /api/sandboxes returns a job id; the parent
// switches to the JobView for the build progress.
//
// The scenario catalog is passed down from App (fetched once, shared with
// the env editor); on null this page fetches it itself and reports back via
// onScenarios so the editor reuses the copy.

import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";

import { createSandbox, listImages, listSandboxes, listScenarios } from "../api";
import { fmt, t, type Lang } from "../i18n";
import type { Image, Sandbox, SandboxEnv, Scenario } from "../types";
import { defaultEnv, EnvPicker } from "./EnvPicker";
import { defaultServices, ServicesPicker } from "./ServicesPicker";

/** Sandbox-name slug contract, mirroring routes.rs validate_name. Exported
 * for AdoptPage (the adopt wizard registers a row under the same rules). */
export const NAME_RE = /^[a-z0-9][a-z0-9-]{0,31}$/;

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
  const [services, setServices] = useState(defaultServices());
  const [cpus, setCpus] = useState("");
  const [memMb, setMemMb] = useState("");
  const [msg, setMsg] = useState<{ kind: "err" | "ok"; text: string } | null>(null);
  const [submitting, setSubmitting] = useState(false);
  // Reuse-hint sources: existing sandboxes + recorded images, fetched once.
  // Failure is non-fatal - the hint just falls back to "build needed".
  const [knownSbx, setKnownSbx] = useState<Sandbox[] | null>(null);
  const [knownImg, setKnownImg] = useState<Image[] | null>(null);
  // Step nav: which .sec is in view (IntersectionObserver).
  const [activeSec, setActiveSec] = useState("s-name");
  const formRef = useRef<HTMLFormElement>(null);

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

  useEffect(() => {
    let cancelled = false;
    listSandboxes()
      .then((r) => {
        if (!cancelled) setKnownSbx(r.sandboxes);
      })
      .catch(() => {
        if (!cancelled) setKnownSbx([]);
      });
    listImages()
      .then((r) => {
        if (!cancelled) setKnownImg(r.images);
      })
      .catch(() => {
        if (!cancelled) setKnownImg([]);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Step-nav highlighting: observe the .sec sections inside the wizard's
  // scroll container (the shell's .main; prototype uses rootMargin so the
  // "active" band sits in the upper third of the viewport).
  const sectionsReady = scenarios !== null && env !== null;
  useEffect(() => {
    const form = formRef.current;
    if (form === null || !sectionsReady) return;
    const root = form.closest(".main");
    const io = new IntersectionObserver(
      (entries) => {
        for (const en of entries) {
          if (en.isIntersecting) setActiveSec(en.target.id);
        }
      },
      { root, rootMargin: "-20% 0px -60% 0px" },
    );
    form.querySelectorAll(".sec").forEach((s) => io.observe(s));
    return () => io.disconnect();
  }, [sectionsReady]);

  const goto = (id: string) => {
    const form = formRef.current;
    const sec = document.getElementById(id);
    if (form === null || sec === null) return;
    const root = form.closest(".main");
    if (root === null) {
      sec.scrollIntoView({ behavior: "smooth", block: "start" });
      return;
    }
    const r = sec.getBoundingClientRect();
    const rr = root.getBoundingClientRect();
    root.scrollTo({ top: root.scrollTop + r.top - rr.top - 16, behavior: "smooth" });
  };

  // Step anchors are href-less <a> (the prototype's hash navigation becomes
  // a container scrollTo); keep them focusable and Enter/Space-operable.
  const stepProps = (id: string) => ({
    onClick: () => goto(id),
    tabIndex: 0,
    onKeyDown: (e: KeyboardEvent) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        goto(id);
      }
    },
  });

  const nameOk = NAME_RE.test(name);
  const nameErr = name === "" || nameOk ? "" : t(lang, "wzNameErr");
  const cpusVal = cpus.trim() === "" ? null : Number(cpus);
  const memVal = memMb.trim() === "" ? null : Number(memMb);
  const cpusErr =
    cpusVal !== null && (!Number.isFinite(cpusVal) || cpusVal <= 0) ? t(lang, "wzCpusErr") : "";
  const memErr =
    memVal !== null && (!Number.isFinite(memVal) || !Number.isInteger(memVal) || memVal < 128)
      ? t(lang, "wzMemErr")
      : "";
  const resErr = cpusErr !== "" || memErr !== "" ? t(lang, "wzResErr") : "";
  const canSubmit =
    scenarios !== null &&
    env !== null &&
    nameOk &&
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
        services,
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

  // ── summary values ──────────────────────────────────────────────────

  const svcOn = (["code_server", "vnc", "pi", "pi_web"] as const).filter((k) => services[k]);
  const svcCount = svcOn.length;

  const alwaysOn = useMemo(
    () => (scenarios ?? []).filter((s) => s.always_on),
    [scenarios],
  );
  const optionalSel = useMemo(() => {
    if (env === null || scenarios === null) return [] as Scenario[];
    return scenarios.filter(
      (s) =>
        !s.always_on &&
        s.id !== "pi" &&
        s.id !== "pi-web" &&
        env.scenarios.includes(s.id),
    );
  }, [env, scenarios]);
  const scnText = [
    ...alwaysOn.map(
      (s) => `${s.name} ${env?.versions[s.id] ?? s.default_version ?? ""}`.trim(),
    ),
    ...optionalSel.map((s) => s.name),
  ].join(" · ");

  const resText =
    cpus.trim() === "" && memMb.trim() === ""
      ? t(lang, "sbUnlimited")
      : [cpus.trim() !== "" ? `${cpus.trim()} ${t(lang, "sbCores")}` : "", memMb.trim() !== "" ? `${memMb.trim()} MB` : ""]
          .filter(Boolean)
          .join(" · ");

  /** App image tag a same-env create would reuse, or null = new build.
   * See the header comment: match on the env_hash surface (scenarios +
   * versions, pi/pi-web folded like routes.rs normalize_services). */
  const reuse = useMemo(() => {
    if (env === null || knownSbx === null) return undefined; // not loaded yet
    const folded = foldServices(env, services);
    const key = envKey(folded);
    const sbx = knownSbx.find((sb) => !sb.adopted && envKey(sb.env) === key);
    if (sbx !== undefined) return sbx.image;
    if (knownImg !== null) {
      for (const img of knownImg) {
        if (img.combo !== null && comboKey(img.combo) === key) {
          return img.tag.replace("sandbox-base-", "sandbox-app-");
        }
      }
    }
    return null;
  }, [env, services, knownSbx, knownImg]);

  const stName = name === "" ? t(lang, "wzStPending") : nameOk ? name : t(lang, "wzStInvalid");
  const stScn = `${alwaysOn.length} ${fmt(lang, "wzStScn", optionalSel.length)}`;

  const svcLabels: [keyof typeof services, string][] = [
    ["code_server", t(lang, "svcCode_server")],
    ["vnc", t(lang, "svcVnc")],
    ["pi", t(lang, "svcPi")],
    ["pi_web", t(lang, "svcPi_web")],
  ];

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>{t(lang, "wzTitle")}</h1>
        </div>
        <div className="page-actions">
          <button className="btn btn-secondary" onClick={onCancel}>
            {t(lang, "cancel")}
          </button>
        </div>
      </div>

      <form
        ref={formRef}
        className="wz"
        noValidate
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <div className="wz-form">
          {/* 1 · 名称 */}
          <section className="sec" id="s-name">
            <div className="sec-head">
              <h2>
                <span className="n">1</span>
                {t(lang, "wzName")}
              </h2>
            </div>
            <div className={`field${nameErr !== "" ? " invalid" : ""}`} style={{ maxWidth: 420 }}>
              <label htmlFor="wz-name">{t(lang, "wzNameField")}</label>
              <div className="input-wrap">
                <input
                  id="wz-name"
                  className="input mono"
                  value={name}
                  placeholder={t(lang, "wzNamePh")}
                  autoComplete="off"
                  aria-invalid={nameErr !== ""}
                  autoFocus
                  onChange={(e) => setName(e.target.value.trim())}
                />
                <span className="addon num">
                  {name.length} / 32
                </span>
              </div>
              <span className="hint">{t(lang, "wzNameHint")}</span>
              <span className="err">{t(lang, "wzNameErr")}</span>
              <span className="subdomain">
                sbx-<b>{name || "…"}</b>.mgr.localhost
              </span>
            </div>
          </section>

          {/* 2 · 服务 */}
          <section className="sec" id="s-svc">
            <div className="sec-head">
              <h2>
                <span className="n">2</span>
                {t(lang, "wzServices")}
              </h2>
              <p>{t(lang, "wzServicesHint")}</p>
            </div>
            <ServicesPicker lang={lang} services={services} onChange={setServices} />
          </section>

          {/* 3 · 场景 */}
          <section className="sec" id="s-scn">
            <div className="sec-head">
              <h2>
                <span className="n">3</span>
                {t(lang, "wzScenarios")}
              </h2>
              <p>{t(lang, "wzScenariosHint")}</p>
            </div>
            {scenarios === null || env === null ? (
              <div className="status">{t(lang, "loading")}</div>
            ) : (
              <EnvPicker lang={lang} scenarios={scenarios} env={env} onChange={setEnv} />
            )}
          </section>

          {/* 4 · 资源 */}
          <section className="sec" id="s-res">
            <div className="sec-head">
              <h2>
                <span className="n">4</span>
                {t(lang, "wzResources")}
              </h2>
              <p>{t(lang, "wzResHint")}</p>
            </div>
            <div className="field-row" style={{ maxWidth: 420 }}>
              <div className={`field${cpusErr !== "" ? " invalid" : ""}`}>
                <label htmlFor="wz-cpus">{t(lang, "wzCpus")}</label>
                <input
                  id="wz-cpus"
                  className="input mono"
                  value={cpus}
                  placeholder={t(lang, "wzCpusPh")}
                  inputMode="decimal"
                  aria-invalid={cpusErr !== ""}
                  onChange={(e) => setCpus(e.target.value)}
                />
                <span className="err">{t(lang, "wzCpusErr")}</span>
              </div>
              <div className={`field${memErr !== "" ? " invalid" : ""}`}>
                <label htmlFor="wz-mem">{t(lang, "wzMem")}</label>
                <div className="input-wrap">
                  <input
                    id="wz-mem"
                    className="input mono"
                    value={memMb}
                    placeholder={t(lang, "wzMemPh")}
                    inputMode="numeric"
                    aria-invalid={memErr !== ""}
                    onChange={(e) => setMemMb(e.target.value)}
                  />
                  <span className="addon">MB</span>
                </div>
                <span className="err">{t(lang, "wzMemErr")}</span>
              </div>
            </div>
          </section>
        </div>

        {/* 右侧:步骤 + 摘要 */}
        <aside className="side">
          <nav className="steps" aria-label={t(lang, "wzSteps")}>
            <a
              className={activeSec === "s-name" ? "active" : nameOk ? "done" : ""}
              {...stepProps("s-name")}
            >
              <span className="n">1</span>
              {t(lang, "wzName")}
              <span className="st">{stName}</span>
            </a>
            <a className={activeSec === "s-svc" ? "active" : ""} {...stepProps("s-svc")}>
              <span className="n">2</span>
              {t(lang, "wzServices")}
              <span className="st">
                {svcCount} / 4 {t(lang, "wzStSvcCount")}
              </span>
            </a>
            <a className={activeSec === "s-scn" ? "active" : ""} {...stepProps("s-scn")}>
              <span className="n">3</span>
              {t(lang, "wzScenarios")}
              <span className="st">{stScn}</span>
            </a>
            <a className={activeSec === "s-res" ? "active" : ""} {...stepProps("s-res")}>
              <span className="n">4</span>
              {t(lang, "wzResources")}
              <span className="st">{resText}</span>
            </a>
          </nav>
          <div className="card sum">
            <h3>{t(lang, "wzSumTitle")}</h3>
            <dl>
              <div>
                <dt>{t(lang, "wzSumUrl")}</dt>
                <dd className="mono txs">sbx-{name || "…"}.mgr.localhost</dd>
              </div>
              <div>
                <dt>{t(lang, "wzSumSvc")}</dt>
                <dd className="chips">
                  {svcLabels.map(([key, label]) => (
                    <span key={key} className={`chip${services[key] ? "" : " off"}`}>
                      <span className={`sdot${services[key] ? "" : " stopped"}`} />
                      {label}
                    </span>
                  ))}
                </dd>
              </div>
              <div>
                <dt>{t(lang, "wzSumScn")}</dt>
                <dd>{scnText}</dd>
              </div>
              <div>
                <dt>{t(lang, "wzSumRes")}</dt>
                <dd>{resText}</dd>
              </div>
              <div>
                <dt>{t(lang, "wzSumImg")}</dt>
                <dd>
                  {reuse === undefined ? (
                    <span className="muted txs">…</span>
                  ) : reuse !== null ? (
                    <>
                      <span className="badge badge-ok">
                        <span className="dot" />
                        {t(lang, "wzReuseImg")}
                      </span>
                      <div className="mono txs" style={{ marginTop: 4 }}>
                        {reuse}
                      </div>
                      <div className="fine" style={{ marginTop: 4 }}>
                        {t(lang, "wzReuseImgFine")}
                      </div>
                    </>
                  ) : (
                    <>
                      <span className="badge badge-neutral">{t(lang, "wzNewImg")}</span>
                      <div className="fine" style={{ marginTop: 4 }}>
                        {t(lang, "wzNewImgFine")}
                      </div>
                    </>
                  )}
                </dd>
              </div>
            </dl>
            <button className="btn btn-primary" type="submit" disabled={!canSubmit}>
              {submitting ? t(lang, "wzSubmitting") : t(lang, "wzSubmit")}
            </button>
            {msg && (
              <p className="fine" style={{ color: msg.kind === "err" ? "var(--danger)" : "var(--success)" }}>
                {msg.text}
              </p>
            )}
            <p className="fine">{t(lang, "wzSumFine")}</p>
          </div>
        </aside>
      </form>
    </div>
  );
}

// ── image-reuse key helpers ────────────────────────────────────────

/** The wizard's env, folded to the wire shape like routes.rs
 * normalize_services: pi / pi-web switches travel as scenario ids. */
function foldServices(env: SandboxEnv, services: { pi: boolean; pi_web: boolean }): SandboxEnv {
  const s = new Set(env.scenarios);
  s.delete("pi");
  s.delete("pi-web");
  if (services.pi_web) {
    s.add("pi");
    s.add("pi-web");
  } else if (services.pi) {
    s.add("pi");
  }
  return { scenarios: [...s], versions: env.versions };
}

/** Canonical env key over the env_hash surface: sorted scenario ids +
 * id-sorted "id@label" versions (mirrors envhash.rs canonical_json and
 * describe_combo's ordering). */
function envKey(env: SandboxEnv): string {
  const scns = [...env.scenarios].sort().join("+") || "(base)";
  const vers = Object.entries(env.versions)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([id, label]) => `${id}@${label}`)
    .join(",");
  return `${scns}|${vers}`;
}

/** Parse an image row's combo description (describe_combo format
 * "<scns+> <id@label,...> [svc: ...]") back into the envKey shape; null when
 * the string doesn't match (pre-S3 rows, future format drift). */
function comboKey(combo: string): string | null {
  const m = combo.match(/^(.*) \[svc: [^\]]*\]$/);
  if (m === null) return null;
  const parts = m[1].trim().split(" ");
  const scns = parts[0] ?? "";
  const vers = parts[1] ?? "";
  return `${scns}|${vers}`;
}

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
