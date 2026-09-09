// EnvPicker - scenario + version selection UI shared by the create wizard and
// the env editor (design §4: "环境配置编辑：同创建向导的场景/版本区").
//
// Semantics mirror the config TUI (config/src/tui.rs + scenario.rs):
//   - always_on scenarios are LOCKED ON (checkbox disabled, "always-on" pill);
//     only their version dropdown is interactive. They are never part of
//     env.scenarios - the API shape rejects always_on ids there (envhash.rs)
//     - they only contribute env.versions entries.
//   - non-always_on scenarios are toggleable checkboxes.
//   - versioned scenarios (versions.length > 0) get a dropdown; the current
//     selection comes from the editor's env, defaulting to the scenario's
//     default_version (falling back to versions[0] server-side in gen).
//
// The value is a SandboxEnv ({scenarios, versions}) - the same contract as
// POST/PUT bodies and the sandbox list's env field (types.ts single owner).

import type { Lang } from "../i18n";
import { t } from "../i18n";
import type { Scenario, SandboxEnv } from "../types";

interface Props {
  lang: Lang;
  scenarios: Scenario[];
  /** Current selection (controlled). */
  env: SandboxEnv;
  onChange: (env: SandboxEnv) => void;
}

export function EnvPicker({ lang, scenarios, env, onChange }: Props): JSX.Element {
  const toggle = (id: string, on: boolean) => {
    if (on) {
      onChange({ ...env, scenarios: [...env.scenarios, id] });
    } else {
      onChange({
        ...env,
        scenarios: env.scenarios.filter((s) => s !== id),
      });
    }
  };

  const setVersion = (id: string, label: string) => {
    onChange({ ...env, versions: { ...env.versions, [id]: label } });
  };

  return (
    <div>
      <div className="field">
        <label>{t(lang, "wzScenarios")}</label>
        <span className="hint">{t(lang, "wzScenariosHint")}</span>
      </div>
      <div className="scn-list" role="group" aria-label={t(lang, "wzScenarios")}>
        {scenarios.map((s) => {
          const checked = s.always_on || env.scenarios.includes(s.id);
          // Current version label: explicit selection > scenario default >
          // first offered (matches gen's resolve_version fallback chain).
          const current =
            env.versions[s.id] ?? s.default_version ?? s.versions[0] ?? "";
          return (
            <div key={s.id} className={`scn-row${s.always_on ? " locked" : ""}`}>
              <input
                className="check"
                type="checkbox"
                checked={checked}
                disabled={s.always_on}
                aria-label={s.name}
                onChange={(e) => !s.always_on && toggle(s.id, e.target.checked)}
              />
              <div className="scn-main">
                <span className="scn-name">
                  <span>{s.name}</span>
                  {s.always_on && <span className="scn-lock">{t(lang, "wzLocked")}</span>}
                </span>
                <span className="scn-desc">{s.description}</span>
              </div>
              {s.versions.length > 0 && (
                <div className="scn-ver">
                  <select
                    aria-label={`${s.name} ${t(lang, "wzVersion")}`}
                    value={current}
                    onChange={(e) => setVersion(s.id, e.target.value)}
                  >
                    {s.versions.map((label) => (
                      <option key={label} value={label}>
                        {label}
                      </option>
                    ))}
                  </select>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

/** Build the initial SandboxEnv for a picker from the scenario catalog:
 * always_on scenarios pre-seed their default version; selectable scenarios
 * start unchecked (wizard) unless the caller passes an existing env. */
export function defaultEnv(scenarios: Scenario[]): SandboxEnv {
  const versions: Record<string, string> = {};
  for (const s of scenarios) {
    if (s.versions.length > 0) {
      const v = s.default_version ?? s.versions[0];
      if (v !== undefined) versions[s.id] = v;
    }
  }
  return { scenarios: [], versions };
}
