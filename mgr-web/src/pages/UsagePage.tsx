// UsagePage — multi-sandbox usage view over GET /api/usage (Phase 4c).
//
// Data source is mgr's fan-out shape {sandboxes: [{name, error, usage}]}
// where `usage` is each sandbox app's /api/models/usage payload verbatim
// (mgr/src/usage.rs). Because every row is a per-(agent, provider?, model)
// aggregate with purely additive numeric fields, rows from different
// sandboxes concatenate safely: the "全部合计" view just sums them (the
// cache-hit rate is recomputed token-weighted per row's agent convention —
// cacheHitDenom — so cross-sandbox mixing stays correct), and the detail
// table gains a sandbox column. Per-sandbox rows never merge across sandboxes
// within a row: each keeps its own line.
//
// Summary cards / bar chart / cost donut are ported from the workbench
// UsageTab; a sandbox whose probe failed renders an error card, never a
// zero-filled table (usage is null on the wire for it).

import { useCallback, useEffect, useMemo, useState } from "react";
import { getUsage } from "../api";
import { t, type Lang } from "../i18n";
import { Icon } from "../icons";
import { CostDonut, TokenBars, type ChartItem } from "./models/charts";
import {
  cacheHitDenom,
  cacheHitRate,
  fmtCost,
  fmtPct,
  fmtTokens,
  type SandboxUsageEntry,
  type UsageRow,
} from "./models/types";

type UsageWindow = "today" | "7d" | "all";

const WINDOWS: { key: UsageWindow; label: string }[] = [
  { key: "today", label: "mcUsageToday" },
  { key: "7d", label: "mcUsage7d" },
  { key: "all", label: "mcUsageAll" },
];

/** mgr caches each (sandbox, window) probe for 30s — a poll at that period is
 * free on the sandbox side while keeping the page live. */
const POLL_MS = 30000;

/** A usage row tagged with its source sandbox (the "all" view's table). */
interface TaggedRow extends UsageRow {
  sandbox: string;
}

