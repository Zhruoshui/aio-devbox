// UsageTab — token/cost statistics for the usage tab.
//
// Pure frontend over the existing GET /api/models/usage aggregate rows: four
// summary cards, a horizontal bar chart of tokens by model, a cost-share
// donut by agent (hidden when no row carries a cost), and the Kumo-styled
// detail table with a 合计 footer. No new backend time-series (design §7).

import { Icon } from "../../icons";
import { t, type Lang } from "../../i18n";
import { fmtCost, fmtTokens, type UsageRow } from "./types";
import { CostDonut, TokenBars, type ChartItem } from "./charts";

export type UsageWindow = "today" | "7d" | "all";

const WINDOWS: { key: UsageWindow; label: string }[] = [
  { key: "today", label: "mcUsageToday" },
  { key: "7d", label: "mcUsage7d" },
  { key: "all", label: "mcUsageAll" },
];

/** Aggregate per-model tokens and per-agent cost from the rows. */
function summarize(rows: UsageRow[]) {
  const totalIn = rows.reduce((a, r) => a + r.in, 0);
  const totalOut = rows.reduce((a, r) => a + r.out, 0);
  const totalCache = rows.reduce((a, r) => a + r.cacheRead + r.cacheWrite, 0);
  const hasCost = rows.some((r) => r.cost !== undefined);
  const totalCost = hasCost ? rows.reduce((a, r) => a + (r.cost ?? 0), 0) : 0;

  // Tokens per model (in+out+cache), top 8, descending.
  const byModel = new Map<string, number>();
  for (const r of rows) {
    const key = r.model || r.agent;
    byModel.set(key, (byModel.get(key) ?? 0) + r.in + r.out + r.cacheRead + r.cacheWrite);
  }
  const modelItems: ChartItem[] = [...byModel.entries()]
    .map(([label, value]) => ({ label, value }))
    .sort((a, b) => b.value - a.value)
    .slice(0, 8);

  // Cost per agent (only rows with a real cost).
  const byAgentCost = new Map<string, number>();
  for (const r of rows) {
    if (r.cost !== undefined && r.cost > 0) {
      byAgentCost.set(r.agent, (byAgentCost.get(r.agent) ?? 0) + r.cost);
    }
  }
  const costItems: ChartItem[] = [...byAgentCost.entries()]
    .map(([label, value]) => ({ label, value }))
    .sort((a, b) => b.value - a.value);

  return {
    totalIn,
    totalOut,
    totalCache,
    totalCost,
    hasCost,
    modelItems,
    costItems,
  };
}

