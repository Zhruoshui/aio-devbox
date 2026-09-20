// ServicesPicker - the S1 four-switch "services" area of the create wizard
// (parent D1): code-server / vnc / pi / pi-web are a distinct concern from
// scenario selection (EnvPicker) — pi/pi-web ARE scenarios, but the UI
// surfaces them here (services), not in the four-layer scenario section.
//
// 09-11 prototype form (create-sandbox.html §2): a `.rows` surface where
// each service is a `.row` (name + description + `.switch` toggle).
//
// 09-20 grouped layout (09-20-sandbox-service-buttons, prd Req 1 iteration):
// the four switches render as TWO semantic groups — base services
// (independently toggleable) and the combined service (pi Web, whose
// dependencies live in the base group). Grouping is semantics, not
// decoration: base rows are free switches, while the combo group carries a
// LIVE dependency status line under pi Web (✓/✗ per dependency) plus the
// cascade hint, so "turning a base service off also turns pi Web off" is
// visible BEFORE the flip. Readonly (image detail) keeps the grouping but
// degrades the status line to installed/not-installed words (svcOn/svcOff)
// and drops the cascade hint — it describes the past, not an editable state.
//
// Dependency rule (mirrors routes.rs normalize_services): pi-web needs
// pi (its config lives under the pi install) and vnc (its Chromium is the
// vnc sidecar). Enabling pi-web auto-enables both; disabling pi or vnc
// while pi-web is on CASCADES pi-web off (the backend 400 line stays as the
// old-client defence).
//
// The value is a ServicesInput ({code_server, vnc, pi, pi_web}).

import type { Lang, StringKey } from "../i18n";
import { t } from "../i18n";
import type { ServicesInput } from "../types";

interface Props {
  lang: Lang;
  /** Current selection (controlled). */
  services: ServicesInput;
  onChange: (services: ServicesInput) => void;
  /** Read-only (env edit page): installed set is fixed by the image. */
  readonly?: boolean;
}

type ServiceKey = keyof ServicesInput;

/** One service entry: the ServicesInput key plus its i18n keys (label +
 * description). The keys are load-bearing (normalize_services on the
 * backend and the dependency linkage below switch on them) - never pass a
 * display label where a key belongs. */
interface ServiceDef {
  key: ServiceKey;
  labelKey: "svcCode_server" | "svcVnc" | "svcPi" | "svcPi_web";
  descKey: StringKey;
}

/** Base group: independently toggleable services (prd Req 1 iteration -
 * "基础服务" / base services). */
const BASE_SERVICES: ServiceDef[] = [
  { key: "code_server", labelKey: "svcCode_server", descKey: "svcCsDesc" },
  { key: "vnc", labelKey: "svcVnc", descKey: "svcVncDesc" },
  { key: "pi", labelKey: "svcPi", descKey: "svcPiDesc" },
];

/** Combo group: the single service that DEPENDS on base services ("组合服
 * 务" / combined services). pi_web is the only member today. */
const COMBO_SERVICES: ServiceDef[] = [
  { key: "pi_web", labelKey: "svcPi_web", descKey: "svcPiWebDesc" },
];

/** pi Web's dependencies in display order (mirrors the cascade in `set`):
 * vnc first (its Chromium is pi Web's browser proxy), then pi. Reuses the
 * plain service-label i18n keys - no duplicate naming. */
const PI_WEB_DEPS: { key: ServiceKey; labelKey: "svcVnc" | "svcPi" }[] = [
  { key: "vnc", labelKey: "svcVnc" },
  { key: "pi", labelKey: "svcPi" },
];

