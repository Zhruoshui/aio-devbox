// ServicesPicker - the S1 four-switch "services" area of the create wizard
// (parent D1): code-server / vnc / pi / pi-web are a distinct concern from
// scenario selection (EnvPicker) — pi/pi-web ARE scenarios, but the UI
// surfaces them here (services), not in the four-layer scenario section.
//
// Dependency rule (mirrors routes.rs normalize_services): pi-web needs
// pi (its config lives under the pi install) and vnc (its Chromium is the
// vnc sidecar). Enabling pi-web auto-enables both; disabling pi or vnc
// while pi-web is on is prevented client-side (the backend re-validates).
//
// The value is a ServicesInput ({code_server, vnc, pi, pi_web}).

import type { Lang } from "../i18n";
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

/** The four services in display order: the ServicesInput key plus its i18n
 * key. The keys are load-bearing (normalize_services on the backend and the
 * dependency linkage below switch on them) - never pass a display label
 * where a key belongs. */
const SERVICES: { key: keyof ServicesInput; labelKey: "svcCode_server" | "svcVnc" | "svcPi" | "svcPi_web" }[] = [
  { key: "code_server", labelKey: "svcCode_server" },
  { key: "vnc", labelKey: "svcVnc" },
  { key: "pi", labelKey: "svcPi" },
  { key: "pi_web", labelKey: "svcPi_web" },
];

export function ServicesPicker({ lang, services, onChange, readonly }: Props): JSX.Element {
  const set = (key: keyof ServicesInput, v: boolean) => {
    const next = { ...services, [key]: v };
    // pi-web dependency (mirror of routes.rs normalize_services): turning
    // it on pulls pi + vnc; turning pi or vnc off while pi-web is on is
    // refused (a broken pane > a silent toggle). The backend 400s the
    // combination anyway - this is the client-side half of AC3.
    if (key === "pi_web" && v) {
      next.pi = true;
      next.vnc = true;
    } else if (key === "pi" && !v && next.pi_web) {
      return;
    } else if (key === "vnc" && !v && next.pi_web) {
      return;
    }
    onChange(next);
  };

  return (
    <div className="field">
      <label>{t(lang, "wzServices")}</label>
      <span className="hint">{t(lang, "wzServicesHint")}</span>
      <div className="scn-list" role="group" aria-label={t(lang, "wzServices")}>
        {SERVICES.map(({ key, labelKey }) => {
          const on = services[key];
          const name = t(lang, labelKey);
          return (
            <div key={key} className="scn-row">
              {readonly ? (
                <span
                  className={`svc-dot${on ? " on" : " off"}`}
                  title={on ? t(lang, "svcOn") : t(lang, "svcOff")}
                />
              ) : (
                <input
                  className="check"
                  type="checkbox"
                  checked={on}
                  aria-label={name}
                  onChange={(e) => set(key, e.target.checked)}
                />
              )}
              <div className="scn-main">
                <span className="scn-name">
                  <span>{name}</span>
                  {key === "pi_web" && !readonly && (
                    <span className="scn-hint-inline">{t(lang, "svcPiWebDep")}</span>
                  )}
                </span>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

/** Build the all-on default from services alone (create wizard). */
export function defaultServices(): ServicesInput {
  return { code_server: true, vnc: true, pi: true, pi_web: true };
}