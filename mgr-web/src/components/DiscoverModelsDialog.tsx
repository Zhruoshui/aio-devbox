// DiscoverModelsDialog — the shared "fetch models → multi-select" dialog
// (task 10-13-models-config-ux R5), extracted from ProviderEditorModal's
// inline discover overlay so the sandbox-side incremental node editor can
// run the same flow.
//
// The dialog is a pure view over DiscoverState (models + filter + selected
// Set): filter box, 全选/清空 shortcuts, a vertical checkbox list (one model
// per row, capped at 200 rows like the original), an "n / total" counter,
// and a confirm button that stays disabled until something is selected.

import { useRef } from "react";
import { useDialogChrome } from "./Dialogs";
import { Icon } from "../icons";
import { fmt, t, type Lang } from "../i18n";
import type { DiscoverState } from "../pages/models/types";

/** The visible (filter-passing) rows — the cap matches the original overlay. */
const MAX_ROWS = 200;

export function DiscoverModelsDialog({
  discover,
  onSet,
  onConfirm,
  lang,
}: {
  discover: DiscoverState;
  onSet: (d: DiscoverState | null) => void;
  /** Commit the current selection (parent merges into its own state). */
  onConfirm: () => void;
  lang: Lang;
}): JSX.Element {
  // Focus lands on the filter input once rendered (the first control).
  const filterRef = useRef<HTMLInputElement>(null);
  useDialogChrome(() => onSet(null), filterRef);

  const visible = discover.models
    .filter(
      (m) =>
        !discover.filter ||
        m.id.toLowerCase().includes(discover.filter.toLowerCase()),
    )
    .slice(0, MAX_ROWS);

  const toggle = (id: string, on: boolean) => {
    const next = new Set(discover.selected);
    if (on) next.add(id);
    else next.delete(id);
    onSet({ ...discover, selected: next });
  };

  return (
    <div className="overlay open" role="presentation">
      <div
        className="dialog discover-dialog"
        role="dialog"
        aria-modal="true"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="dialog-head">
          <h2>{t(lang, "mcDiscoverTitle")}</h2>
          <button
            className="icon-btn"
            aria-label={t(lang, "mcClose")}
            onClick={() => onSet(null)}
          >
            <Icon name="x" />
          </button>
        </div>
        <div className="dialog-body">
          {discover.loading && <p>{t(lang, "mcLoading")}</p>}
          {discover.error && <p className="ml-msg err">{discover.error}</p>}
          {!discover.loading && !discover.error && (
            <>
              <p className="muted txs">
                {t(lang, "mcDiscoverEndpoint")}: {discover.endpoint}
              </p>
              <div className="field">
                <input
                  ref={filterRef}
                  className="input"
                  placeholder={t(lang, "mcFilterPlaceholder")}
                  value={discover.filter}
                  onChange={(e) =>
                    onSet({ ...discover, filter: e.target.value })
                  }
                />
              </div>
              <div className="discover-toolbar">
                <button
                  type="button"
                  className="btn btn-ghost btn-sm"
                  onClick={() => {
                    const next = new Set(discover.selected);
                    for (const m of visible) next.add(m.id);
                    onSet({ ...discover, selected: next });
                  }}
                >
                  {t(lang, "mcSelectAll")}
                </button>
                <button
                  type="button"
                  className="btn btn-ghost btn-sm"
                  onClick={() => onSet({ ...discover, selected: new Set() })}
                >
                  {t(lang, "mcClearAll")}
                </button>
                <span className="flex-spacer" />
                <span className="muted txs">
                  {fmt(lang, "mcDiscoverSelected", discover.selected.size)} /{" "}
                  {visible.length}
                </span>
              </div>
              <div className="discover-list">
                {visible.map((m) => (
                  <label key={m.id} className="discover-row">
                    <input
                      type="checkbox"
                      checked={discover.selected.has(m.id)}
                      onChange={(e) => toggle(m.id, e.target.checked)}
                    />
                    <span className="mono">{m.id}</span>
                    {m.name && <span className="muted"> — {m.name}</span>}
                  </label>
                ))}
              </div>
            </>
          )}
        </div>
        <div className="dialog-actions">
          <span className="muted txs">
            {fmt(lang, "mcDiscoverSelected", discover.selected.size)}
          </span>
          <span className="flex-spacer" />
          <button
            className="btn btn-secondary"
            onClick={() => onSet(null)}
          >
            {t(lang, "cancel")}
          </button>
          <button
            className="btn btn-primary"
            disabled={discover.selected.size === 0 || discover.loading}
            onClick={onConfirm}
          >
            {t(lang, "mcDiscoverAdd")}
          </button>
        </div>
      </div>
    </div>
  );
}
