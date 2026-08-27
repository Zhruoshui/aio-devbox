// Lightweight Kumo-styled charts for the usage tab.
//
// Deliberately dependency-free: horizontal bars are divs, the donut is an SVG
// circle segment stack. Colors come from the Kumo categorical palette
// (cloudflare_kumo_ui.md §Data visualization) — never a raw hex outside the
// palette. Both charts pair color with text labels so meaning survives
// grayscale (design §7).

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
    <div className="ml-bars" role="img" aria-label="按项目用量">
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
      <div className="ml-donut-svg" role="img" aria-label="成本占比环图">
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
