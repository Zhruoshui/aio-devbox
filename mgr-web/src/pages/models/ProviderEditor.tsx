// ProviderEditor — right-side sheet drawer editing one provider, redesigned
// per the 09-11 prototype (docs/Web-Prototype/models.html .drawer).
//
// Structure: viewport-fixed `.drawer > .scrim + .sheet[role=dialog]` with
// `.sheet-head` (title + close) / `.sheet-body` (name+protocol row, Base URL,
// API key with show/hide, collapsible advanced JSON, model library with
// discover + per-model test pills + catalog fill, binding overview) /
// `.sheet-foot` (delete | dirty/msg | cancel | save). All fields, the
// discover modal (.overlay) and every callback are carried over verbatim
// from the pre-redesign editor — only the chrome changed.
//
// Escape closes; the scrim click closes; the drawer opens on mount (the
// prototype's .drawer is display:none → .open block, no animation).

import { useEffect, useState } from "react";
import { Icon } from "../../icons";
import { fmt, t, type Lang } from "../../i18n";
import { ModelRow, type CatalogFillState } from "./ModelRow";
import {
  API_PROTOCOLS,
  bindingAgents,
  type AgentTab,
  type CanonicalConfig,
  type CostEntry,
  type DiscoverState,
  type ModelEntry,
  type ProviderEntry,
  type TestStateMap,
} from "./types";

const AGENT_LABEL: Record<AgentTab, string> = {
  pi: "pi",
  opencode: "opencode",
  claude: "Claude",
  codex: "Codex",
};

