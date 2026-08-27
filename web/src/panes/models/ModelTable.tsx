// ModelTable — per-provider model library editor.
//
// Columns: id (mono), per-model protocol override (empty = inherit the
// provider's protocol), display name, reasoning, context window, max output,
// cost in/out/cacheR/cacheW, availability test pill, delete. Test pill state
// is keyed by `${providerId}:${modelId}` and resets when a row's id or the
// provider's identity fields change (ModelsPane owns that effect).

import { Icon } from "../../icons";
import { t, type Lang } from "../../i18n";
import { API_PROTOCOLS, type CostEntry, type ModelEntry, type TestStateMap } from "./types";

export function ModelTable({
  providerId,
  models,
  testState,
  onPatchModel,
  onDeleteModel,
  onUpdateCost,
  onTest,
  onResetTest,
  lang,
}: {
  providerId: string;
  models: ModelEntry[];
  testState: TestStateMap;
  onPatchModel: (idx: number, patch: Partial<ModelEntry>) => void;
  onDeleteModel: (idx: number) => void;
  onUpdateCost: (idx: number, field: keyof CostEntry, val: string) => void;
  onTest: (modelId: string) => void;
  onResetTest: (providerId: string, modelId: string) => void;
  lang: Lang;
}): JSX.Element {
  return (
    <table className="ml-table">
      <thead>
        <tr>
          <th>{t(lang, "mcModelId")}</th>
          <th>{t(lang, "mcApi")}</th>
          <th>{t(lang, "mcModelName")}</th>
          <th className="ml-chk">{t(lang, "mcReasoning")}</th>
          <th>{t(lang, "mcContextWindow")}</th>
          <th>{t(lang, "mcMaxTokens")}</th>
          <th>{t(lang, "mcCost")} →</th>
          <th>in</th>
          <th>out</th>
          <th>cacheR</th>
          <th>cacheW</th>
          <th>{t(lang, "mcTest")}</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        {models.map((m, idx) => {
          const ts = testState[`${providerId}:${m.id}`];
          return (
            <tr key={idx}>
              <td>
                <input
                  value={m.id}
                  className="ml-cell-mono"
                  onChange={(e) => {
                    onResetTest(providerId, m.id);
                    onPatchModel(idx, { id: e.target.value });
                  }}
                />
              </td>
              <td>
                <select
                  className="ml-cell-select"
                  value={m.api ?? ""}
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
              </td>
              <td>
                <input
                  value={m.name ?? ""}
                  onChange={(e) =>
                    onPatchModel(idx, { name: e.target.value || undefined })
                  }
                />
              </td>
              <td className="ml-chk">
                <input
                  type="checkbox"
                  checked={m.reasoning ?? false}
                  onChange={(e) => onPatchModel(idx, { reasoning: e.target.checked })}
                />
              </td>
              <td>
                <input
                  type="number"
                  value={m.contextWindow ?? ""}
                  onChange={(e) =>
                    onPatchModel(idx, {
                      contextWindow: e.target.value
                        ? parseInt(e.target.value, 10)
                        : undefined,
                    })
                  }
                />
              </td>
              <td>
                <input
                  type="number"
                  value={m.maxTokens ?? ""}
                  onChange={(e) =>
                    onPatchModel(idx, {
                      maxTokens: e.target.value
                        ? parseInt(e.target.value, 10)
                        : undefined,
                    })
                  }
                />
              </td>
              <td className="ml-cost-sep"></td>
              <td>
                <input
                  type="number"
                  step="any"
                  value={m.cost?.input ?? ""}
                  onChange={(e) => onUpdateCost(idx, "input", e.target.value)}
                />
              </td>
              <td>
                <input
                  type="number"
                  step="any"
                  value={m.cost?.output ?? ""}
                  onChange={(e) => onUpdateCost(idx, "output", e.target.value)}
                />
              </td>
              <td>
                <input
                  type="number"
                  step="any"
                  value={m.cost?.cacheRead ?? ""}
                  onChange={(e) => onUpdateCost(idx, "cacheRead", e.target.value)}
                />
              </td>
              <td>
                <input
                  type="number"
                  step="any"
                  value={m.cost?.cacheWrite ?? ""}
                  onChange={(e) => onUpdateCost(idx, "cacheWrite", e.target.value)}
                />
              </td>
              <td>
                <div className="ml-test-cell">
                  <button
                    className="btn btn-secondary ml-sm"
                    onClick={() => onTest(m.id)}
                    disabled={!m.id || ts?.status === "testing"}
                  >
                    {ts?.status === "testing"
                      ? t(lang, "mcTesting")
                      : t(lang, "mcTest")}
                  </button>
                  {ts && ts.status !== "idle" && (
                    <span
                      className={`ml-pill ml-pill-${ts.status}`}
                      title={
                        ts.status === "ok"
                          ? `HTTP ${ts.statusHttp ?? "?"} · ${ts.responseText ?? ""}`
                          : ts.status === "fail"
                            ? ts.error ?? ""
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
                </div>
              </td>
              <td>
                <button
                  className="icon-btn ml-cell-del"
                  aria-label={t(lang, "mcDeleteProvider")}
                  onClick={() => onDeleteModel(idx)}
                >
                  <Icon name="x" />
                </button>
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
