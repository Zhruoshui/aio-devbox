// Charts for the usage page — Recharts-based (09-20), replacing the earlier
// hand-rolled div/SVG versions. Export surface is unchanged so UsagePage
// needs no edits: TokenBars, SandboxBars, CostDonut, DayTrend, ChartItem,
// SandboxBarItem, DayTrendItem, KUMO_CATEGORICAL.
//
// Style contract: colors come from the --chart-N tokens (a categorical
// palette validated per light/dark surface in styles.css) — never a raw hex
// outside them. Recharts animation durations follow the motion tokens where
// practical; every chart pairs color with text labels (legend/axis/tooltip)
// so meaning survives grayscale.
//
// Nominal bar lists (TokenBars, SandboxBars) deliberately stay div-based:
// one series, slot-1 hue — a chart library adds nothing there but weight.

import { useEffect, useState } from "react";
import {
  Area,
  Bar,
  CartesianGrid,
  Cell,
  ComposedChart,
  Line,
  Pie,
  PieChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import type { TooltipContentProps } from "recharts";
import { fmtCost, fmtTokens } from "./types";

/** Kumo categorical palette, ordered. Cycled by index only when unavoidable.
 * The values live in styles.css as --chart-N so dark mode can re-step slot
 * lightness without touching this module; resolved by CSS at paint time. */
export const KUMO_CATEGORICAL = [
  "var(--chart-1)",
  "var(--chart-2)",
  "var(--chart-3)",
  "var(--chart-4)",
  "var(--chart-5)",
  "var(--chart-6)",
];

export interface ChartItem {
  label: string;
  value: number;
}

/** Shared tooltip chrome: surface card, value leads (strong) + label follows
 * (secondary) per the interaction spec. Line keys, not boxes, in the trend
 * tooltip rows. Labels are model/sandbox names from the API — rendered via
 * React text interpolation only (no dangerouslySetInnerHTML). */
function TooltipCard({
  title,
  rows,
}: {
  title: string;
  rows: { label: string; value: string; key?: string }[];
}): JSX.Element {
  return (
    <div className="ml-tip">
      <div className="ml-tip-title">{title}</div>
      {rows.map((r) => (
        <div className="ml-tip-row" key={r.label}>
          {r.key !== undefined && (
            <span className="ml-tip-key" style={{ background: r.key }} />
          )}
          <span className="ml-tip-label">{r.label}</span>
          <span className="ml-tip-val">{r.value}</span>
        </div>
      ))}
    </div>
  );
}

/** Hook: CSS variables in SVG fills need a paint-cycle flip to re-resolve on
 * theme change. Returns a key that changes whenever the active [data-mode]
 * flips, forcing Recharts to re-render marks with fresh var() reads. */
function useThemeFlip(): number {
  const [flip, setFlip] = useState(0);
  useEffect(() => {
    const obs = new MutationObserver(() => setFlip((f) => f + 1));
    obs.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-mode", "data-theme"],
    });
    return () => obs.disconnect();
  }, []);
  return flip;
}

/**
 * Horizontal bar chart of a magnitude per label (e.g. tokens by model).
 * `items` should already be sorted descending and capped (top N) by the
 * caller. Bar width is relative to the max value; each row shows its label,
 * the bar, and the formatted value.
 */
export function TokenBars({
  items,
  formatValue = fmtTokens,
}: {
  items: ChartItem[];
  formatValue?: (n: number) => string;
}): JSX.Element | null {
  if (items.length === 0) return null;
  const max = Math.max(...items.map((i) => i.value));
  return (
    <div className="ml-bars" role="img" aria-label="usage by item">
      {items.map((it, i) => (
        <div className="ml-bar-row" key={it.label}>
          <span className="ml-bar-label" title={it.label}>
            {it.label}
          </span>
          <div className="ml-bar-track">
            <div
              className="ml-bar-fill"
              style={{
                width: `${max > 0 ? (it.value / max) * 100 : 0}%`,
                background: KUMO_CATEGORICAL[i % KUMO_CATEGORICAL.length],
                animationDelay: `${Math.min(i * 18, 180)}ms`,
              }}
            />
          </div>
          <span className="ml-bar-val">{formatValue(it.value)}</span>
        </div>
      ))}
    </div>
  );
}

/** A sandbox's total for the cross-sandbox bar list (S4, R1/AC1). */
export interface SandboxBarItem {
  label: string;
  value: number;
  /** Whether this sandbox has any cost data (drives the bar color, AC4). */
  hasCost: boolean;
}

/**
 * Horizontal bar chart of total tokens per sandbox (S4, R1/AC1). Same
 * interaction as TokenBars but each row is a sandbox and CLICKABLE — clicking
 * a bar jumps to that sandbox's single-sandbox view. Bars with cost data use
 * the cost accent, cost-less sandboxes a neutral dotted bar (AC4 degrade).
 */