export function UsagePage({ lang }: { lang: Lang }): JSX.Element {
  const [window, setWindow] = useState<UsageWindow>("today");
  const [entries, setEntries] = useState<SandboxUsageEntry[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [selected, setSelected] = useState(""); // "" = all sandboxes

  const fetchUsage = useCallback(async (w: UsageWindow): Promise<void> => {
    // `loading` only disables the refresh button — the in-page spinner is
    // gated on entries === null instead, so background polls never flicker.
    setLoading(true);
    try {
      const r = await getUsage(w);
      setEntries(r.sandboxes);
      setError("");
      // Drop a selection that no longer exists (sandbox deleted/stopped).
      setSelected((sel) =>
        sel === "" || r.sandboxes.some((e) => e.name === sel) ? sel : "",
      );
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    setEntries(null); // window change invalidates the previous rows
    void fetchUsage(window);
    const timer = setInterval(() => void fetchUsage(window), POLL_MS);
    return () => clearInterval(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [window]);

  const current = selected === "" ? null : entries?.find((e) => e.name === selected) ?? null;

  // Rows for the active view: all sandboxes concatenated, or one sandbox's.
  const rows: TaggedRow[] = useMemo(() => {
    const out: TaggedRow[] = [];
    for (const e of entries ?? []) {
      if (selected !== "" && e.name !== selected) continue;
      for (const r of e.usage?.rows ?? []) out.push({ ...r, sandbox: e.name });
    }
    return out;
  }, [entries, selected]);

  // Latest generatedAt across the included sandboxes (ISO-8601 Zulu strings
  // compare lexicographically).
  const generatedAt = useMemo(() => {
    let max = "";
    for (const e of entries ?? []) {
      if (selected !== "" && e.name !== selected) continue;
      const g = e.usage?.generatedAt ?? "";
      if (g > max) max = g;
    }
    return max;
  }, [entries, selected]);

  const s = summarize(rows);

  return (
    <div className="page">
      <div className="page-head">
        <h1>{t(lang, "navUsage")}</h1>
        <div className="page-actions">
          <div className="ml-window-switch" role="group" aria-label="time window">
            {WINDOWS.map((w) => (
              <button
                key={w.key}
                aria-pressed={window === w.key}
                onClick={() => setWindow(w.key)}
              >
                {t(lang, w.label)}
              </button>
            ))}
          </div>
          <button
            className="icon-btn"
            disabled={loading}
            aria-label={t(lang, "refresh")}
            title={t(lang, "refresh")}
            onClick={() => void fetchUsage(window)}
          >
            <Icon name="refresh" />
          </button>
        </div>
        <p className="sub">{t(lang, "usageSub")}</p>
      </div>

      {error && <div className="status error">{t(lang, "loadFailed")}{error}</div>}

      {/* sandbox selector — a chip per sandbox + the combined view; erroring
       * sandboxes carry a warn dot with the message in the title. */}
      {entries !== null && entries.length > 0 && (
        <div className="mu-sbx-row">
          <button
            className={`mu-sbx-chip${selected === "" ? " is-selected" : ""}`}
            aria-pressed={selected === ""}
            onClick={() => setSelected("")}
          >
            {t(lang, "muAll")}
          </button>
          {entries.map((e) => (
            <button
              key={e.name}
              className={`mu-sbx-chip${selected === e.name ? " is-selected" : ""}${e.error ? " is-error" : ""}`}
              aria-pressed={selected === e.name}
              title={e.error ?? e.name}
              onClick={() => setSelected(e.name)}
            >
              {e.error && <span className="dot" />}
              {e.name}
            </button>
          ))}
          {generatedAt && (
            <span className="ml-usage-gen">
              {t(lang, "mcUsageGeneratedAt")} {generatedAt}
            </span>
          )}
        </div>
      )}

      {entries === null && !error ? (
        <div className="ml-loading">{t(lang, "mcLoading")}</div>
      ) : entries !== null && entries.length === 0 ? (
        <div className="ml-empty">
          <p>{t(lang, "muNoSandboxes")}</p>
        </div>
      ) : current !== null && current.error !== null ? (
        /* selected sandbox failed its probe: error card, not a zero table */
        <div className="ml-error">
          {t(lang, "muSandboxErr")}
          {current.error}
        </div>
      ) : rows.length === 0 ? (
        <div className="ml-empty">
          <p>{t(lang, "mcUsageEmpty")}</p>
        </div>
      ) : (
        <>
          {/* summary cards */}
          <div className="ml-stats">
            {card(t(lang, "mcUsageColIn"), fmtTokens(s.totalIn))}
            {card(t(lang, "mcUsageColOut"), fmtTokens(s.totalOut))}
            {card(
              t(lang, "mcUsageCacheHit"),
              s.cacheHit != null ? fmtPct(s.cacheHit) : "—",
            )}
            {s.hasCost
              ? card(t(lang, "mcUsageColCost"), fmtCost(s.totalCost))
              : card(
                  t(lang, "mcUsageTotalTokens"),
                  fmtTokens(s.totalIn + s.totalOut + s.totalCacheR + s.totalCacheW),
                )}
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
            {s.hasCostValue && s.costItems.length > 0 && (
              <div className="ml-chart-card">
                <h3 className="ml-chart-title">{t(lang, "mcUsageCostShare")}</h3>
                <CostDonut items={s.costItems} total={s.totalCost} />
              </div>
            )}
          </div>

          {/* detail table (sandbox column only in the combined view — the
           * per-sandbox view already names it in the selector) */}
          <div className="ml-chart-card ml-usage-table-card">
            <div className="ml-table-scroll">
              <table className="ml-table ml-usage-table">
                <thead>
                  <tr>
                    {selected === "" && <th>{t(lang, "muColSandbox")}</th>}
                    <th>{t(lang, "mcUsageColAgent")}</th>
                    <th>{t(lang, "mcUsageColProvider")}</th>
                    <th>{t(lang, "mcUsageColModel")}</th>
                    <th className="ml-num">{t(lang, "mcUsageColIn")}</th>
                    <th className="ml-num">{t(lang, "mcUsageColOut")}</th>
                    <th className="ml-num">{t(lang, "mcUsageCacheHit")}</th>
                    {s.hasCost && (
                      <th className="ml-num">{t(lang, "mcUsageColCost")}</th>
                    )}
                  </tr>
                </thead>
                <tbody>
                  {rows.map((r, i) => (
                    <tr key={i}>
                      {selected === "" && <td className="ml-cell-clip">{r.sandbox}</td>}
                      <td>{r.agent}</td>
                      <td className="ml-cell-clip" title={r.provider ?? undefined}>
                        {r.provider ?? "—"}
                      </td>
                      <td className="ml-cell-clip ml-cell-mono" title={r.model}>
                        {r.model}
                      </td>
                      <td className="ml-num">{fmtTokens(r.in)}</td>
                      <td className="ml-num">{fmtTokens(r.out)}</td>
                      <td className="ml-num">
                        {(() => {
                          const hr = cacheHitRate(r);
                          return hr != null ? fmtPct(hr) : "—";
                        })()}
                      </td>
                      {s.hasCost && (
                        <td className="ml-num">
                          {r.cost !== undefined ? fmtCost(r.cost) : "—"}
                        </td>
                      )}
                    </tr>
                  ))}
                  <tr className="ml-table-total">
                    <td colSpan={selected === "" ? 4 : 3}>{t(lang, "mcUsageTotal")}</td>
                    <td className="ml-num">{fmtTokens(s.totalIn)}</td>
                    <td className="ml-num">{fmtTokens(s.totalOut)}</td>
                    <td className="ml-num">
                      {s.cacheHit != null ? fmtPct(s.cacheHit) : "—"}
                    </td>
                    {s.hasCost && (
                      <td className="ml-num">{fmtCost(s.totalCost)}</td>
                    )}
                  </tr>
                </tbody>
              </table>
            </div>
          </div>
        </>
      )}
    </div>
  );
}

function card(label: string, value: string): JSX.Element {
  return (
    <div className="ml-stat">
      <span className="ml-stat-label">{label}</span>
      <span className="ml-stat-value">{value}</span>
    </div>
  );
}

/** Aggregate per-model tokens and per-agent cost from the (tagged) rows.
 * Ported from the workbench UsageTab's summarize — identical math, the
 * sandbox tag only rides along untouched. */
function summarize(rows: TaggedRow[]) {
  const totalIn = rows.reduce((a, r) => a + r.in, 0);
  const totalOut = rows.reduce((a, r) => a + r.out, 0);
  const totalCacheR = rows.reduce((a, r) => a + r.cacheRead, 0);
  const totalCacheW = rows.reduce((a, r) => a + r.cacheWrite, 0);
  // hasCost: some row carries a cost (log or backfilled) -> show the cost
  // column. hasCostValue: some cost is actually > 0 -> show the donut (an
  // all-zero donut is noise).
  const hasCost = rows.some((r) => r.cost !== undefined);
  const hasCostValue = rows.some((r) => (r.cost ?? 0) > 0);
  const totalCost = hasCost ? rows.reduce((a, r) => a + (r.cost ?? 0), 0) : 0;

  // Overall cache hit rate: token-weighted across rows (each row's cache
  // reads over its agent-convention input denominator — cacheHitDenom).
  const hitReads = rows.reduce((a, r) => a + r.cacheRead, 0);
  const hitDenoms = rows.reduce((a, r) => a + cacheHitDenom(r), 0);
  const cacheHit = hitReads > 0 ? hitReads / hitDenoms : null;

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
    totalCacheR,
    totalCacheW,
    totalCost,
    cacheHit,
    hasCost,
    hasCostValue,
    modelItems,
    costItems,
  };
}
