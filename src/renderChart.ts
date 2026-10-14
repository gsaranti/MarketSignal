// Renders a fenced ```chart block's JSON body into an inline-SVG figure after
// the design system's YieldChart reference: monochrome ink series, at most one
// accent-emphasized series, a hairline grid, no markers or shadows. "line" keeps
// the reference register; "bar" and "area" extend it to flat fills drawn from a
// zero baseline (a noted design-package extension, mirrored in
// colors_and_type.css). Styling lives there under `.prose .chart-*`; this
// module emits only geometry and class hooks.
//
// Pure and fail-soft: any parse or validation failure returns null so the
// markdown-it fence rule falls back to a plain code block. Labels are escaped
// before embedding because the returned string is injected via v-html, which
// bypasses markdown-it's html:false guard.

interface ChartSeries {
  label?: string;
  points: number[];
  emphasis: boolean;
}

type ChartType = "line" | "bar" | "area";

interface ChartSpec {
  type: ChartType;
  title?: string;
  series: ChartSeries[];
  // Per-slot x-axis labels, bar-only: a line or area connecting unrelated
  // categories would imply a trend that isn't real.
  categories?: string[];
}

// Caps: a handful of series over a bounded run, not a dashboard.
const MAX_SERIES = 3;
const MAX_POINTS = 120;
const MIN_POINTS = 2;
// Labeled x-ticks truncate to nothing past this; 16 clears the 11 GICS sectors.
// A category list must match the point count, so this also caps a categorical
// bar's points.
const MAX_CATEGORIES = 16;

// viewBox units, matching the reference (720×130, 8px padding); the SVG scales
// to the reading column in CSS.
const W = 720;
const H = 130;
const PAD = 8;
// The y-tick labels sit at the right edge; the data area stops at `plotR`, a
// gutter sized to the widest tick string so a six-figure axis never overflows
// into the plot. TICK_CHAR_W is a conservative upper bound on a 9px mono
// digit's advance in viewBox units.
const TICK_CHAR_W = 6;
const TICK_GAP = 6;
// Height added below the plot for category labels — a taller viewBox rather
// than a shrunken plot, so the plot geometry is untouched.
const X_AXIS_BAND = 28;
// Category-label baselines below H; labels stagger across two rows.
const X_LABEL_BASELINE = 11;
const X_LABEL_ROW_GAP = 11;

// Bars fill ~2/3 of each slot with a 1u gutter within a group; the floor keeps a
// many-series group from collapsing to slivers.
const BAR_SLOT_FILL = 0.66;
const BAR_GUTTER = 1;
const BAR_MIN_WIDTH = 0.5;

// A conservative UPPER BOUND on a glyph's advance in the 10px label font, in
// viewBox units (about one em). An upper bound, not an average, is what makes
// the label clamps actually contain real labels; wide Unicode (CJK/emoji) can
// exceed it.
const LABEL_CHAR_W = 10;
// Bounds the on-canvas end label so a pathological one can't outgrow the
// canvas; the full label still rides into the aria description.
const MAX_LABEL_CHARS = 24;

// Covers element text and double-quoted attribute values; deliberately an
// escape, not a denylist, so an `onerror=`-style substring renders inert.
function escapeXml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

// Trims the space a cut exposes so "Real Estate" ellipsizes to "Real…", not
// "Real …".
function truncate(s: string, maxChars: number): string {
  if (s.length <= maxChars) return s;
  return `${s.slice(0, maxChars - 1).trimEnd()}…`;
}

function isFiniteNumber(v: unknown): v is number {
  return typeof v === "number" && Number.isFinite(v);
}