export function ServicesPicker({ lang, services, onChange, readonly }: Props): JSX.Element {
  const set = (key: ServiceKey, v: boolean) => {
    const next = { ...services, [key]: v };
    // pi-web dependency (mirror of routes.rs normalize_services): turning
    // it on pulls pi + vnc; turning pi or vnc off while pi-web is on
    // CASCADES pi-web off too (prd Req 1, user-confirmed: one flip lands the
    // whole intent - the old silent "do nothing" left a toggle that seemed
    // broken). The backend 400 line stays as the old-client defence; the
    // cascade guarantees this client never sends the contradictory combo.
    // LOGIC UNCHANGED by the 09-20 grouping iteration - only the rendering
    // around it is new.
    if (key === "pi_web" && v) {
      next.pi = true;
      next.vnc = true;
    } else if (key === "pi" && !v && next.pi_web) {
      next.pi_web = false;
    } else if (key === "vnc" && !v && next.pi_web) {
      next.pi_web = false;
    }
    onChange(next);
  };

  // Shared row renderer for both groups (same .row anatomy as 09-11: name +
  // desc + switch, or the svc-dot status in readonly mode).
  const renderRow = ({ key, labelKey, descKey }: ServiceDef): JSX.Element => {
    const on = services[key];
    const name = t(lang, labelKey);
    // pi / VNC carry a "pi Web depends on this" tag while pi Web is on
    // (Req 1): the cascade in `set` will take them down together, so the
    // dependency must be visible BEFORE the flip (complements the status
    // line under pi Web, which reads the OTHER direction). Readonly (image
    // detail) rows describe the past, not an editable state - no tag.
    const dependedByPiWeb =
      !readonly && services.pi_web && (key === "pi" || key === "vnc");
    return (
      <label key={key} className={`row${on ? "" : " off"}`}>
        <div className="main-col">
          <div className="name">
            {name}
            {dependedByPiWeb && <span className="lock">{t(lang, "svcDependedBy")}</span>}
          </div>
          <div className="desc">{t(lang, descKey)}</div>
        </div>
        {readonly ? (
          <span
            className={`svc-dot${on ? " on" : " off"}`}
            title={on ? t(lang, "svcOn") : t(lang, "svcOff")}
          />
        ) : (
          <input
            className="switch"
            type="checkbox"
            checked={on}
            aria-label={name}
            onChange={(e) => set(key, e.target.checked)}
          />
        )}
      </label>
    );
  };

  // Live dependency status line under pi Web (09-20): one ✓/✗ item per
  // dependency, coloured by state (.ok = success green / .no = muted).
  // Readonly mode renders pure installed/not-installed words (svcOn/svcOff)
  // instead of toggle semantics and never shows the cascade hint.
  const showCascadeHint = !readonly && services.pi_web;
  const renderDeps = (): JSX.Element => (
    <div className="svc-deps">
      <span className="svc-deps-label">{t(lang, "svcDepends")}</span>
      {PI_WEB_DEPS.map(({ key, labelKey }, i) => {
        const on = services[key];
        const item = readonly
          ? `${on ? "✓" : "✗"} ${t(lang, labelKey)} · ${t(lang, on ? "svcOn" : "svcOff")}`
          : `${on ? "✓" : "✗"} ${t(lang, labelKey)}`;
        return (
          <span key={key} className={`svc-dep${on ? " ok" : " no"}`}>
            {i > 0 && <span className="svc-deps-sep">·</span>}
            {item}
          </span>
        );
      })}
      {showCascadeHint && <span className="svc-dep-hint">{t(lang, "svcCascadeHint")}</span>}
    </div>
  );

  return (
    <div role="group" aria-label={t(lang, "wzServices")}>
      {/* Base group (09-20): aria-labelled container so screen readers get
          the same base-vs-combo split the visual grouping shows. */}
      <div className="svc-group" role="group" aria-label={t(lang, "svcGroupBase")}>
        <div className="svc-group-title">{t(lang, "svcGroupBase")}</div>
        <div className="rows">{BASE_SERVICES.map(renderRow)}</div>
      </div>
      <div className="svc-group" role="group" aria-label={t(lang, "svcGroupCombo")}>
        <div className="svc-group-title">{t(lang, "svcGroupCombo")}</div>
        <div className="rows">
          {COMBO_SERVICES.map(renderRow)}
          {renderDeps()}
        </div>
      </div>
    </div>
  );
}

/** Build the all-on default from services alone (create wizard). */
export function defaultServices(): ServicesInput {
  return { code_server: true, vnc: true, pi: true, pi_web: true };
}
