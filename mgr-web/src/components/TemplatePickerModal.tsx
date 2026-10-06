// TemplatePickerModal — pick a provider template from the mgr's shared
// provider library (task 10-04-model-config-cc-switch Phase 4, prd R3).
//
// The sandbox-side agent panels open this when creating a provider so the
// user can copy baseUrl/api/models from a library entry instead of typing
// them. The library's apiKey is NEVER transmitted to the sandbox (the
// /api/models/templates route strips it entirely — types.ts
// ProviderTemplatePublic); the user pastes their own key after copying.
//
// Chrome follows the project's Dialog contract (components/Dialogs.tsx
// useDialogChrome): Escape / scrim close, focus moves to the search field.

import { useEffect, useRef, useState } from "react";
import { getProviderTemplates } from "../api";
import { useDialogChrome } from "./Dialogs";
import { Icon } from "../icons";
import { t, type Lang } from "../i18n";
import {
  protocolBadge,
  type ProviderTemplatePublic,
} from "../pages/models/types";

interface Props {
  onSelect: (template: ProviderTemplatePublic) => void;
  onClose: () => void;
  lang: Lang;
}

export function TemplatePickerModal({ onSelect, onClose, lang }: Props): JSX.Element {
  const firstRef = useRef<HTMLInputElement>(null);
  useDialogChrome(onClose, firstRef);

  const [templates, setTemplates] = useState<ProviderTemplatePublic[] | null>(null);
  const [error, setError] = useState("");
  const [filter, setFilter] = useState("");

  useEffect(() => {
    let cancelled = false;
    getProviderTemplates()
      .then((r) => {
        if (!cancelled) setTemplates(r.templates);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const needle = filter.trim().toLowerCase();
  const visible = (templates ?? []).filter(
    (tp) =>
      needle === "" ||
      tp.id.toLowerCase().includes(needle) ||
      tp.name.toLowerCase().includes(needle) ||
      tp.baseUrl.toLowerCase().includes(needle),
  );

  return (
    <div className="overlay open" role="presentation" onClick={onClose}>
      <div
        className="dialog tpl-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="tpl-title"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="dialog-head">
          <h2 id="tpl-title">{t(lang, "tplTitle")}</h2>
          <button className="icon-btn" aria-label={t(lang, "mcClose")} onClick={onClose}>
            <Icon name="x" />
          </button>
        </div>

        <div className="field">
          <input
            ref={firstRef}
            className="input"
            value={filter}
            placeholder={t(lang, "mcFilterPlaceholder")}
            onChange={(e) => setFilter(e.target.value)}
          />
        </div>

        <div className="tpl-list">
          {error !== "" && <p className="ml-msg err">{t(lang, "mcLoadFailed")}{error}</p>}
          {templates === null && error === "" && (
            <p className="muted">{t(lang, "mcLoading")}</p>
          )}
          {templates !== null && visible.length === 0 && (
            <p className="muted">{t(lang, "tplEmpty")}</p>
          )}
          {visible.map((tp) => (
            <button
              key={tp.id}
              className="tpl-row"
              onClick={() => onSelect(tp)}
              title={t(lang, "tplPickHint")}
            >
              <span className="tpl-row-head">
                <span className="tpl-row-name">{tp.name || tp.id}</span>
                <span className="badge">{protocolBadge(tp.api)}</span>
              </span>
              <span className="tpl-row-url mono">{tp.baseUrl}</span>
              <span className="muted txs">
                {t(lang, "tplModelCount").replace("{n}", String(tp.models.length))}
                {tp.models.length > 0 && ` · ${tp.models.map((m) => m.id).join(", ")}`}
              </span>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
