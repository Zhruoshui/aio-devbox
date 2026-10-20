// ProviderEditorModal — the cc-switch provider library's center modal
// (task 10-04-model-config-cc-switch Phase 3, prd R1/D6).
//
// Replaces the pre-redesign right-side drawer: a center `.overlay > .dialog`
// follows the project's Dialog chrome (components/Dialogs.tsx useDialogChrome)
// — Escape closes, scrim click closes, focus moves to the first control on
// open. Structure: head (title + close) / body (name+id+protocol row, Base
// URL, API key with show/hide, collapsible advanced headers JSON, model
// library with discover + per-model test pills) / foot (delete | dirty/msg |
// cancel | save).
//
// The catalog-fill button and the agent-binding overview are GONE (prd D6:
// models.dev proxy deleted; D1: mgr-side agent bindings deleted). The compat
// textarea is also gone — ProviderTemplate has no `compat` field (only
// `headers`).

import { useEffect, useRef, useState } from "react";
import { DiscoverModelsDialog } from "../../components/DiscoverModelsDialog";
import { useDialogChrome } from "../../components/Dialogs";
import { Icon } from "../../icons";
import { t, type Lang } from "../../i18n";
import { ModelRow } from "./ModelRow";
import {
  API_PROTOCOLS,
  type DiscoverState,
  type ProviderTemplate,
  type TemplateCost,
  type TemplateModelEntry,
  type TestStateMap,
} from "./types";