function parseSpec(content: string): ChartSpec | null {
  let parsed: unknown;
  try {
    // A model sometimes emits a Unicode dash (U+2010–2015, U+2212) for a minus
    // sign; fold those to ASCII only where a digit follows, so an em-dash in a
    // title is left alone.
    const normalized = content.replace(/[‐-―−](?=\d)/g, "-");
    parsed = JSON.parse(normalized);
  } catch {
    return null;
  }
  if (typeof parsed !== "object" || parsed === null) return null;
  const obj = parsed as Record<string, unknown>;
  if (obj.type !== "line" && obj.type !== "bar" && obj.type !== "area") {
    return null;
  }
  const type: ChartType = obj.type;
  if (!Array.isArray(obj.series) || obj.series.length === 0) return null;
  if (obj.series.length > MAX_SERIES) return null;

  const series: ChartSeries[] = [];
  let emphasisCount = 0;
  for (const raw of obj.series) {
    if (typeof raw !== "object" || raw === null) return null;
    const s = raw as Record<string, unknown>;
    if (!Array.isArray(s.points)) return null;
    if (s.points.length < MIN_POINTS || s.points.length > MAX_POINTS) return null;
    if (!s.points.every(isFiniteNumber)) return null;
    if (s.label !== undefined && typeof s.label !== "string") return null;
    const emphasis = s.emphasis === true;
    if (emphasis) emphasisCount += 1;
    series.push({
      label: typeof s.label === "string" ? s.label : undefined,
      points: s.points as number[],
      emphasis,
    });
  }
  if (emphasisCount > 1) return null;
  // All series share one x-axis, so unequal lengths would imply an alignment
  // that isn't real.
  if (!series.every((s) => s.points.length === series[0].points.length)) {
    return null;
  }

  // A blank title would render an empty caption and a dangling "Line chart: .".
  const rawTitle = typeof obj.title === "string" ? obj.title.trim() : "";
  const title = rawTitle.length > 0 ? rawTitle : undefined;

  // Labels are trimmed so surrounding whitespace can't eat the per-slot
  // truncation budget and render as "   …".
  let categories: string[] | undefined;
  if (obj.categories !== undefined) {
    if (type !== "bar") return null;
    // A categorical bar identifies its series by fill alone, and the palette is
    // ink plus at most one accent, so it honestly shows at most two series, a
    // two-series one needing exactly one `emphasis`. Reject rather than draw an
    // ambiguous chart.
    if (series.length > 2) return null;
    if (series.length === 2 && emphasisCount !== 1) return null;
    // The colour-keyed legend needs two distinct, non-blank names; a lone
    // series leans on the title.
    if (series.length === 2) {
      const a = typeof series[0].label === "string" ? series[0].label.trim() : "";
      const b = typeof series[1].label === "string" ? series[1].label.trim() : "";
      if (a.length === 0 || b.length === 0 || a === b) return null;
    }
    if (!Array.isArray(obj.categories) || obj.categories.length === 0) return null;
    if (obj.categories.length > MAX_CATEGORIES) return null;
    if (!obj.categories.every((c) => typeof c === "string" && c.trim().length > 0))
      return null;
    if (obj.categories.length !== series[0].points.length) return null;
    categories = (obj.categories as string[]).map((c) => c.trim());
  }

  return { type, title, series, categories };
}

function coord(n: number): string {
  return (Math.round(n * 100) / 100).toString();
}

// Y-axis tick label: integer for index-scale levels, 2 decimals for rates/spreads.
function formatTick(v: number): string {
  return Math.abs(v) >= 100 ? Math.round(v).toString() : v.toFixed(2);
}

function polyline(
  points: number[],
  x: (i: number) => number,
  y: (v: number) => number,
): string {
  return points
    .map((v, i) => `${i === 0 ? "M" : "L"}${coord(x(i))} ${coord(y(v))}`)
    .join(" ");
}

// Emphasized series last, so the accent draws on top of the others.
function byEmphasis(series: ChartSeries[]): ChartSeries[] {
  return [...series].sort((a, b) => Number(a.emphasis) - Number(b.emphasis));
}

function linePaths(
  series: ChartSeries[],
  y: (v: number) => number,
  xScale: (count: number) => (i: number) => number,
): string {
  return byEmphasis(series)
    .map((s) => {
      const x = xScale(s.points.length);
      const cls = s.emphasis ? "chart-line chart-line--accent" : "chart-line";
      return `<path class="${cls}" d="${polyline(s.points, x, y)}" />`;
    })
    .join("");
}

