// ModelPicker — stateless model-id picker over a provider's model list.
//
// Minimal reusable primitive (08-27-provider-form-piweb design §2): a
// searchable list that calls back with the chosen model id. Deliberately has
// no provider-selection layer of its own — R2/R3 (08-27-agent-tabs-live-config)
// wrap this with a provider dropdown for the "pick provider, then pick one of
// its models" agent-assignment flow; this component only owns the model list.

import { useState } from "react";
import { t, type Lang } from "../../i18n";
import type { ModelEntry } from "./types";

export function ModelPicker({
  models,
  selectedId,
  onPick,
  lang,
}: {
  models: ModelEntry[];
  selectedId?: string;
  onPick: (modelId: string) => void;
  lang: Lang;
}): JSX.Element {
  const [filter, setFilter] = useState("");
  const q = filter.trim().toLowerCase();
  const shown = q
    ? models.filter(
        (m) =>
          m.id.toLowerCase().includes(q) ||
          (m.name ?? "").toLowerCase().includes(q),
      )
    : models;

  return (
    <div className="ml-model-picker">
      <input
        className="ml-discover-search"
        placeholder={t(lang, "mcSearch")}
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
      />
      <div className="ml-discover-list">
        {shown.length === 0 ? (
          <div className="ml-hint">—</div>
        ) : (
          shown.map((m) => (
            <button
              key={m.id}
              className={`ml-model-picker-item${m.id === selectedId ? " is-selected" : ""}`}
              onClick={() => onPick(m.id)}
            >
              <span className="ml-discover-id">{m.id}</span>
              {m.name && <span className="ml-discover-name">{m.name}</span>}
            </button>
          ))
        )}
      </div>
    </div>
  );
}
