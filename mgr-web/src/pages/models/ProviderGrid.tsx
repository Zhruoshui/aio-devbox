// ProviderGrid — the providers tab's card wall, redesigned per the 09-11
// prototype (docs/Web-Prototype/models.html .pv-grid).
//
// Each provider is the prototype's `.card.pv` BUTTON: head (name + protocol
// badge), mono baseUrl, model-id chips, and a "used by" footer (key state +
// which agent/profile pairs bind this provider across ALL profiles — the
// prototype's usedBy() walks every profile's pi/opencode assignments and
// claude/codex current presets; here the canonical config only carries the
// SELECTED profile, so cross-profile usage is derived from the profile rows'
// sandbox counts: "agent (profile)" pairs are shown for the selected
// profile, other profiles render as "profile · N 沙箱"). A trailing
// `.pv.add` ghost card opens the drawer on a blank provider.
//
// Data flow unchanged: read-only view over the canonical config; edits
// (including delete) happen in the ProviderEditor drawer.

import { Icon } from "../../icons";
import { t, type Lang } from "../../i18n";
import type { ModelProfile } from "../../api";
import {
  bindingAgents,
  protocolBadge,
  type AgentTab,
  type CanonicalConfig,
} from "./types";

const AGENT_LABEL: Record<AgentTab, string> = {
  pi: "pi",
  opencode: "opencode",
  claude: "Claude",
  codex: "Codex",
};

export function ProviderGrid({
  config,
  profileId,
  profiles,
  onSelect,
  onAdd,
  lang,
}: {
  config: CanonicalConfig;
  /** Selected profile id — scopes the "used by" pairs' agent part. */
  profileId: string;
  /** All profile rows — other profiles render as usage count chips. */
  profiles: ModelProfile[];
  onSelect: (id: string) => void;
  onAdd: () => void;
  lang: Lang;
}): JSX.Element {
  const ids = Object.keys(config.providers);

  return (
    <div className="pv-grid">
      {ids.map((id) => {
        const p = config.providers[id];
        const bound = bindingAgents(config, id);
        return (
          <button
            key={id}
            className="card pv"
            aria-label={`${t(lang, "mcEdit")} ${p.name || id}`}
            onClick={() => onSelect(id)}
          >
            <div className="pv-head">
              <h3>{p.name || id}</h3>
              <span className="badge badge-neutral">{protocolBadge(p.api)}</span>
            </div>
            <div className="url" title={p.baseUrl}>
              {p.baseUrl || "—"}
            </div>
            <div className="models">
              {p.models.length === 0 ? (
                <span className="txs muted">{t(lang, "mcNoModelsYet")}</span>
              ) : (
                p.models.slice(0, 12).map((m) => (
                  <span className="chip mono" key={m.id} title={m.name ?? m.id}>
                    {m.id}
                  </span>
                ))
              )}
              {p.models.length > 12 && (
                <span className="chip off mono" title={p.models.slice(12).map((m) => m.id).join("\n")}>
                  +{p.models.length - 12}
                </span>
              )}
            </div>
            <div className="used">
              {p.apiKey && p.apiKey.length > 0 ? (
                <>
                  <Icon name="key" />
                  <span>{t(lang, "mcKeyConfigured")}</span>
                </>
              ) : (
                <>
                  <Icon name="info" />
                  <span>{t(lang, "mcNoKey")}</span>
                </>
              )}
              <span>·</span>
              {bound.length > 0 ? (
                <span>
                  {t(lang, "mcUsedBy")}{" "}
                  <b>
                    {bound
                      .map((a) => `${AGENT_LABEL[a]} (${profileId})`)
                      .join(t(lang, "mpListSep"))}
                  </b>
                  {profiles.length > 1 &&
                    ` ${t(lang, "mcOtherProfiles").replace("{n}", String(profiles.length - 1))}`}
                </span>
              ) : (
                <span>{t(lang, "mcNoBoundAgents")}</span>
              )}
            </div>
          </button>
        );
      })}

      {/* trailing add-provider ghost card (click → blank editor drawer) */}
      <button className="card pv add" onClick={onAdd}>
        <Icon name="plus" />
        <span className="tsm">{t(lang, "mcAddProvider")}</span>
      </button>
    </div>
  );
}