export function SandboxBars({
  items,
  onSelect,
  formatValue = fmtTokens,
}: {
  items: SandboxBarItem[];
  onSelect: (label: string) => void;
  formatValue?: (n: number) => string;
}): JSX.Element | null {
  if (items.length === 0) return null;
  const max = Math.max(...items.map((i) => i.value));
  return (
    <div className="ml-bars" role="img" aria-label="usage by sandbox">
      {items.map((it, i) => (
        <button
          type="button"
          key={it.label}
          className="ml-sbx-bar"
          title={`${it.label} — ${formatValue(it.value)}`}
          onClick={() => onSelect(it.label)}
        >
          <span className="ml-bar-label" title={it.label}>
            {it.label}
          </span>
          <div className="ml-bar-track">
            <div
              className={`ml-bar-fill${it.hasCost ? "" : " ml-bar-fill-nocost"}`}
              style={{
                width: `${max > 0 ? (it.value / max) * 100 : 0}%`,
                background: it.hasCost
                  ? KUMO_CATEGORICAL[i % KUMO_CATEGORICAL.length]
                  : undefined,
                animationDelay: `${Math.min(i * 18, 180)}ms`,
              }}
            />
          </div>
          <span className="ml-bar-val">{formatValue(it.value)}</span>
        </button>
      ))}
    </div>
  );
}

const DONUT_INNER = 58;
const DONUT_OUTER = 78;
const DONUT_GAP = 3;

/**
 * Donut of cost share per label (Recharts Pie + paddingAngle surface gaps).
 * Center label shows the total; a legend beside it pairs each segment's
 * color with label + value + percent (not color alone). Caller hides this
 * when there is no cost data.
 */
