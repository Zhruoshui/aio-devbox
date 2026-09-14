// Lightweight Kumo-styled charts for the usage page — ported verbatim from
// web/src/panes/models/charts.tsx (Phase 4c).
//
// Deliberately dependency-free: horizontal bars are divs, the donut is an SVG
// circle segment stack. Colors come from the Kumo categorical palette — never
// a raw hex outside the palette. Both charts pair color with text labels so
// meaning survives grayscale.

import { fmtTokens } from "./types";

/** Kumo categorical palette, ordered. Cycled by index only when unavoidable. */
export const KUMO_CATEGORICAL = [
  "#4290F0",
  "#F5B647",
  "#E8649D",
  "#8D58EE",
  "#50C3B6",
  "#D37536",
];

export interface ChartItem {
  label: string;
  value: number;
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
              }}
            />
          </div>
          <span className="ml-bar-val">{formatValue(it.value)}</span>
        </div>
      ))}
    </div>
  );
}

const DONUT_RADIUS = 54;
const DONUT_STROKE = 24;

/**
 * SVG donut of cost share per label. Center shows the total; a legend beside
 * it pairs each segment's color with a label + value + percent (not color
 * alone). Caller hides this when there is no cost data.
 */
export function CostDonut({
  items,
  total,
  formatValue = (n: number) => `$${n.toFixed(2)}`,
}: {
  items: ChartItem[];
  total: number;
  formatValue?: (n: number) => string;
}): JSX.Element {
  const C = 2 * Math.PI * DONUT_RADIUS;
  let offset = 0;
  const segments = items.map((it, i) => {
    const frac = total > 0 ? it.value / total : 0;
    const len = frac * C;
    const seg = (
      <circle
        key={it.label}
        cx="80"
        cy="80"
        r={DONUT_RADIUS}
        fill="none"
        stroke={KUMO_CATEGORICAL[i % KUMO_CATEGORICAL.length]}
        strokeWidth={DONUT_STROKE}
        strokeDasharray={`${len} ${C - len}`}
        strokeDashoffset={-offset}
        transform="rotate(-90 80 80)"
      />
    );
    offset += len;
    return seg;
  });

  return (
    <div className="ml-donut">
      <div className="ml-donut-svg" role="img" aria-label="cost share donut">
        <svg viewBox="0 0 160 160" width="160" height="160">
          <circle
            cx="80"
            cy="80"
            r={DONUT_RADIUS}
            fill="none"
            stroke="var(--border-soft)"
            strokeWidth={DONUT_STROKE}
          />
          {segments}
        </svg>
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
            <span className="ml-donut-legend-val">
              {formatValue(it.value)}
            </span>
            <span className="ml-donut-legend-pct">
              {total > 0 ? `${((it.value / total) * 100).toFixed(0)}%` : ""}
            </span>
          </div>
        ))}
      </div>
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
          title={`${it.label} — ${ formatValue(it.value)}`}
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
              }}
            />
          </div>
          <span className="ml-bar-val">{formatValue(it.value)}</span>
        </button>
      ))}
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

const TREND_W = 520;
const TREND_H = 150;
const TREND_PAD_L = 52;
const TREND_PAD_R = 8;
const TREND_PAD_T = 12;
const TREND_PAD_B = 24;

/**
 * SVG trend of the last 14 days: two vertical bar series (token in/out) +
 * an overlaid cost line (only drawn when at least one day carries cost —
 * AC4 degrade for cost-less agents/sandboxes). X axis labels every-other
 * day to keep them readable. `items` should already be the 14-day series
 * sorted by date; missing calendar days are gap-filled to zero (the backend
 * returns only days that have data; the caller fills the rest).
 */