export function ProviderEditorModal({
  providerId,
  provider,
  isNew,
  dirty,
  saving,
  saveMsg,
  headersText,
  showAdvanced,
  testState,
  discover,
  onClose,
  onPatchProvider,
  onPatchModel,
  onAddModel,
  onDeleteModel,
  onUpdateCost,
  onHeadersChange,
  onToggleAdvanced,
  onSave,
  onTest,
  onResetTest,
  onFetchModels,
  onDiscoverSet,
  onDiscoverAddSelected,
  onDeleteProvider,
  lang,
}: {
  providerId: string;
  provider: ProviderTemplate;
  /** True when this modal is creating a new provider (id editable). */
  isNew: boolean;
  dirty: boolean;
  saving: boolean;
  saveMsg: { ok: boolean; text: string } | null;
  headersText: string;
  showAdvanced: boolean;
  testState: TestStateMap;
  discover: DiscoverState | null;
  onClose: () => void;
  onPatchProvider: (patch: Partial<ProviderTemplate>) => void;
  onPatchModel: (idx: number, patch: Partial<TemplateModelEntry>) => void;
  onAddModel: () => void;
  onDeleteModel: (idx: number) => void;
  onUpdateCost: (idx: number, field: keyof TemplateCost, val: string) => void;
  onHeadersChange: (v: string) => void;
  onToggleAdvanced: () => void;
  onSave: () => void;
  onTest: (modelId: string) => void;
  onResetTest: (providerId: string, modelId: string) => void;
  onFetchModels: () => void;
  onDiscoverSet: (d: DiscoverState | null) => void;
  onDiscoverAddSelected: () => void;
  onDeleteProvider: (() => void) | null;
  lang: Lang;
}): JSX.Element {
  const firstRef = useRef<HTMLInputElement>(null);
  useDialogChrome(onClose, firstRef);

  const [showKey, setShowKey] = useState(false);

  // Reset the test pills + the discover modal when switching providers.
  useEffect(() => {
    onDiscoverSet(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [providerId]);

  return (
    <div className="overlay open" role="presentation" onClick={onClose}>
      <div
        className="dialog provider-editor-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="pe-title"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="dialog-head">
          <h2 id="pe-title">
            {isNew ? t(lang, "mcAddProvider") : (provider.name || providerId)}
          </h2>
          <button
            className="icon-btn"
            aria-label={t(lang, "mcClose")}
            onClick={onClose}
          >
            <Icon name="x" />
          </button>
        </div>

        <div className="dialog-body">
          {/* name + id + protocol */}
          <div className="field-row-3">
            <div className="field">
              <label>{t(lang, "mcName")}</label>
              <input
                ref={firstRef}
                className="input"
                value={provider.name}
                placeholder={t(lang, "mcNamePlaceholder")}
                onChange={(e) => onPatchProvider({ name: e.target.value })}
              />
            </div>
            <div className="field">
              <label>{t(lang, "mcProviderId")}</label>
              <input
                className="input mono"
                value={providerId}
                disabled={!isNew}
                placeholder="provider-1"
                readOnly
              />
            </div>
            <div className="field">
              <label>{t(lang, "mcApi")}</label>
              <select
                className="input"
                value={provider.api}
                onChange={(e) => onPatchProvider({ api: e.target.value })}
              >
                {API_PROTOCOLS.map((p) => (
                  <option key={p} value={p}>
                    {p}
                  </option>
                ))}
              </select>
            </div>
          </div>

          {/* baseUrl + apiKey */}
          <div className="field-row">
            <div className="field">
              <label>{t(lang, "mcBaseUrl")}</label>
              <input
                className="input mono"
                value={provider.baseUrl}
                placeholder="https://api.example.com/v1"
                onChange={(e) => onPatchProvider({ baseUrl: e.target.value })}
              />
            </div>
            <div className="field">
              <label>{t(lang, "mcApiKey")}</label>
              <div className="input-group">
                <input
                  className="input mono"
                  type={showKey ? "text" : "password"}
                  value={provider.apiKey ?? ""}
                  autoComplete="off"
                  onChange={(e) =>
                    onPatchProvider({ apiKey: e.target.value || undefined })
                  }
                />
                <button
                  type="button"
                  className="icon-btn"
                  aria-label={showKey ? t(lang, "mcHideKey") : t(lang, "mcShowKey")}
                  onClick={() => setShowKey((v) => !v)}
                >
                  <Icon name={showKey ? "eye-off" : "eye"} />
                </button>
              </div>
            </div>
          </div>

          {/* website + category (cc-switch metadata) */}
          <div className="field-row">
            <div className="field">
              <label>{t(lang, "mcWebsiteUrl")}</label>
              <input
                className="input mono"
                value={provider.websiteUrl ?? ""}
                placeholder="https://example.com"
                onChange={(e) =>
                  onPatchProvider({ websiteUrl: e.target.value || undefined })
                }
              />
            </div>
            <div className="field">
              <label>{t(lang, "mcCategory")}</label>
              <input
                className="input"
                value={provider.category ?? ""}
                placeholder="official | aggregator | third_party | custom"
                onChange={(e) =>
                  onPatchProvider({ category: e.target.value || undefined })
                }
              />
            </div>
          </div>

          {/* advanced: extra headers (JSON) */}
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={onToggleAdvanced}
          >
            <Icon name={showAdvanced ? "chev-down" : "chev-r"} />
            {t(lang, "mcAdvanced")}
          </button>
          {showAdvanced && (
            <div className="field">
              <label>{t(lang, "mcHeadersJson")}</label>
              <textarea
                className="input mono"
                rows={4}
                value={headersText}
                spellCheck={false}
                onChange={(e) => onHeadersChange(e.target.value)}
              />
            </div>
          )}

          {/* model library */}
          <div className="ml-section-head">
            <span className="ml-section-title">
              {t(lang, "mcModels")} ({provider.models.length})
            </span>
            <div className="ml-section-actions">
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={onFetchModels}
                disabled={!provider.baseUrl}
                title={!provider.baseUrl ? t(lang, "mcNeedBaseUrl") : undefined}
              >
                <Icon name="refresh" />
                {t(lang, "mcFetchModels")}
              </button>
              <button type="button" className="btn btn-ghost btn-sm" onClick={onAddModel}>
                <Icon name="plus" />
                {t(lang, "mcAddModel")}
              </button>
            </div>
          </div>
          {provider.models.length === 0 && (
            <p className="muted txs">{t(lang, "mcNoModelsYet")}</p>
          )}
          {provider.models.map((m, idx) => (
            <ModelRow
              key={`${idx}-${m.id}`}
              providerId={providerId}
              model={m}
              idx={idx}
              testState={testState}
              onPatchModel={onPatchModel}
              onDeleteModel={onDeleteModel}
              onUpdateCost={onUpdateCost}
              onTest={onTest}
              onResetTest={onResetTest}
              lang={lang}
            />
          ))}
        </div>

        <div className="dialog-actions">
          {onDeleteProvider && (
            <button
              type="button"
              className="btn btn-danger"
              onClick={onDeleteProvider}
            >
              <Icon name="trash" />
              {t(lang, "mcDeleteProvider")}
            </button>
          )}
          <span className="flex-spacer" />
          {saveMsg && (
            <span className={`ml-msg${saveMsg.ok ? "" : " err"}`}>{saveMsg.text}</span>
          )}
          <button type="button" className="btn btn-secondary" onClick={onClose}>
            {t(lang, "cancel")}
          </button>
          <button
            type="button"
            className="btn btn-primary"
            disabled={!dirty || saving}
            onClick={onSave}
          >
            {saving ? t(lang, "mcSaving") : t(lang, "save")}
          </button>
        </div>
      </div>

      {/* discover overlay (shared component, R5) */}
      {discover && (
        <DiscoverModelsDialog
          discover={discover}
          onSet={onDiscoverSet}
          onConfirm={onDiscoverAddSelected}
          lang={lang}
        />
      )}
    </div>
  );
}
