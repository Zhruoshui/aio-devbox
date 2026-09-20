// ServicesPicker - the S1 four-switch "services" area of the create wizard
// (parent D1): code-server / vnc / pi / pi-web are a distinct concern from
// scenario selection (EnvPicker) — pi/pi-web ARE scenarios, but the UI
// surfaces them here (services), not in the four-layer scenario section.
//
// 09-11 prototype form (create-sandbox.html §2): a `.rows` surface where
// each service is a `.row` (name + description + `.switch` toggle).
//
// Dependency rule (mirrors routes.rs normalize_services): pi-web needs
// pi (its config lives under the pi install) and vnc (its Chromium is the
// vnc sidecar). Enabling pi-web auto-enables both; disabling pi or vnc
// while pi-web is on is prevented client-side (the backend re-validates).
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

/** The four services in display order: the ServicesInput key plus its i18n
 * keys (label + description). The keys are load-bearing (normalize_services
 * on the backend and the dependency linkage below switch on them) - never
 * pass a display label where a key belongs. */
const SERVICES: {
  key: keyof ServicesInput;
  labelKey: "svcCode_server" | "svcVnc" | "svcPi" | "svcPi_web";
  descKey: StringKey;
}[] = [
  { key: "code_server", labelKey: "svcCode_server", descKey: "svcCsDesc" },
  { key: "vnc", labelKey: "svcVnc", descKey: "svcVncDesc" },
  { key: "pi", labelKey: "svcPi", descKey: "svcPiDesc" },
  { key: "pi_web", labelKey: "svcPi_web", descKey: "svcPiWebDesc" },
];

export function ServicesPicker({ lang, services, onChange, readonly }: Props): JSX.Element {
  const set = (key: keyof ServicesInput, v: boolean) => {
    const next = { ...services, [key]: v };
    // pi-web dependency (mirror of routes.rs normalize_services): turning
    // it on pulls pi + vnc; turning pi or vnc off while pi-web is on
    // CASCADES pi-web off too (prd Req 1, user-confirmed: one flip lands the
    // whole intent - the old silent "do nothing" left a toggle that seemed
    // broken). The backend 400 line stays as the old-client defence; the
    // cascade guarantees this client never sends the contradictory combo.
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

  return (
    <div className="rows" role="group" aria-label={t(lang, "wzServices")}>
      {SERVICES.map(({ key, labelKey, descKey }) => {
        const on = services[key];
        const name = t(lang, labelKey);
        // pi / VNC carry a "pi Web depends on this" tag while pi Web is on
        // (Req 1): the cascade below will take them down together, so the
        // dependency must be visible BEFORE the flip. Readonly (image
        // detail) rows describe the past, not an editable state - no tag.
        const dependedByPiWeb =
          !readonly && services.pi_web && (key === "pi" || key === "vnc");
        return (
          <label key={key} className={`row${on ? "" : " off"}`}>
            <div className="main-col">
              <div className="name">
                {name}
                {key === "pi_web" && !readonly && <span className="lock">{t(lang, "svcPiWebDep")}</span>}
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
      })}
    </div>
  );
}

/** Build the all-on default from services alone (create wizard). */
export function defaultServices(): ServicesInput {
  return { code_server: true, vnc: true, pi: true, pi_web: true };
}