export function DayTrend({
  items,
  formatValue = fmtTokens,
}: {
  items: DayTrendItem[];
  formatValue?: (n: number) => string;
}): JSX.Element | null {
  // Title + legend (text labels pair color with meaning, grayscale-safe).
  const hasCost = items.some((d) => (d.cost ?? 0) > 0);
  const maxTok = Math.max(1, ...items.map((d) => d.in + d.out));
  const maxCost = Math.max(0.000001, ...items.map((d) => d.cost ?? 0));

  // Bar area: (x - PAD_L) / (W - PAD_L - PAD_R) spans the plot; each bar
  // group is centred on its day's x.
  const plotW = TREND_W - TREND_PAD_L - TREND_PAD_R;
  const plotH = TREND_H - TREND_PAD_T - TREND_PAD_B;
  const n = items.length;
  const groupW = plotW / Math.max(1, n);
  const barW = Math.max(3, groupW * 0.32);

  const bars: JSX.Element[] = [];
  let costPoints = "";
  items.forEach((d, i) => {
    const x = TREND_PAD_L + groupW * (i + 0.5);
    const hIn = plotH * (d.in / maxTok);
    const hOut = plotH * (d.out / maxTok);
    bars.push(
      <g key={d.date}>
        <rect
          x={x - barW - 1}
          y={TREND_PAD_T + plotH - hIn}
          width={barW}
          height={hIn}
          fill={KUMO_CATEGORICAL[0]}
        />
        <rect
          x={x + 1}
          y={TREND_PAD_T + plotH - hOut}
          width={barW}
          height={hOut}
          fill={KUMO_CATEGORICAL[1]}
        />
      </g>,
    );
    // Cost line point (only meaningful when a cost exists — but always emit
    // the path so hover tooltips line up; rendering is gated by hasCost).
    const cy = TREND_PAD_T + plotH - plotH * ((d.cost ?? 0) / maxCost);
    costPoints += `${x},${cy} `;
  });

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
            <span className="ml-trend-line-swatch" />
            cost
          </span>
        )}
      </div>
      <svg viewBox={`0 0 ${TREND_W} ${TREND_H}`} width="100%" height={TREND_H}>
        {/* gridlines at 0/25/50/75/100% of max */}
        {[0, 0.25, 0.5, 0.75, 1].map((f) => {
          const y = TREND_PAD_T + plotH - plotH * f;
          return (
            <g key={f}>
              <line
                x1={TREND_PAD_L}
                y1={y}
                x2={TREND_W - TREND_PAD_R}
                y2={y}
                stroke="var(--border-soft)"
                strokeDasharray="2 3"
              />
              <text x={TREND_PAD_L - 6} y={y + 3} textAnchor="end" className="ml-trend-tick">
                {formatValue(maxTok * f)}
              </text>
            </g>
          );
        })}
        {/* token bars: in (left) + out (right) per day, width-scaled to fit */}
        {bars}
        {/* X labels every other day */}
        {items.map((d, i) =>
          i % 2 === 0 ? (
            <text
              key={d.date}
              x={TREND_PAD_L + groupW * (i + 0.5)}
              y={TREND_H - 6}
              textAnchor="middle"
              className="ml-trend-tick"
            >
              {d.date.slice(5)}
            </text>
          ) : null,
        )}
        {/* cost line (AC4: only when some day has cost) */}
        {hasCost && (
          <polyline
            points={costPoints.trim()}
            fill="none"
            stroke={KUMO_CATEGORICAL[3]}
            strokeWidth={2}
          />
        )}
        {/* hover tooltips: invisible hit rects + title */}
        {items.map((d, i) => {
          const x = TREND_PAD_L + groupW * (i + 0.5);
          return (
            <rect
              key={d.date}
              x={x - groupW / 2}
              y={TREND_PAD_T}
              width={groupW}
              height={plotH}
              fill="transparent"
            >
              <title>
                {`${d.date} — in ${formatValue(d.in)}, out ${formatValue(d.out)}${
                  d.cost !== undefined ? `, cost $${(d.cost ?? 0).toFixed(2)}` : ""
                }`}
              </title>
            </rect>
          );
        })}
      </svg>
    </div>
  );
}
