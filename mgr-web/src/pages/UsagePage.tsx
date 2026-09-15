// UsagePage — multi-sandbox usage view over GET /api/usage (Phase 4c).
//
// S7 (prototype redesign): window switch -> .segmented, sandbox selector
// chips -> the shared .chip (error dot -> .sdot.warn), day filter select ->
// .input.sm; the chart/stat classes (.ml-*/.mu-*) stay (real chart CSS, not
// in the prototype's component layer).
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
import {
  CostDonut,
  DayTrend,
  SandboxBars,
  TokenBars,
  type ChartItem,
  type DayTrendItem,
  type SandboxBarItem,
} from "./models/charts";
import {
  cacheHitDenom,
  cacheHitRate,
  fmtCost,
  fmtPct,
  fmtTokens,
  type DayUsage,
  type SandboxTotal,
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
  const [totals, setTotals] = useState<SandboxTotal[] | undefined>(undefined);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [selected, setSelected] = useState(""); // "" = all sandboxes
  // S4: day filter for the detail table — "" = all days ("全部"), else a
  // "YYYY-MM-DD" date. Independent of the window switch (R4, AC3).
  const [dayFilter, setDayFilter] = useState("");

  const fetchUsage = useCallback(async (w: UsageWindow): Promise<void> => {
    // `loading` only disables the refresh button — the in-page spinner is
    // gated on entries === null instead, so background polls never flicker.
    setLoading(true);
    try {
      const r = await getUsage(w);
      setEntries(r.sandboxes);
      setTotals(r.totals);
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

  // S4: cross-sandbox bar items for the combined view (R1/AC1). Use mgr's
  // totals when present; fall back to deriving from entries (old mgr).
  const sandboxBars: SandboxBarItem[] = useMemo(() => {
    const out: SandboxBarItem[] = [];
    if (totals) {
      for (const tb of totals) {
        out.push({
          label: tb.name,
          value: tb.in + tb.out,
          hasCost: tb.cost > 0,
        });
      }
    } else {
      for (const e of entries ?? []) {
        if (e.error || !e.usage) continue;
        let inT = 0;
        let outT = 0;
        let hasCost = false;
        for (const r of e.usage.rows) {
          inT += r.in;
          outT += r.out;
          if ((r.cost ?? 0) > 0) hasCost = true;
        }
        out.push({ label: e.name, value: inT + outT, hasCost });
      }
    }
    return out.sort((a, b) => b.value - a.value);
  }, [totals, entries]);

  // S4: the single-sandbox 14-day trend (R2/R3/AC2). Derived from the
  // selected sandbox's byDay; days without data are gap-filled to zero so
  // the chart always spans the 14 calendar days.
  const trendItems: DayTrendItem[] = useMemo(() => {
    const byDay = current?.usage?.byDay;
    if (!byDay) return [];
    const byDate = new Map<string, DayTrendItem>();
    for (const d of byDay) {
      const ex = byDate.get(d.date) ?? { date: d.date, in: 0, out: 0, cost: 0 };
      ex.in += d.in;
      ex.out += d.out;
      if (d.cost !== undefined) ex.cost = (ex.cost ?? 0) + d.cost;
      byDate.set(d.date, ex);
    }
    // Last 14 calendar days ending today (UTC) — same span the backend clips
    // to. The backend returns only days with data; we gap-fill the rest.
    const out: DayTrendItem[] = [];
    const today = new Date();
    const todayUtc = Date.UTC(today.getUTCFullYear(), today.getUTCMonth(), today.getUTCDate());
    for (let i = 13; i >= 0; i--) {
      const ms = todayUtc - i * 86400000;
      const d = new Date(ms);
      const label = `${d.getUTCFullYear()}-${String(d.getUTCMonth() + 1).padStart(2, "0")}-${String(d.getUTCDate()).padStart(2, "0")}`;
      const item = byDate.get(label);
      out.push(item ?? { date: label, in: 0, out: 0 });
    }
    return out;
  }, [current]);

  // Distinct dates present in the current (agent, model) rows for the
  // single-sandbox view — the day-filter dropdown options.
  const dayOptions: string[] = useMemo(() => {
    const byDay = current?.usage?.byDay ?? [];
    return [...new Set(byDay.map((d) => d.date))].sort().reverse();
  }, [current]);

  // S4: (agent, model) -> DayUsage on the selected day (single-sandbox view).
  // When a day is selected, only rows that have usage that day are shown, and
  // the date column shows that day's per-row value (R4, AC3). Null when no
  // day is selected (the "全部" option) or in the combined view.
  const onDay = useMemo(() => {
    if (dayFilter === "" || selected === "") return null;
    const byDay = current?.usage?.byDay ?? [];
    const out = new Map<string, DayUsage>();
    for (const d of byDay) {
      if (d.date !== dayFilter) continue;
      out.set(`${d.agent}|${d.model}`, d);
    }
    return out;
  }, [dayFilter, current, selected]);

  // Rows for the table: the active view's rows, narrowed to (agent, model)
  // pairs that had usage on the selected day (when one is selected).
  const visibleRows = useMemo(() => {
    if (!onDay) return rows;
    return rows.filter((r) => onDay.has(`${r.agent}|${r.model}`));
  }, [rows, onDay]);

  // Day filter only makes sense in the per-sandbox view (the combined view
  // would need per-sandbox+day pairs, which byDay is not shaped for).
  const showDayFilter = selected !== "" && dayOptions.length > 0;

  // Reset the day filter when the selected sandbox changes (a date that
  // exists in one sandbox may not in another — and a stale filter would
  // silently show an empty table).
  useEffect(() => {
    // Previous selected value rides in dayFilter's closure; just clear when
    // a per-sandbox selection is active or the selection changed.
    setDayFilter("");
  }, [selected]);

  const s = summarize(visibleRows);

  return (
    <div className="page">
      <div className="page-head">
        <h1>{t(lang, "navUsage")}</h1>
        <div className="page-actions">
          <div className="segmented" role="group" aria-label="time window">
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
      </div>

      {error && <div className="status error">{t(lang, "loadFailed")}{error}</div>}

      {/* sandbox selector — a chip per sandbox + the combined view; erroring
       * sandboxes carry a warn dot with the message in the title. */}
      {entries !== null && entries.length > 0 && (
        <div className="mu-sbx-row">
          <button
            className={`chip${selected === "" ? " is-selected" : ""}`}
            aria-pressed={selected === ""}
            onClick={() => setSelected("")}
          >
            {t(lang, "muAll")}
          </button>
          {entries.map((e) => (
            <button
              key={e.name}
              className={`chip${selected === e.name ? " is-selected" : ""}${e.error ? " is-error" : ""}`}
              aria-pressed={selected === e.name}
              title={e.error ?? e.name}
              onClick={() => setSelected(e.name)}
            >
              {e.error && <span className="sdot warn" />}
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
      ) : visibleRows.length === 0 ? (
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
            {/* S4/AC1: cross-sandbox bars in the combined view; click = jump
             * to that sandbox's single view. */}
            {selected === "" && sandboxBars.length > 0 && (
              <div className="ml-chart-card">
                <h3 className="ml-chart-title">
                  {t(lang, "mcUsageBySandbox")}
                </h3>
                <SandboxBars
                  items={sandboxBars}
                  onSelect={(name) => setSelected(name)}
                />
              </div>
            )}
            {/* S4/R2 AC2: the 14-day trend in the per-sandbox view; hidden on
             * old apps (no byDay) or when the sandbox has no trend data. */}
            {selected !== "" && trendItems.length > 0 && (
              <div className="ml-chart-card ml-trend-card">
                <h3 className="ml-chart-title">
                  {t(lang, "mcUsageTrend")}
                </h3>
                <DayTrend items={trendItems} />
              </div>
            )}
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
            {/* S4/R4: day filter (per-sandbox view). "全部" = no filter; each
             * option narrows the table to rows with usage on that day. */}
            {showDayFilter && (
              <div className="mu-day-filter">
                <label htmlFor="mu-day-filter" className="mu-day-label">
                  {t(lang, "muDayFilter")}
                </label>
                <select
                  id="mu-day-filter"
                  className="input sm"
                  value={dayFilter}
                  onChange={(e) => setDayFilter(e.target.value)}
                >
                  <option value="">{t(lang, "muDayAll")}</option>
                  {dayOptions.map((d) => (
                    <option key={d} value={d}>
                      {d}
                    </option>
                  ))}
                </select>
              </div>
            )}
            <div className="ml-table-scroll">
              <table className="ml-table ml-usage-table">
                <thead>
                  <tr>
                    {selected === "" && <th>{t(lang, "muColSandbox")}</th>}
                    <th>{t(lang, "mcUsageColAgent")}</th>
                    <th>{t(lang, "mcUsageColProvider")}</th>
                    <th>{t(lang, "mcUsageColModel")}</th>
                    {/* S4/R4: date column shown only in the per-sandbox view
                     * with a day selected — value is that day's per-row usage. */}
                    {onDay && <th>{t(lang, "muColDay")}</th>}
                    <th className="ml-num">{t(lang, "mcUsageColIn")}</th>
                    <th className="ml-num">{t(lang, "mcUsageColOut")}</th>
                    <th className="ml-num">{t(lang, "mcUsageCacheHit")}</th>
                    {s.hasCost && (
                      <th className="ml-num">{t(lang, "mcUsageColCost")}</th>
                    )}
                  </tr>
                </thead>
                <tbody>
                  {visibleRows.map((r, i) => (
                    <tr key={i}>
                      {selected === "" && <td className="ml-cell-clip">{r.sandbox}</td>}
                      <td>{r.agent}</td>
                      <td className="ml-cell-clip" title={r.provider ?? undefined}>
                        {r.provider ?? "—"}
                      </td>
                      <td className="ml-cell-clip ml-cell-mono" title={r.model}>
                        {r.model}
                      </td>
                      {onDay && (
                        <td className="ml-num">
                          {(() => {
                            const d = onDay.get(`${r.agent}|${r.model}`);
                            return d ? fmtTokens(d.in + d.out + d.cacheRead + d.cacheWrite) : "—";
                          })()}
                        </td>
                      )}
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
                    <td colSpan={selected === "" ? 4 : onDay ? 4 : 3}>{t(lang, "mcUsageTotal")}</td>
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