export function UsageTab({
  rows,
  generatedAt,
  window,
  loading,
  error,
  onWindowChange,
  onRefresh,
  lang,
}: {
  rows: UsageRow[] | null;
  generatedAt: string;
  window: UsageWindow;
  loading: boolean;
  error: string;
  onWindowChange: (w: UsageWindow) => void;
  onRefresh: () => void;
  lang: Lang;
}): JSX.Element {
  const all = rows ?? [];
  const s = summarize(all);

  const card = (label: string, value: string, note?: string): JSX.Element => (
    <div className="ml-stat">
      <span className="ml-stat-label">{label}</span>
      <span className="ml-stat-value">{value}</span>
      {note && <span className="ml-stat-note">{note}</span>}
    </div>
  );

  return (
    <div className="ml-usage">
      <div className="ml-usage-bar">
        {WINDOWS.map((w) => (
          <button
            key={w.key}
            className={`ml-tab ml-sm${window === w.key ? " active" : ""}`}
            onClick={() => onWindowChange(w.key)}
          >
            {t(lang, w.label)}
          </button>
        ))}
        <button
          className="btn btn-secondary ml-sm"
          disabled={loading}
          onClick={onRefresh}
        >
          {loading ? <Icon name="refresh" /> : null}
          {loading ? t(lang, "mcUsageRefreshing") : t(lang, "mcUsageRefresh")}
        </button>
        {generatedAt && (
          <span className="ml-usage-gen">
            {t(lang, "mcUsageGeneratedAt")} {generatedAt}
          </span>
        )}
      </div>

      {error && <div className="ml-error">{error}</div>}

      {loading && all.length === 0 ? (
        <div className="ml-loading">{t(lang, "mcLoading")}</div>
      ) : all.length === 0 ? (
        <div className="ml-empty">
          <p>{t(lang, "mcUsageEmpty")}</p>
        </div>
      ) : (
        <>
          {/* summary cards */}
          <div className="ml-stats">
            {card(t(lang, "mcUsageColIn"), fmtTokens(s.totalIn))}
            {card(t(lang, "mcUsageColOut"), fmtTokens(s.totalOut))}
            {card(t(lang, "mcUsageColCache"), fmtTokens(s.totalCache))}
            {s.hasCost
              ? card(t(lang, "mcUsageColCost"), fmtCost(s.totalCost))
              : card(t(lang, "mcUsageTotalTokens"), fmtTokens(s.totalIn + s.totalOut + s.totalCache))}
          </div>

          {/* charts */}
          <div className="ml-charts">
            {s.modelItems.length > 0 && (
              <div className="ml-chart-card">
                <h3 className="ml-chart-title">
                  {t(lang, "mcUsageByModel")}
                </h3>
                <TokenBars items={s.modelItems} />
              </div>
            )}
            {s.hasCost && s.costItems.length > 0 && (
              <div className="ml-chart-card">
                <h3 className="ml-chart-title">{t(lang, "mcUsageCostShare")}</h3>
                <CostDonut items={s.costItems} total={s.totalCost} />
              </div>
            )}
          </div>

          {/* detail table */}
          <div className="ml-chart-card ml-usage-table-card">
            <table className="ml-table">
              <thead>
                <tr>
                  <th>{t(lang, "mcUsageColAgent")}</th>
                  <th>{t(lang, "mcUsageColProvider")}</th>
                  <th>{t(lang, "mcUsageColModel")}</th>
                  <th className="ml-num">{t(lang, "mcUsageColIn")}</th>
                  <th className="ml-num">{t(lang, "mcUsageColOut")}</th>
                  <th className="ml-num">{t(lang, "mcUsageColCache")}</th>
                  {s.hasCost && (
                    <th className="ml-num">{t(lang, "mcUsageColCost")}</th>
                  )}
                </tr>
              </thead>
              <tbody>
                {all.map((r, i) => {
                  const cacheTotal = r.cacheRead + r.cacheWrite;
                  return (
                    <tr key={i}>
                      <td>{r.agent}</td>
                      <td>{r.provider ?? "—"}</td>
                      <td>{r.model}</td>
                      <td className="ml-num">{fmtTokens(r.in)}</td>
                      <td className="ml-num">{fmtTokens(r.out)}</td>
                      <td
                        className="ml-num"
                        title={`cacheRead=${r.cacheRead} cacheWrite=${r.cacheWrite}`}
                      >
                        {fmtTokens(cacheTotal)}
                      </td>
                      {s.hasCost && (
                        <td className="ml-num">
                          {r.cost !== undefined ? fmtCost(r.cost) : "—"}
                        </td>
                      )}
                    </tr>
                  );
                })}
                <tr className="ml-table-total">
                  <td colSpan={3}>{t(lang, "mcUsageTotal")}</td>
                  <td className="ml-num">{fmtTokens(s.totalIn)}</td>
                  <td className="ml-num">{fmtTokens(s.totalOut)}</td>
                  <td className="ml-num">{fmtTokens(s.totalCache)}</td>
                  {s.hasCost && (
                    <td className="ml-num">{fmtCost(s.totalCost)}</td>
                  )}
                </tr>
              </tbody>
            </table>
          </div>
        </>
      )}
    </div>
  );
}
