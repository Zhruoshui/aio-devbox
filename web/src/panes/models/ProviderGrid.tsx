// ProviderGrid — cc-switch style card grid for the 供应商库 tab.
//
// Each card is a Kumo LayerCard (white surface, ring + small shadow, 8px
// radius): protocol badge + name, hover-reveal edit/delete actions, mono
// baseUrl, model count + masked key, and chips for the agents that bind this
// provider (clicking a chip jumps to that agent tab). Clicking the card body
// opens the editor drawer (ModelsPane owns the open state).

import { Icon } from "../../icons";
import { t, type Lang } from "../../i18n";
import {
  bindingAgents,
  protocolLabel,
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
  onSelect,
  onAdd,
  onImport,
  onDelete,
  onJumpToAgent,
  lang,
}: {
  config: CanonicalConfig;
  onSelect: (id: string) => void;
  onAdd: () => void;
  onImport: () => void;
  onDelete: (id: string) => void;
  onJumpToAgent: (agent: AgentTab) => void;
  lang: Lang;
}): JSX.Element {
  const ids = Object.keys(config.providers);

  if (ids.length === 0) {
    return (
      <div className="ml-empty">
        <p>{t(lang, "mcNoProviders")}</p>
        <div className="ml-empty-actions">
          <button className="btn btn-primary" onClick={onAdd}>
            <Icon name="plus" />
            {t(lang, "mcAddProvider")}
          </button>
          <button className="btn btn-secondary" onClick={onImport}>
            {t(lang, "mcImportPi")}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="ml-grid">
      {ids.map((id) => {
        const p = config.providers[id];
        const bound = bindingAgents(config, id);
        return (
          <div
            key={id}
            className="ml-card"
            role="button"
            tabIndex={0}
            aria-label={p.name || id}
            onClick={() => onSelect(id)}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onSelect(id);
              }
            }}
          >
            <div className="ml-card-head">
              <div className="ml-card-main">
                <span className="ml-badge ml-badge-protocol" title={p.api}>
                  {protocolLabel(p.api)}
                </span>
                <span className="ml-card-name">{p.name || id}</span>
              </div>
              <span className="ml-card-actions">
                <button
                  className="icon-btn"
                  aria-label={t(lang, "mcEdit")}
                  title={t(lang, "mcEdit")}
                  onClick={(e) => {
                    e.stopPropagation();
                    onSelect(id);
                  }}
                >
                  <Icon name="edit" />
                </button>
                <button
                  className="icon-btn ml-card-del"
                  aria-label={t(lang, "mcDeleteProvider")}
                  title={t(lang, "mcDeleteProvider")}
                  onClick={(e) => {
                    e.stopPropagation();
                    onDelete(id);
                  }}
                >
                  <Icon name="trash" />
                </button>
              </span>
            </div>

            <div className="ml-card-url" title={p.baseUrl}>
              {p.baseUrl || "—"}
            </div>

            <div className="ml-card-meta">
              <span>
                {p.models.length} {t(lang, "mcModels")}
              </span>
              <span className="dot">·</span>
              <span className="ml-card-key">
                {p.apiKey && p.apiKey.length > 0 ? p.apiKey : "—"}
              </span>
            </div>

            <div className="ml-card-chips">
              {bound.length > 0 ? (
                bound.map((a) => (
                  <button
                    key={a}
                    className="ml-chip"
                    onClick={(e) => {
                      e.stopPropagation();
                      onJumpToAgent(a);
                    }}
                  >
                    {AGENT_LABEL[a]}
                  </button>
                ))
              ) : (
                <span className="none">{t(lang, "mcNoBoundAgents")}</span>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}
