// ModelRow — single-model collapsed/expanded row (pi-web style), replaces the
// former ModelTable's flat 13-column table (08-27-provider-form-piweb design
// §3). Collapsed: id + name + reasoning badge + cost summary + test pill +
// expand/delete. Expanded: full field editor + per-model protocol override +
// "fill from models.dev" button.

import { useState } from "react";
import { Icon } from "../../icons";
import { t, type Lang } from "../../i18n";
import { API_PROTOCOLS, type CostEntry, type ModelEntry, type TestStateMap } from "./types";

export type CatalogFillState = "loading" | "notfound" | "error" | undefined;

export function ModelRow({
  providerId,
  model,
  idx,
  testState,
  catalogFillState,
  onPatchModel,
  onDeleteModel,
  onUpdateCost,
  onTest,
  onResetTest,
  onFillFromCatalog,
  lang,
}: {
  providerId: string;
  model: ModelEntry;
  idx: number;
  testState: TestStateMap;
  catalogFillState: CatalogFillState;
  onPatchModel: (idx: number, patch: Partial<ModelEntry>) => void;
  onDeleteModel: (idx: number) => void;
  onUpdateCost: (idx: number, field: keyof CostEntry, val: string) => void;
  onTest: (modelId: string) => void;
  onResetTest: (providerId: string, modelId: string) => void;
  onFillFromCatalog: (idx: number) => void;
  lang: Lang;
}): JSX.Element {
  const [expanded, setExpanded] = useState(false);
  const ts = testState[`${providerId}:${model.id}`];
  const costSummary =
    model.cost && (model.cost.input != null || model.cost.output != null)
      ? `${model.cost.input ?? "—"} / ${model.cost.output ?? "—"}`
      : null;

  return (
    <div className="ml-model-row" data-od-id={`model-row-${idx}`}>
      <div className="ml-model-row-head">
        <button
          className="icon-btn ml-model-expand"
          aria-label={expanded ? t(lang, "mcCollapse") : t(lang, "mcExpand")}
          onClick={() => setExpanded((v) => !v)}
        >
          <Icon name={expanded ? "chev-r" : "chev-l"} />
        </button>
        <input
          value={model.id}
          className="ml-cell-mono ml-model-id"
          placeholder={t(lang, "mcModelId")}
          onChange={(e) => {
            onResetTest(providerId, model.id);
            onPatchModel(idx, { id: e.target.value });
          }}
        />
        <span className="ml-model-name">{model.name || "—"}</span>
        {model.reasoning && (
          <span className="ml-badge">{t(lang, "mcReasoning")}</span>
        )}
        {costSummary && <span className="ml-model-cost-summary">{costSummary}</span>}
        <div className="ml-model-row-actions">
          <button
            className="btn btn-secondary ml-sm"
            onClick={() => onTest(model.id)}
            disabled={!model.id || ts?.status === "testing"}
          >
            {ts?.status === "testing" ? t(lang, "mcTesting") : t(lang, "mcTest")}
          </button>
          {ts && ts.status !== "idle" && (
            <span
              className={`ml-pill ml-pill-${ts.status}`}
              title={
                ts.status === "ok"
                  ? `HTTP ${ts.statusHttp ?? "?"} · ${ts.responseText ?? ""}`
                  : ts.status === "fail"
                    ? (ts.error ?? "")
                    : ""
              }
            >
              {ts.status === "ok"
                ? `${t(lang, "mcTestOk")} · ${ts.latencyMs ?? "?"}ms`
                : ts.status === "fail"
                  ? t(lang, "mcTestFail")
                  : "…"}
            </span>
          )}
          <button
            className="icon-btn ml-cell-del"
            aria-label={t(lang, "mcDeleteProvider")}
            onClick={() => onDeleteModel(idx)}
          >
            <Icon name="x" />
          </button>
        </div>
      </div>

      {expanded && (
        <div className="ml-model-row-body">
          <div className="field">
            <label>{t(lang, "mcApi")}</label>
            <select
              className="ml-cell-select"
              value={model.api ?? ""}
              onChange={(e) => onPatchModel(idx, { api: e.target.value || undefined })}
            >
              <option value="">{t(lang, "mcApiInherit")}</option>
              {API_PROTOCOLS.map((p) => (
                <option key={p} value={p}>
                  {p}
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <label>{t(lang, "mcModelName")}</label>
            <input
              value={model.name ?? ""}
              onChange={(e) => onPatchModel(idx, { name: e.target.value || undefined })}
            />
          </div>
          <div className="field ml-chk">
            <label>{t(lang, "mcReasoning")}</label>
            <input
              type="checkbox"
              checked={model.reasoning ?? false}
              onChange={(e) => onPatchModel(idx, { reasoning: e.target.checked })}
            />
          </div>
          <div className="field">
            <label>{t(lang, "mcContextWindow")}</label>
            <input
              type="number"
              value={model.contextWindow ?? ""}
              onChange={(e) =>
                onPatchModel(idx, {
                  contextWindow: e.target.value ? parseInt(e.target.value, 10) : undefined,
                })
              }
            />
          </div>
          <div className="field">
            <label>{t(lang, "mcMaxTokens")}</label>
            <input
              type="number"
              value={model.maxTokens ?? ""}
              onChange={(e) =>
                onPatchModel(idx, {
                  maxTokens: e.target.value ? parseInt(e.target.value, 10) : undefined,
                })
              }
            />
          </div>

          <div className="ml-section-title">{t(lang, "mcCostPerM")}</div>
          <div className="ml-model-cost-grid">
            <div className="field">
              <label>in</label>
              <input
                type="number"
                step="any"
                value={model.cost?.input ?? ""}
                onChange={(e) => onUpdateCost(idx, "input", e.target.value)}
              />
            </div>
            <div className="field">
              <label>out</label>
              <input
                type="number"
                step="any"
                value={model.cost?.output ?? ""}
                onChange={(e) => onUpdateCost(idx, "output", e.target.value)}
              />
            </div>
            <div className="field">
              <label>{t(lang, "mcUsageColCacheR")}</label>
              <input
                type="number"
                step="any"
                value={model.cost?.cacheRead ?? ""}
                onChange={(e) => onUpdateCost(idx, "cacheRead", e.target.value)}
              />
            </div>
            <div className="field">
              <label>{t(lang, "mcUsageColCacheW")}</label>
              <input
                type="number"
                step="any"
                value={model.cost?.cacheWrite ?? ""}
                onChange={(e) => onUpdateCost(idx, "cacheWrite", e.target.value)}
              />
            </div>
          </div>

          <button
            className="btn btn-secondary ml-sm"
            disabled={!model.id || catalogFillState === "loading"}
            title={
              catalogFillState === "notfound"
                ? t(lang, "mcCatalogNotFound")
                : catalogFillState === "error"
                  ? t(lang, "mcCatalogError")
                  : undefined
            }
            onClick={() => onFillFromCatalog(idx)}
          >
            {catalogFillState === "loading"
              ? t(lang, "mcLoading")
              : t(lang, "mcCatalogFill")}
          </button>
          {(catalogFillState === "notfound" || catalogFillState === "error") && (
            <span className="ml-hint ml-catalog-hint">
              {catalogFillState === "notfound"
                ? t(lang, "mcCatalogNotFound")
                : t(lang, "mcCatalogError")}
            </span>
          )}
        </div>
      )}
    </div>
  );
}
