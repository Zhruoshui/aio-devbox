// ModelPicker — stateless model-id picker over a provider's model list.
// Ported from web/src/panes/models/ModelPicker.tsx (Phase 4c); used by the
// pi/opencode assignment editor (the workbench also reuses it in its live
// provider flows, which mgr does not have).

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
