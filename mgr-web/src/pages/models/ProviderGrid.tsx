// ProviderGrid — the provider library's card wall (cc-switch refactor,
// task 10-04-model-config-cc-switch Phase 3).
//
// Each provider is a `.card.pv` BUTTON: head (name + protocol badge), mono
// baseUrl, model-id chips, and a "key configured" footer. A trailing
// `.pv.add` ghost card opens the editor modal on a blank provider.
//
// Read-only view over the provider library; edits (including delete) happen
// in the ProviderEditorModal. There is no agent-binding footer anymore: the
// cc-switch refactor drops mgr-side agent bindings entirely (each sandbox
// owns its agent configs end-to-end).

import { Icon } from "../../icons";
import { t, type Lang } from "../../i18n";
import { protocolBadge, type ProviderLibrary } from "./types";

export function ProviderGrid({
  library,
  onSelect,
  onAdd,
  lang,
}: {
  library: ProviderLibrary;
  onSelect: (id: string) => void;
  onAdd: () => void;
  lang: Lang;
}): JSX.Element {
  const ids = Object.keys(library.providers);

  return (
    <div className="pv-grid">
      {ids.map((id) => {
        const p = library.providers[id];
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
                <span
                  className="chip off mono"
                  title={p.models.slice(12).map((m) => m.id).join("\n")}
                >
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
              {p.category && (
                <>
                  <span>·</span>
                  <span className="badge badge-neutral">{p.category}</span>
                </>
              )}
            </div>
          </button>
        );
      })}

      {/* trailing add-provider ghost card (click → blank editor modal) */}
      <button className="card pv add" onClick={onAdd}>
        <Icon name="plus" />
        <span className="tsm">{t(lang, "mcAddProvider")}</span>
      </button>
    </div>
  );
}
