// ModelRow — single-model collapsed/expanded row (cc-switch refactor,
// task 10-04-model-config-cc-switch Phase 3). Ported from the pre-redesign
// ModelRow with the catalog-fill button dropped (the models.dev proxy is
// gone — prd D6): chevron + editable mono id + name + test pill + delete
// collapsed; full field editor expanded. All edits patch library state
// through ModelsPage callbacks.

import { useState } from "react";
import { Icon } from "../../icons";
import { t, type Lang } from "../../i18n";
import {
  type TemplateCost,
  type TemplateModelEntry,
  type TestStateMap,
} from "./types";

export function ModelRow({
  providerId,
  model,
  idx,
  testState,
  onPatchModel,
  onDeleteModel,
  onUpdateCost,
  onTest,
  onResetTest,
  lang,
}: {
  providerId: string;
  model: TemplateModelEntry;
  idx: number;
  testState: TestStateMap;
  onPatchModel: (idx: number, patch: Partial<TemplateModelEntry>) => void;
  onDeleteModel: (idx: number) => void;
  onUpdateCost: (idx: number, field: keyof TemplateCost, val: string) => void;
  onTest: (modelId: string) => void;
  onResetTest: (providerId: string, modelId: string) => void;
  lang: Lang;
}): JSX.Element {
  const [expanded, setExpanded] = useState(false);
  const ts = testState[`${providerId}:${model.id}`];

  return (
    <div className="ml-model-row">
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
        <div className="ml-model-row-actions">
          <button
            className={`ml-test-pill${ts?.status === "ok" ? " ok" : ts?.status === "fail" ? " fail" : ""}`}
            onClick={() => onTest(model.id)}
            disabled={!model.id || ts?.status === "testing"}
            title={ts?.status === "fail" ? (ts.error ?? undefined) : undefined}
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
              ? ts.latencyMs != null
                ? `${ts.latencyMs}ms`
                : t(lang, "mcTestOk")
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
          {/* name */}
          <div className="field">
            <label>{t(lang, "mcModelName")}</label>
            <input
              className="input"
              value={model.name ?? ""}
              onChange={(e) =>
                onPatchModel(idx, { name: e.target.value || undefined })
              }
            />
          </div>

          {/* ctx / maxOut / reasoning */}
          <div className="field-row-3">
            <div className="field">
              <label>{t(lang, "mcContextWindow")}</label>
              <input
                className="input mono"
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
                className="input mono"
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

          {/* cost per M */}
          <span className="ml-section-title">{t(lang, "mcCostPerM")}</span>
          <div className="ml-model-cost-grid">
            <div className="field">
              <label>in</label>
              <input
                className="input mono"
                type="number"
                step="any"
                value={model.cost?.input ?? ""}
                onChange={(e) => onUpdateCost(idx, "input", e.target.value)}
              />
            </div>
            <div className="field">
              <label>out</label>
              <input
                className="input mono"
                type="number"
                step="any"
                value={model.cost?.output ?? ""}
                onChange={(e) => onUpdateCost(idx, "output", e.target.value)}
              />
            </div>
            <div className="field">
              <label>{t(lang, "mcUsageColCacheR")}</label>
              <input
                className="input mono"
                type="number"
                step="any"
                value={model.cost?.cacheRead ?? ""}
                onChange={(e) => onUpdateCost(idx, "cacheRead", e.target.value)}
              />
            </div>
            <div className="field">
              <label>{t(lang, "mcUsageColCacheW")}</label>
              <input
                className="input mono"
                type="number"
                step="any"
                value={model.cost?.cacheWrite ?? ""}
                onChange={(e) => onUpdateCost(idx, "cacheWrite", e.target.value)}
              />
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