// Top strokes are drawn over ALL fills so a base series' line is never occluded
// by the accent wash.
function areaShapes(
  series: ChartSeries[],
  y: (v: number) => number,
  xScale: (count: number) => (i: number) => number,
): string {
  const y0 = coord(y(0));
  const fills = byEmphasis(series)
    .map((s) => {
      const x = xScale(s.points.length);
      const top = polyline(s.points, x, y);
      const lastX = coord(x(s.points.length - 1));
      const firstX = coord(x(0));
      const cls = s.emphasis ? "chart-area chart-area--accent" : "chart-area";
      return `<path class="${cls}" d="${top} L${lastX} ${y0} L${firstX} ${y0} Z" />`;
    })
    .join("");
  return fills + linePaths(series, y, xScale);
}

// Bars center in evenly-divided slots rather than on the continuous point
// axis, so the edge bars stay inside the canvas. Shared with end-label
// placement so a label centers over its own bar.
function barColumn(
  slotCount: number,
  n: number,
  slotIndex: number,
  j: number,
  plotR: number,
): { x: number; w: number } {
  const slot = (plotR - PAD) / slotCount;
  const groupW = slot * BAR_SLOT_FILL;
  const bandW = groupW / n;
  const w = Math.max(BAR_MIN_WIDTH, bandW - BAR_GUTTER);
  const groupLeft = PAD + (slotIndex + 0.5) * slot - groupW / 2;
  return { x: groupLeft + j * bandW, w };
}

function barRects(series: ChartSeries[], y: (v: number) => number, plotR: number): string {
  const slotCount = series[0].points.length;
  const n = series.length;
  const y0 = y(0);
  const rects: string[] = [];
  for (let i = 0; i < slotCount; i += 1) {
    series.forEach((s, j) => {
      const { x, w } = barColumn(slotCount, n, i, j, plotR);
      const yv = y(s.points[i]);
      const top = Math.min(yv, y0);
      const ht = Math.abs(yv - y0);
      const cls = s.emphasis ? "chart-bar chart-bar--accent" : "chart-bar";
      rects.push(
        `<rect class="${cls}" x="${coord(x)}" y="${coord(top)}" width="${coord(w)}" height="${coord(ht)}" />`,
      );
    });
  }
  return rects.join("");
}