export function ProviderEditor({
  providerId,
  provider,
  config,
  dirty,
  saving,
  saveMsg,
  headersText,
  compatText,
  showAdvanced,
  testState,
  discover,
  catalogFillState,
  onClose,
  onPatchProvider,
  onPatchModel,
  onAddModel,
  onDeleteModel,
  onUpdateCost,
  onHeadersChange,
  onCompatChange,
  onToggleAdvanced,
  onSave,
  onTest,
  onResetTest,
  onFetchModels,
  onDiscoverSet,
  onDiscoverAddSelected,
  onFillFromCatalog,
  onJumpToAgent,
  onDeleteProvider,
  lang,
}: {
  providerId: string;
  provider: ProviderEntry;
  config: CanonicalConfig;
  dirty: boolean;
  saving: boolean;
  saveMsg: { ok: boolean; text: string } | null;
  headersText: string;
  compatText: string;
  showAdvanced: boolean;
  testState: TestStateMap;
  discover: DiscoverState | null;
  catalogFillState: Record<string, CatalogFillState>;
  onClose: () => void;
  onPatchProvider: (patch: Partial<ProviderEntry>) => void;
  onPatchModel: (idx: number, patch: Partial<ModelEntry>) => void;
  onAddModel: () => void;
  onDeleteModel: (idx: number) => void;
  onUpdateCost: (idx: number, field: keyof CostEntry, val: string) => void;
  onHeadersChange: (v: string) => void;
  onCompatChange: (v: string) => void;
  onToggleAdvanced: () => void;
  onSave: () => void;
  onTest: (modelId: string) => void;
  onResetTest: (providerId: string, modelId: string) => void;
  onFetchModels: () => void;
  onDiscoverSet: (d: DiscoverState | null) => void;
  onDiscoverAddSelected: () => void;
  onFillFromCatalog: (idx: number) => void;
  onJumpToAgent: (agent: AgentTab) => void;
  onDeleteProvider: () => void;
  lang: Lang;
}): JSX.Element {
  const [showKey, setShowKey] = useState(false);
  // Escape closes the drawer.
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);

  const bound = bindingAgents(config, providerId);

  return (
    <>
      <div className="drawer open">
        <div className="scrim" onClick={onClose} aria-hidden="true" />

        <div
          className="sheet"
          role="dialog"
          aria-modal="true"
          aria-label={provider.name || t(lang, "mcNewProvider")}
        >
          <div className="sheet-head">
            <h2>
              {provider.name
                ? `${t(lang, "mcEditProviderTitle")} · ${provider.name}`
                : t(lang, "mcNewProvider")}
            </h2>
            <button
              className="icon-btn"
              aria-label={t(lang, "mcClose")}
              title={t(lang, "mcClose")}
              onClick={onClose}
            >
              <Icon name="x" />
            </button>
          </div>

          <div className="sheet-body">
            {/* ── basic info ── */}
            <div className="field-row">
              <div className="field">
                <label>{t(lang, "mcName")}</label>
                <input
                  className="input"
                  value={provider.name}
                  onChange={(e) => onPatchProvider({ name: e.target.value })}
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
            <div className="field">
              <label>{t(lang, "mcBaseUrl")}</label>
              <input
                className="input mono"
                value={provider.baseUrl}
                onChange={(e) => onPatchProvider({ baseUrl: e.target.value })}
                placeholder="https://api.example.com/v1"
              />
            </div>
            <div className="field">
              <label>{t(lang, "mcApiKey")}</label>
              <div className="input-wrap ml-key-row">
                <input
                  className="input mono"
                  type={showKey ? "text" : "password"}
                  value={provider.apiKey ?? ""}
                  placeholder={t(lang, "mcApiKeyPh")}
                  onChange={(e) => onPatchProvider({ apiKey: e.target.value })}
                />
                <button
                  className="icon-btn"
                  aria-label={showKey ? t(lang, "mcHideKey") : t(lang, "mcShowKey")}
                  title={showKey ? t(lang, "mcHideKey") : t(lang, "mcShowKey")}
                  onClick={() => setShowKey((v) => !v)}
                >
                  <Icon name={showKey ? "eye-off" : "eye"} />
                </button>
              </div>
              <span className="hint">{t(lang, "mcKeyHint")}</span>
            </div>

            {/* ── advanced (collapsible) ── */}
            <div className="ml-dgroup">
              <button
                className="ml-dgroup-toggle"
                onClick={onToggleAdvanced}
                aria-expanded={showAdvanced}
              >
                <span className="ml-section-title" style={{ margin: 0 }}>
                  {t(lang, "mcAdvanced")}
                </span>
                <Icon name="chev-down" />
              </button>
              {showAdvanced && (
                <>
                  <div className="field">
                    <label>{t(lang, "mcHeaders")}</label>
                    <textarea
                      className="input ml-json-area"
                      value={headersText}
                      onChange={(e) => onHeadersChange(e.target.value)}
                      rows={4}
                      spellCheck={false}
                    />
                  </div>
                  <div className="field">
                    <label>{t(lang, "mcCompat")}</label>
                    <textarea
                      className="input ml-json-area"
                      value={compatText}
                      onChange={(e) => onCompatChange(e.target.value)}
                      rows={4}
                      spellCheck={false}
                    />
                  </div>
                </>
              )}
            </div>

            {/* ── models ── */}
            <div className="ml-dgroup">
              <div className="sec-head" style={{ marginBottom: "var(--space-2)" }}>
                <h3 className="tsm" style={{ margin: 0, fontWeight: 600 }}>
                  {t(lang, "mcModels")}
                </h3>
                <div className="sec-acts">
                  <button
                    className="btn btn-ghost btn-sm"
                    onClick={onFetchModels}
                    disabled={!provider.baseUrl}
                    title={t(lang, "mcFetchModelsTitle")}
                  >
                    {t(lang, "mcFetchModels")}
                  </button>
                  <button className="btn btn-ghost btn-sm" onClick={onAddModel}>
                    <Icon name="plus" />
                    {t(lang, "mcAddModel")}
                  </button>
                </div>
              </div>
              <div className="ml-model-list">
                {provider.models.length === 0 ? (
                  <span className="ml-hint">{t(lang, "mcDiscoverEmpty")}</span>
                ) : (
                  provider.models.map((m, idx) => (
                    <ModelRow
                      key={idx}
                      providerId={providerId}
                      model={m}
                      idx={idx}
                      testState={testState}
                      catalogFillState={catalogFillState[`${providerId}:${m.id}`]}
                      onPatchModel={onPatchModel}
                      onDeleteModel={onDeleteModel}
                      onUpdateCost={onUpdateCost}
                      onTest={onTest}
                      onResetTest={onResetTest}
                      onFillFromCatalog={onFillFromCatalog}
                      lang={lang}
                    />
                  ))
                )}
              </div>
            </div>

            {/* ── binding overview ── */}
            <div className="ml-dgroup">
              <h3 className="ml-section-title">{t(lang, "mcBoundAgents")}</h3>
              {bound.length === 0 ? (
                <span className="ml-hint">{t(lang, "mcNoBindings")}</span>
              ) : (
                <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
                  {bound.map((a) => (
                    <button
                      key={a}
                      className="ml-chip"
                      onClick={() => onJumpToAgent(a)}
                    >
                      {AGENT_LABEL[a]}
                    </button>
                  ))}
                </div>
              )}
            </div>
          </div>

          <div className="sheet-foot">
            <button
              className="icon-btn danger lg"
              aria-label={t(lang, "mcDeleteProvider")}
              title={t(lang, "mcDeleteProvider")}
              onClick={onDeleteProvider}
            >
              <Icon name="trash" />
            </button>
            {dirty && <span className="ml-dirty">{t(lang, "mcDirty")}</span>}
            {saveMsg && (
              <span className={`ml-msg${saveMsg.ok ? " ok" : " err"}`}>
                {saveMsg.text}
              </span>
            )}
            <button className="btn btn-secondary" onClick={onClose}>
              {t(lang, "cancel")}
            </button>
            <button
              className="btn btn-primary"
              disabled={!dirty || saving}
              onClick={onSave}
            >
              {saving ? <Icon name="refresh" /> : null}
              {saving ? t(lang, "mcSaving") : t(lang, "mcSave")}
            </button>
          </div>
        </div>
      </div>

      {/* ── discover modal ── */}
      {discover && (
        <div className="overlay open">
          <div className="dialog ml-discover">
            <div className="dialog-head">
              <div>
                <h2>{t(lang, "mcFetchModels")}</h2>
                <p className="endpoint mono">{discover.endpoint}</p>
              </div>
              <button
                className="icon-btn"
                title={t(lang, "mcClose")}
                onClick={() => onDiscoverSet(null)}
              >
                <Icon name="x" />
              </button>
            </div>
            <div className="ml-discover-body">
              {discover.loading ? (
                <div className="ml-loading">{t(lang, "mcLoading")}</div>
              ) : discover.error ? (
                <div className="ml-error">
                  {t(lang, "mcDiscoverFailed") + discover.error}
                </div>
              ) : (
                <>
                  {discover.models.length === 0 ? (
                    <div className="ml-hint">{t(lang, "mcDiscoverEmpty")}</div>
                  ) : (
                    <>
                      <div className="ml-discover-search-wrap">
                        <Icon name="search" />
                        <input
                          className="ml-discover-search"
                          placeholder={t(lang, "mcSearch")}
                          value={discover.filter}
                          onChange={(e) =>
                            onDiscoverSet({ ...discover, filter: e.target.value })
                          }
                        />
                      </div>
                      <div className="ml-discover-list">
                        {(() => {
                          const existing = new Set(
                            provider.models.map((m) => m.id),
                          );
                          const q = discover.filter.trim().toLowerCase();
                          const shown = discover.models.filter(
                            (m) =>
                              !q ||
                              m.id.toLowerCase().includes(q) ||
                              (m.name ?? "").toLowerCase().includes(q),
                          );
                          if (shown.length === 0) {
                            return <div className="ml-hint">—</div>;
                          }
                          return shown.map((m) => {
                            const already = existing.has(m.id);
                            const checked =
                              already || discover.selected.has(m.id);
                            return (
                              <label
                                key={m.id}
                                className={`ml-discover-item${already ? " is-existing" : ""}`}
                              >
                                <input
                                  type="checkbox"
                                  checked={checked}
                                  disabled={already}
                                  onChange={(e) => {
                                    const sel = new Set(discover.selected);
                                    if (e.target.checked) sel.add(m.id);
                                    else sel.delete(m.id);
                                    onDiscoverSet({ ...discover, selected: sel });
                                  }}
                                />
                                <span className="ml-discover-id">{m.id}</span>
                                {m.name && (
                                  <span className="ml-discover-name">{m.name}</span>
                                )}
                                {already && (
                                  <span className="ml-discover-tag">✓</span>
                                )}
                              </label>
                            );
                          });
                        })()}
                      </div>
                      <div className="ml-discover-actions">
                        <button
                          className="btn btn-ghost btn-sm"
                          onClick={() => {
                            const existing = new Set(
                              provider.models.map((m) => m.id),
                            );
                            const q = discover.filter.trim().toLowerCase();
                            const shown = discover.models.filter(
                              (m) =>
                                !q ||
                                m.id.toLowerCase().includes(q) ||
                                (m.name ?? "").toLowerCase().includes(q),
                            );
                            const sel = new Set(discover.selected);
                            for (const m of shown) {
                              if (!existing.has(m.id)) sel.add(m.id);
                            }
                            onDiscoverSet({ ...discover, selected: sel });
                          }}
                        >
                          {t(lang, "mcSelectAllPage")}
                        </button>
                        <button
                          className="btn btn-ghost btn-sm"
                          onClick={() =>
                            onDiscoverSet({ ...discover, selected: new Set() })
                          }
                        >
                          {t(lang, "mcClearSel")}
                        </button>
                        <span className="ml-discover-count">
                          {fmt(lang, "mcSelectedCount", discover.selected.size)}
                        </span>
                        <span className="spacer" />
                        <button
                          className="btn btn-secondary btn-sm"
                          onClick={() => onDiscoverSet(null)}
                        >
                          {t(lang, "cancel")}
                        </button>
                        <button
                          className="btn btn-primary btn-sm"
                          disabled={discover.selected.size === 0}
                          onClick={() => {
                            onDiscoverAddSelected();
                            onDiscoverSet(null);
                          }}
                        >
                          {t(lang, "mcAddSelected")}
                        </button>
                      </div>
                    </>
                  )}
                </>
              )}
            </div>
          </div>
        </div>
      )}
    </>
  );
}