export function CostDonut({
  items,
  total,
  formatValue = fmtCost,
}: {
  items: ChartItem[];
  total: number;
  formatValue?: (n: number) => string;
}): JSX.Element {
  const flip = useThemeFlip();
  const data = items.map((it) => ({ name: it.label, value: it.value }));

  return (
    <div className="ml-donut">
      <div className="ml-donut-svg" role="img" aria-label="cost share donut">
        {/* ResponsiveContainer needs a sized parent; .ml-donut-svg is 160px. */}
        <ResponsiveContainer key={flip} width="100%" height="100%">
          <PieChart>
            <Pie
              data={data}
              dataKey="value"
              nameKey="name"
              innerRadius={DONUT_INNER}
              outerRadius={DONUT_OUTER}
              paddingAngle={DONUT_GAP}
              startAngle={90}
              endAngle={-270}
              stroke="var(--surface)"
              strokeWidth={2}
              animationDuration={600}
              animationEasing="ease-out"
            >
              {items.map((it, i) => (
                <Cell
                  key={it.label}
                  fill={KUMO_CATEGORICAL[i % KUMO_CATEGORICAL.length]}
                />
              ))}
            </Pie>
            <Tooltip
              content={(props: TooltipContentProps) => {
                const p = props.payload?.[0];
                if (!p) return null;
                return (
                  <TooltipCard
                    title={String(p.name ?? "")}
                    rows={[
                      {
                        label: "cost",
                        value: formatValue(Number(p.value ?? 0)),
                        key: String(p.color ?? ""),
                      },
                    ]}
                  />
                );
              }}
            />
          </PieChart>
        </ResponsiveContainer>
        <div className="ml-donut-center">
          <span className="ml-donut-total">{formatValue(total)}</span>
          <span className="ml-donut-cap">total</span>
        </div>
      </div>
      <div className="ml-donut-legend">
        {items.map((it, i) => (
          <div className="ml-donut-legend-row" key={it.label}>
            <span
              className="ml-donut-swatch"
              style={{
                background: KUMO_CATEGORICAL[i % KUMO_CATEGORICAL.length],
              }}
            />
            <span className="ml-donut-legend-label" title={it.label}>
              {it.label}
            </span>
            <span className="ml-donut-legend-val">{formatValue(it.value)}</span>
            <span className="ml-donut-legend-pct">
              {total > 0 ? `${((it.value / total) * 100).toFixed(0)}%` : ""}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

/** A day in the 14-day trend series (S4, R2/R3/AC2). */
export interface DayTrendItem {
  date: string;
  in: number;
  out: number;
  cost?: number;
}

const TREND_H = 170;
const TREND_COST_KEY = "cost";

/**
 * 14-day trend (Recharts ComposedChart): grouped in/out bars + an overlaid
 * cost line, only drawn when at least one day carries cost (AC4 degrade for
 * cost-less agents/sandboxes). `items` should already be the 14-day series
 * sorted by date; missing calendar days are gap-filled to zero by the caller.
 * Cost is plotted on its own hidden axis but SCALED to the token axis domain
 * (one visual axis — the dataviz one-axis rule: never two y-scales).
 */
export function DayTrend({
  items,
  formatValue = fmtTokens,
}: {
  items: DayTrendItem[];
  formatValue?: (n: number) => string;
}): JSX.Element | null {
  const flip = useThemeFlip();
  const hasCost = items.some((d) => (d.cost ?? 0) > 0);
  // Cost scale: normalize to the token domain so both series share ONE axis.
  const maxTok = Math.max(1, ...items.map((d) => d.in + d.out));
  const maxCost = Math.max(0.000001, ...items.map((d) => d.cost ?? 0));
  const data = items.map((d) => ({
    date: d.date.slice(5),
    in: d.in,
    out: d.out,
    [TREND_COST_KEY]: hasCost
      ? ((d.cost ?? 0) / maxCost) * maxTok
      : undefined,
  }));

  return (
    <div className="ml-trend" role="img" aria-label="last 14 days usage trend">
      <div className="ml-trend-legend">
        <span className="ml-trend-legend-item">
          <span
            className="ml-trend-swatch"
            style={{ background: KUMO_CATEGORICAL[0] }}
          />
          in
        </span>
        <span className="ml-trend-legend-item">
          <span
            className="ml-trend-swatch"
            style={{ background: KUMO_CATEGORICAL[1] }}
          />
          out
        </span>
        {hasCost && (
          <span className="ml-trend-legend-item">
            <span
              className="ml-trend-line-swatch"
              style={{ borderTopColor: KUMO_CATEGORICAL[3] }}
            />
            cost
          </span>
        )}
      </div>
      <div className="ml-trend-plot">
        <ResponsiveContainer key={flip} width="100%" height={TREND_H}>
          <ComposedChart data={data} margin={{ top: 8, right: 4, left: 4, bottom: 0 }}>
            <CartesianGrid
              stroke="var(--border-soft)"
              strokeDasharray="0"
              vertical={false}
            />
            <XAxis
              dataKey="date"
              tick={{ fontSize: 10, fill: "var(--muted)" }}
              tickLine={false}
              axisLine={{ stroke: "var(--border-soft)" }}
              interval="preserveStartEnd"
              minTickGap={18}
            />
            <YAxis
              width={46}
              tick={{ fontSize: 10, fill: "var(--muted)" }}
              tickLine={false}
              axisLine={false}
              tickFormatter={(v: number) => formatValue(v)}
            />
            <Tooltip
              cursor={{ fill: "var(--surface-warm)" }}
              content={(props: TooltipContentProps) => {
                if (!props.payload || props.payload.length === 0) return null;
                // props.label is the x tick ("MM-DD"); resolve the raw day so
                // the tooltip shows true cost dollars, not the token-scaled
                // value the Line is plotted with.
                const raw = items.find(
                  (d) => d.date.slice(5) === String(props.label),
                );
                const row = props.payload[0]?.payload as
                  | { date: string; in: number; out: number }
                  | undefined;
                const rows: { label: string; value: string; key?: string }[] = [
                  {
                    label: "in",
                    value: formatValue(raw?.in ?? row?.in ?? 0),
                    key: KUMO_CATEGORICAL[0],
                  },
                  {
                    label: "out",
                    value: formatValue(raw?.out ?? row?.out ?? 0),
                    key: KUMO_CATEGORICAL[1],
                  },
                ];
                if (raw?.cost !== undefined) {
                  rows.push({
                    label: "cost",
                    value: fmtCost(raw.cost),
                    key: KUMO_CATEGORICAL[3],
                  });
                }
                return <TooltipCard title={raw?.date ?? String(props.label)} rows={rows} />;
              }}
            />
            <Bar
              dataKey="in"
              fill={KUMO_CATEGORICAL[0]}
              radius={[4, 4, 0, 0]}
              maxBarSize={14}
              animationDuration={500}
            />
            <Bar
              dataKey="out"
              fill={KUMO_CATEGORICAL[1]}
              radius={[4, 4, 0, 0]}
              maxBarSize={14}
              animationDuration={500}
            />
            {hasCost && (
              <>
                <Line
                  type="monotone"
                  dataKey={TREND_COST_KEY}
                  stroke={KUMO_CATEGORICAL[3]}
                  strokeWidth={2}
                  dot={false}
                  activeDot={{ r: 4, strokeWidth: 2, stroke: "var(--surface)" }}
                  animationDuration={700}
                />
                <Area
                  type="monotone"
                  dataKey={TREND_COST_KEY}
                  stroke="none"
                  fill={KUMO_CATEGORICAL[3]}
                  fillOpacity={0.1}
                  animationDuration={700}
                />
              </>
            )}
          </ComposedChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
}
