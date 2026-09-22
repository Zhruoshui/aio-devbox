// EnvPicker - scenario + version selection UI shared by the create wizard and
// the env editor (design §4: "环境配置编辑：同创建向导的场景/版本区").
//
// 09-11 prototype form (create-sandbox.html §3): per-layer `.layer` heading
// over a `.rows` surface; each scenario is a `.row` (check + name/desc +
// version select). Locked (always_on) rows carry the 「必装」 `.lock` chip
// and a disabled check; unchecked rows dim via `.off`.
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

import type { Lang, StringKey } from "../i18n";
import { t } from "../i18n";
import type { Scenario, SandboxEnv } from "../types";

interface Props {
  lang: Lang;
  scenarios: Scenario[];
  /** Current selection (controlled). */
  env: SandboxEnv;
  onChange: (env: SandboxEnv) => void;
}

/** Layer display order + label key (parent D2): L1 os / L2 shell / L3 lang /
 * L4 app. Unknown categories (future scenarios) render last, like the config
 * TUI's category_rank. */
const LAYERS: { cats: string[]; labelKey: StringKey }[] = [
  { cats: ["os"], labelKey: "layL1" },
  { cats: ["shell"], labelKey: "layL2" },
  { cats: ["lang"], labelKey: "layL3" },
  { cats: ["app"], labelKey: "layL4" },
];

/** Install "rungs" inside a layer, in display order. Mirrors the config TUI's
 * scenario::INSTALLER_ORDER / installer_rank, so both surfaces show the same
 * ladder; labels live in i18n (like LAYERS) rather than being sent by the
 * API. Unknown installers render last, keyed by their raw string. */
const INSTALLERS: { id: string; labelKey: StringKey }[] = [
  { id: "mise", labelKey: "insMise" },
  { id: "apt", labelKey: "insApt" },
  { id: "npm", labelKey: "insNpm" },
  { id: "tarball", labelKey: "insTarball" },
];

/** Rank for the rung order; unknown installers sort last. */
const installerRank = (id: string): number => {
  const i = INSTALLERS.findIndex((x) => x.id === id);
  return i === -1 ? INSTALLERS.length : i;
};

/** Label for a rung. Known installers get the i18n string; an unrecognised
 * one falls back to its raw value (same "never break on the unknown" rule as
 * the layer grouping). */
const installerLabel = (lang: Lang, id: string): string => {
  const hit = INSTALLERS.find((x) => x.id === id);
  return hit ? t(lang, hit.labelKey) : id;
};

/** Scenario ids owned by the services area (S1, parent D1): pi and pi-web are
 * surfaced there, NOT in the four-layer scenario section — a single switch
 * in one place, never two. EnvPicker still honors their presence in
 * env.scenarios internally (they travel in the same SandboxEnv). */
const SERVICE_SCENARIOS = ["pi", "pi-web"];

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

  const visible = scenarios.filter((s) => !SERVICE_SCENARIOS.includes(s.id));
  const layered = LAYERS.map(({ cats, labelKey }) => ({
    labelKey,
    items: visible.filter((s) => cats.includes(s.category)),
  })).filter((g) => g.items.length > 0);
  const unknown = visible.filter((s) => !LAYERS.some((l) => l.cats.includes(s.category)));

  const row = (s: Scenario) => {
    const checked = s.always_on || env.scenarios.includes(s.id);
    // Current version label: explicit selection > scenario default > first
    // offered (matches gen's resolve_version fallback chain).
    const current = env.versions[s.id] ?? s.default_version ?? s.versions[0] ?? "";
    return (
      <div key={s.id} className={`row${s.always_on ? " locked" : ""}${checked ? "" : " off"}`}>
        <input
          className="check"
          type="checkbox"
          checked={checked}
          disabled={s.always_on}
          aria-label={s.name}
          onChange={(e) => !s.always_on && toggle(s.id, e.target.checked)}
        />
        <div className="main-col">
          <div className="name">
            {s.name}
            {s.always_on && <span className="lock">{t(lang, "wzLocked")}</span>}
          </div>
          <div className="desc">{s.description}</div>
        </div>
        {s.versions.length > 0 && (
          <select
            className="input"
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
        )}
      </div>
    );
  };

  /** Rows of one layer, split into install rungs when the layer mixes
   * installers. A uniform layer (L2 shell is all-mise) renders exactly as
   * before - no sub-headings, no extra indent. */
  const layerRows = (items: Scenario[]) => {
    const rungs = [...new Set(items.map((s) => s.installer))];
    if (rungs.length < 2) {
      return <div className="rows" role="group">{items.map(row)}</div>;
    }
    rungs.sort((a, b) => installerRank(a) - installerRank(b));
    return (
      <div className="rows" role="group">
        {rungs.map((r) => (
          <div key={r} className="rung-group">
            <p className="rung">{installerLabel(lang, r)}</p>
            {items.filter((s) => s.installer === r).map(row)}
          </div>
        ))}
      </div>
    );
  };

  const group = (labelKey: StringKey, items: Scenario[]) => (
    <div key={labelKey}>
      <p className="layer">{t(lang, labelKey)}</p>
      {layerRows(items)}
    </div>
  );

  return (
    <div>
      {layered.map((g) => group(g.labelKey, g.items))}
      {unknown.length > 0 && group("layOther", unknown)}
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