function buildSvg(spec: ChartSpec): string {
  const all = spec.series.flatMap((s) => s.points);
  let min = Math.min(...all);
  let max = Math.max(...all);
  // Bar and area read magnitude against a real origin, so their domain includes
  // zero; a line keeps its data-fitted domain so a level series isn't squashed.
  const anchorZero = spec.type !== "line";
  if (anchorZero) {
    min = Math.min(min, 0);
    max = Math.max(max, 0);
  }
  if (min === max) {
    // A flat series — pad symmetrically so it renders as a centered line.
    min -= 1;
    max += 1;
  }
  const headroom = (max - min) * 0.08;
  min -= headroom;
  max += headroom;

  // Reserve a gutter for the widest tick string (see TICK_CHAR_W).
  const tickVals = [max, (max + min) / 2, min];
  const tickStrings = tickVals.map(formatTick);
  const tickW = Math.max(...tickStrings.map((s) => s.length)) * TICK_CHAR_W;
  const plotR = W - 4 - tickW - TICK_GAP;

  const y = (v: number): number =>
    PAD + (1 - (v - min) / (max - min)) * (H - 2 * PAD);
  const xScale =
    (count: number) =>
    (i: number): number =>
      count <= 1 ? (PAD + plotR) / 2 : PAD + i * ((plotR - PAD) / (count - 1));

  const grid = [0, 1, 2, 3]
    .map((i) => {
      const gy = PAD + (i * (H - 2 * PAD)) / 3;
      return `<line class="chart-grid" x1="0" x2="${W}" y1="${coord(gy)}" y2="${coord(gy)}" />`;
    })
    .join("");

  // A line chart has no fixed origin, so it omits the zero baseline.
  const baseline = anchorZero
    ? `<line class="chart-baseline" x1="0" x2="${W}" y1="${coord(y(0))}" y2="${coord(y(0))}" />`
    : "";

  const geometry =
    spec.type === "bar"
      ? barRects(spec.series, y, plotR)
      : spec.type === "area"
        ? areaShapes(spec.series, y, xScale)
        : linePaths(spec.series, y, xScale);

  const ticks = tickVals
    .map((v, i) => {
      const ty = y(v) + 3;
      return `<text class="chart-tick" x="${W - 4}" y="${coord(ty)}" text-anchor="end">${escapeXml(tickStrings[i])}</text>`;
    })
    .join("");

  // End-of-series labels start near each series' last point (the emphasized one
  // above, the rest below), are decluttered together so no two stack, and the
  // cluster is nudged back inside the canvas. Line/area labels ride the shared
  // right edge; bar labels center over each series' own last bar.
  const lastI = spec.series[0].points.length - 1;
  const isBar = spec.type === "bar";
  const n = spec.series.length;
  const lineEndX = coord(xScale(spec.series[0].points.length)(lastI) - 4);
  const anchor = isBar ? "middle" : "end";

  // Categorical bars suppress the end label: it would ride fill-on-fill over
  // the last bar, and the category axis, title and aria already name the
  // series.
  const endLabelSeries = spec.categories !== undefined ? [] : spec.series;

  const labels = endLabelSeries
    .map((s, j) => ({ s, j }))
    .filter(({ s }) => s.label !== undefined)
    .map(({ s, j }) => {
      const raw = s.label as string;
      const display = truncate(raw, MAX_LABEL_CHARS);
      let lx = lineEndX;
      if (isBar) {
        const col = barColumn(spec.series[0].points.length, n, lastI, j, plotR);
        // The last bar's center nears W as the point count grows and the SVG
        // hides overflow, so clamp by the estimated half-width.
        const halfW = (display.length * LABEL_CHAR_W) / 2;
        const center = col.x + col.w / 2;
        lx = coord(Math.min(Math.max(center, PAD + halfW), plotR - halfW));
      }
      return {
        text: escapeXml(display),
        cls: s.emphasis ? "chart-endlabel chart-endlabel--accent" : "chart-endlabel",
        x: lx,
        y: y(s.points[lastI]) + (s.emphasis ? -5 : 12),
      };
    });
  const LABEL_GAP = 11;
  const LABEL_TOP = 8;
  const LABEL_BOTTOM = H - 2;
  labels.sort((a, b) => a.y - b.y);
  for (let i = 1; i < labels.length; i += 1) {
    labels[i].y = Math.max(labels[i].y, labels[i - 1].y + LABEL_GAP);
  }
  if (labels.length > 0) {
    const bottomOverflow = labels[labels.length - 1].y - LABEL_BOTTOM;
    if (bottomOverflow > 0) for (const lbl of labels) lbl.y -= bottomOverflow;
    const topOverflow = LABEL_TOP - labels[0].y;
    if (topOverflow > 0) for (const lbl of labels) lbl.y += topOverflow;
  }
  const endLabels = labels
    .map(
      (lbl) =>
        `<text class="${lbl.cls}" x="${lbl.x}" y="${coord(lbl.y)}" text-anchor="${anchor}">${lbl.text}</text>`,
    )
    .join("");

  // Category labels stagger across two rows so each clears its same-row
  // neighbour by two slots, roughly doubling its width budget — which is what
  // lets "Consumer Discretionary" / "Consumer Staples" truncate to distinct
  // stubs instead of two "Consu…". The budget is capped by the distance to the
  // nearer viewBox edge, so an edge label is contained with no clamp.
  const xLabels =
    spec.categories !== undefined
      ? spec.categories
          .map((cat, i) => {
            const count = spec.categories!.length;
            const slot = (plotR - PAD) / count;
            const cx = PAD + (i + 0.5) * slot;
            // The band sits below the plot, clear of the y-tick gutter, so a
            // label may use the full width to W.
            const halfBudget = Math.min(slot, cx, W - cx);
            const maxChars = Math.max(1, Math.floor((2 * halfBudget) / LABEL_CHAR_W));
            const display = truncate(cat, maxChars);
            const y = H + X_LABEL_BASELINE + (i % 2) * X_LABEL_ROW_GAP;
            // A truncated name carries the full label as a native <title>
            // tooltip; it is also in the figure's aria-label, so it is never
            // hover-gated. Labels are deliberately not focusable: up to 16 tab
            // stops would buy nothing the figure-level aria doesn't give.
            const tip = display !== cat ? `<title>${escapeXml(cat)}</title>` : "";
            return `<text class="chart-xlabel" x="${coord(cx)}" y="${coord(y)}" text-anchor="middle">${tip}${escapeXml(display)}</text>`;
          })
          .join("")
      : "";

  // The SVG is injected via v-html and otherwise invisible to assistive tech. A
  // categorical bar enumerates each category and value (a left-to-right
  // direction is meaningless across categories). Each field is escaped on its
  // own, not the composed string, so a later edit can't skip escaping.
  const safeTitle = spec.title !== undefined ? escapeXml(spec.title) : "";
  const seriesDesc =
    spec.categories !== undefined
      ? spec.series
          .map((s, i) => {
            const name = escapeXml(s.label ?? `series ${i + 1}`);
            const pairs = spec.categories!
              .map((cat, k) => `${escapeXml(cat)} ${formatTick(s.points[k])}`)
              .join(", ");
            return `${name}: ${pairs}`;
          })
          .join("; ")
      : spec.series
          .map((s, i) => {
            const name = escapeXml(s.label ?? `series ${i + 1}`);
            const start = s.points[0];
            const end = s.points[s.points.length - 1];
            const direction =
              end > start ? "rising" : end < start ? "falling" : "flat";
            return `${name} from ${formatTick(start)} to ${formatTick(end)}, ${direction}`;
          })
          .join("; ");
  const typeName =
    spec.type === "bar"
      ? "Bar chart"
      : spec.type === "area"
        ? "Area chart"
        : "Line chart";
  const aria = `${typeName}${safeTitle ? `: ${safeTitle}` : ""}. ${seriesDesc}`;

  const caption =
    spec.title !== undefined
      ? `<figcaption class="chart-caption">${escapeXml(spec.title)}</figcaption>`
      : "";

  const vbH = spec.categories !== undefined ? H + X_AXIS_BAND : H;

  // The legend replaces the suppressed end label for a two-series categorical
  // (validated to one ink and one accent series, distinctly labeled). A single
  // series shows none: its label usually restates the title.
  const showLegend =
    spec.categories !== undefined &&
    spec.series.length >= 2 &&
    spec.series.every((s) => s.label !== undefined);
  const legend = showLegend
    ? `<div class="chart-legend">${spec.series
        .map((s) => {
          const swatch = s.emphasis
            ? "chart-legend-swatch chart-legend-swatch--accent"
            : "chart-legend-swatch";
          return `<span class="chart-legend-item"><span class="${swatch}"></span>${escapeXml(s.label as string)}</span>`;
        })
        .join("")}</div>`
    : "";

  return (
    `<figure class="chart-figure">` +
    legend +
    `<svg class="chart-svg" viewBox="0 0 ${W} ${vbH}" role="img" aria-label="${aria}">` +
    grid +
    geometry +
    baseline +
    ticks +
    endLabels +
    xLabels +
    `</svg>` +
    caption +
    `</figure>`
  );
}

// Render a ```chart fence body to inline-SVG HTML, or null to fall back.
export function renderChart(content: string): string | null {
  const spec = parseSpec(content);
  if (spec === null) return null;
  try {
    return buildSvg(spec);
  } catch {
    return null;
  }
}
