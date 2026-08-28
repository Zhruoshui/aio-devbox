// ModelRow — single-model collapsed/expanded row (Kumo redesign).
//
// Collapsed (screens_model-config.html §model-row): chevron + editable mono id
// (reads as text until focused) + name + cost summary `$in / $out` + a
// .test-pill (play → spin → ok·ms / fail) + delete. The reasoning flag is
// intentionally NOT shown in the collapsed row (08-28 feedback: visual noise;
// still editable via the expanded form and set by catalog fill).
// Expanded: name + protocol-override side by side, then context-window /
// max-output / reasoning-check three-across, then the four cost fields
// (in/out/cacheRead/cacheWrite), then a "fill from models.dev" ghost button.
// All edits patch canonical state through ModelsPane callbacks.

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
  const hasCost =
    model.cost && (model.cost.input != null || model.cost.output != null);
  const costSummary = hasCost
    ? `$${model.cost?.input != null ? model.cost.input.toFixed(2) : "—"} / ${
        model.cost?.output != null ? model.cost.output.toFixed(2) : "—"
      }`
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
        {costSummary && <span className="ml-model-cost-summary">{costSummary}</span>}
        <div className="ml-model-row-actions">
          <button
            className={`ml-test-pill${ts?.status === "ok" ? " ok" : ts?.status === "fail" ? " fail" : ""}`}
            onClick={() => onTest(model.id)}
            disabled={!model.id || ts?.status === "testing"}
            title={
              ts?.status === "fail" ? (ts.error ?? undefined) : undefined
            }
          >
            {ts?.status === "testing" ? (
              <span className="spin" aria-hidden="true" />
            ) : ts?.status === "ok" ? (
              <Icon name="check" />
            ) : ts?.status === "fail" ? (
              <Icon name="x" />
            ) : (
              <Icon name="play" />
            )}
            {ts?.status === "ok"
              ? `${t(lang, "mcTestOk")} · ${ts.latencyMs ?? "?"}ms`
              : ts?.status === "fail"
                ? t(lang, "mcTestFail")
                : t(lang, "mcTest")}
          </button>
          <button
            className="icon-btn ml-cell-del"
            aria-label={t(lang, "mcDeleteProvider")}
            onClick={() => onDeleteModel(idx)}
          >
            <Icon name="trash" />
          </button>
        </div>
      </div>

      {expanded && (
        <div className="ml-model-row-body">
          {/* name + protocol override side by side (design §field-row) */}
          <div className="field-row">
            <div className="field">
              <label>{t(lang, "mcModelName")}</label>
              <input
                value={model.name ?? ""}
                onChange={(e) =>
                  onPatchModel(idx, { name: e.target.value || undefined })
                }
              />
            </div>
            <div className="field">
              <label>{t(lang, "mcApi")}</label>
              <select
                className="ml-cell-select"
                value={model.api ?? ""}
                onChange={(e) =>
                  onPatchModel(idx, { api: e.target.value || undefined })
                }
              >
                <option value="">{t(lang, "mcApiInherit")}</option>
                {API_PROTOCOLS.map((p) => (
                  <option key={p} value={p}>
                    {p}
                  </option>
                ))}
              </select>
            </div>
          </div>

          {/* ctx / maxOut / reasoning (design §field-row-3) */}
          <div className="field-row-3">
            <div className="field">
              <label>{t(lang, "mcContextWindow")}</label>
              <input
                className="mono"
                type="number"
                value={model.contextWindow ?? ""}
                onChange={(e) =>
                  onPatchModel(idx, {
                    contextWindow: e.target.value
                      ? parseInt(e.target.value, 10)
                      : undefined,
                  })
                }
              />
            </div>
            <div className="field">
              <label>{t(lang, "mcMaxTokens")}</label>
              <input
                className="mono"
                type="number"
                value={model.maxTokens ?? ""}
                onChange={(e) =>
                  onPatchModel(idx, {
                    maxTokens: e.target.value
                      ? parseInt(e.target.value, 10)
                      : undefined,
                  })
                }
              />
            </div>
            <div className="field">
              <label className="check-inline" style={{ marginTop: 18 }}>
                <input
                  type="checkbox"
                  checked={model.reasoning ?? false}
                  onChange={(e) =>
                    onPatchModel(idx, { reasoning: e.target.checked })
                  }
                />
                {t(lang, "mcReasoning")}
              </label>
            </div>
          </div>

          {/* cost per M (design §field-row-4) */}
          <span className="ml-section-title">{t(lang, "mcCostPerM")}</span>
          <div className="ml-model-cost-grid">
            <div className="field">
              <label>in</label>
              <input
                className="mono"
                type="number"
                step="any"
                value={model.cost?.input ?? ""}
                onChange={(e) => onUpdateCost(idx, "input", e.target.value)}
              />
            </div>
            <div className="field">
              <label>out</label>
              <input
                className="mono"
                type="number"
                step="any"
                value={model.cost?.output ?? ""}
                onChange={(e) => onUpdateCost(idx, "output", e.target.value)}
              />
            </div>
            <div className="field">
              <label>{t(lang, "mcUsageColCacheR")}</label>
              <input
                className="mono"
                type="number"
                step="any"
                value={model.cost?.cacheRead ?? ""}
                onChange={(e) => onUpdateCost(idx, "cacheRead", e.target.value)}
              />
            </div>
            <div className="field">
              <label>{t(lang, "mcUsageColCacheW")}</label>
              <input
                className="mono"
                type="number"
                step="any"
                value={model.cost?.cacheWrite ?? ""}
                onChange={(e) => onUpdateCost(idx, "cacheWrite", e.target.value)}
              />
            </div>
          </div>

          <button
            className="btn btn-ghost btn-sm"
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
            <Icon name="download" />
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
