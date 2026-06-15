// Render a fenced ```chart block's JSON body into a restrained inline-SVG figure,
// generalizing the design system's YieldChart reference
// (market-signal-design-system/.../market_signal_desktop/LatestReport.jsx):
// monochrome ink series, at most one accent-emphasized series, a hairline grid,
// no markers / shadows. Three `type`s render: "line" (the reference register, no
// fills); "bar" (grouped flat bars) and "area" (a faint single-tint wash) — both
// drawn from a zero baseline so signed / near-zero data reads honestly from a
// real origin. Bar and area broaden the line register's no-fills rule to flat
// fills only (squared, hairline-gridded, no gradient / shadow) — a noted design-
// package extension, mirrored in colors_and_type.css. Styling lives there under
// `.prose .chart-*` so it resolves in both themes; this module only emits the
// geometry and the class hooks.
//
// Pure and fail-soft: any parse or validation failure returns null so the caller
// (the markdown-it fence rule) can fall back to a plain code block — a malformed
// chart never breaks the surrounding report render. The agent's chart labels are
// escaped before embedding because the returned string is injected via v-html,
// which bypasses markdown-it's html:false guard.

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
  // Optional per-chart x-axis category labels (one per slot, shared across
  // series). Bar-only: a category axis makes each slot a named cross-sectional
  // comparison ("returns by sector") rather than a time step — so it carries
  // labeled x-ticks and enumerated aria, and it stays a discrete bar (a line or
  // area connecting unrelated categories would imply a trend that isn't real).
  categories?: string[];
}

// Robustness caps — the journal-figure register is a handful of series over a
// bounded run, not a dashboard. Mirrors the reference's 2 series / 26 points.
const MAX_SERIES = 3;
const MAX_POINTS = 120;
const MIN_POINTS = 2;
// Categorical bars carry a labeled x-axis, so the legibility ceiling is far
// lower than a time series' MAX_POINTS — past this the per-slot labels truncate
// to nothing. 16 clears the 11 GICS sectors with headroom. Because a category
// list must match the series' point count, this also caps a categorical bar's
// point count (a 120-bar categorical chart is rejected, falling back to a table).
const MAX_CATEGORIES = 16;

// SVG geometry in viewBox units — mirrors the reference (720×130, 8px padding).
// The SVG scales uniformly to the reading column (width:100%, height:auto in
// CSS); strokes are pinned crisp via vector-effect:non-scaling-stroke there.
const W = 720;
const H = 130;
const PAD = 8;
// The y-axis tick labels sit at the right edge (anchored at W-4). To keep plotted
// data — bars and the line/area end labels — from growing under them, the data
// area stops at a right edge (`plotR`, computed per chart in buildSvg) that leaves
// a gutter sized to the WIDEST tick string: TICK_CHAR_W is a conservative upper
// bound on a 9px mono digit's advance (viewBox units), and TICK_GAP is the clear
// space between the widest tick and the plot. Sizing from the actual ticks (not a
// fixed width) keeps a large-magnitude axis (six-figure index levels) from
// overflowing back into the plot.
const TICK_CHAR_W = 6;
const TICK_GAP = 6;
// Extra height appended below the plot for the x-axis category labels (only when
// `categories` is present). Reserved as a taller viewBox rather than by shrinking
// the plot, so the line/bar/area domain, grid, ticks, and end-label geometry are
// untouched — the band simply hangs below the existing H.
const X_AXIS_BAND = 28;
// Baselines of the x-axis category text within that band (from the top of the
// plot, i.e. H + this). Labels stagger across two rows so each clears its
// same-row neighbour by two slots; the lower row sits X_LABEL_ROW_GAP beneath
// the upper, and the band leaves descender room below the lower 9px label.
const X_LABEL_BASELINE = 11;
const X_LABEL_ROW_GAP = 11;

// Bar layout in viewBox units — the grouped bars fill ~2/3 of each categorical
// slot (leaving inter-slot gaps), with a ~1u gutter between bars within a group
// and a floor so a many-series group never collapses to invisible slivers.
const BAR_SLOT_FILL = 0.66;
const BAR_GUTTER = 1;
const BAR_MIN_WIDTH = 0.5;

// A conservative UPPER BOUND on a glyph's advance in the 10px end-label font, in
// viewBox units (≈ one em — the widest Latin glyphs, W/M/%, stay under it). Used
// to clamp a centered bar label so its text can't clip past the 720-wide viewBox.
// Being an upper bound (not an average) is what makes the clamp actually contain
// real labels, and makes the unit test that reuses it a sound worst-case check
// rather than a circular under-estimate. Exotic wide-Unicode (CJK/emoji) can
// exceed one em; that residual and true rendered-pixel fit are a GUI-pass concern.
const LABEL_CHAR_W = 10;
// Series labels are short tags ("10Y", "HY OAS"); a chart can't render a long
// one legibly, so the on-canvas end label is truncated to a sane width (the full
// label still rides into the aria description). This bounds the clamp below, so a
// pathological label can't out-grow the canvas no matter where its bar lands.
const MAX_LABEL_CHARS = 24;

// HTML/attr-context escaping for the five XML metacharacters — enough for both
// element text and double-quoted attribute values (where these strings land). It
// is deliberately not a denylist of "script-looking" tokens: an `onerror=`-style
// substring is rendered inert as escaped text, never as a live attribute.
function escapeXml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

// Ellipsize to a bounded character count, trimming any trailing space the cut
// exposes so a truncated label never renders a dangling gap before the ellipsis
// ("Real Estate" -> "Real…", not "Real …"). Shared by the end labels and the
// x-axis category labels.
function truncate(s: string, maxChars: number): string {
  if (s.length <= maxChars) return s;
  return `${s.slice(0, maxChars - 1).trimEnd()}…`;
}

function isFiniteNumber(v: unknown): v is number {
  return typeof v === "number" && Number.isFinite(v);
}

// Parse + validate the spec; null on any contract violation (caller falls back).
function parseSpec(content: string): ChartSpec | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(content);
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
  // All series share one x-axis (point i is the same observation across series),
  // so they must have equal length — otherwise stretching unequal series across
  // the full width would imply an alignment that isn't real.
  if (!series.every((s) => s.points.length === series[0].points.length)) {
    return null;
  }

  // Normalize a blank or whitespace-only title to "no title" so it never renders
  // an empty caption or a dangling "Line chart: ." in the aria description.
  const rawTitle = typeof obj.title === "string" ? obj.title.trim() : "";
  const title = rawTitle.length > 0 ? rawTitle : undefined;

  // Optional category x-axis. Bar-only (line/area imply a continuous axis), one
  // non-empty string per slot — so its length must match the shared point count,
  // and it's capped at MAX_CATEGORIES. Any violation rejects the whole spec.
  // Labels are trimmed (like the title) so surrounding whitespace can't eat the
  // per-slot truncation budget and render a label as "   …".
  let categories: string[] | undefined;
  if (obj.categories !== undefined) {
    if (type !== "bar") return null;
    // A categorical bar identifies its series by fill colour alone — the per-series
    // end label is suppressed and the legend is colour-keyed. The palette is ink
    // plus at most one accent (design system: at most one emphasized series), i.e.
    // two distinguishable fills — so a categorical bar can honestly show at most two
    // series, and a two-series one needs exactly one `emphasis` (one ink, one accent).
    // More series, or two that would both render ink, can't be told apart, so reject
    // (fall back to a table / code block) rather than draw an ambiguous chart.
    if (series.length > 2) return null;
    if (series.length === 2 && emphasisCount !== 1) return null;
    // Both series need a non-blank label so the colour-keyed legend can say what each
    // fill means, AND the two must read differently so the key isn't two identical
    // entries. Reject missing, blank, or duplicate (whitespace-normalized) labels —
    // two unlabeled or same-named colours are as ambiguous as two ink bars. (A
    // single-series categorical needs no label; it leans on the title.)
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

// Compact coordinate — 2 decimals, no trailing-zero noise in the path string.
function coord(n: number): string {
  return (Math.round(n * 100) / 100).toString();
}

// Y-axis tick label: integer for index-scale levels, 2 decimals for rates/spreads.
function formatTick(v: number): string {
  return Math.abs(v) >= 100 ? Math.round(v).toString() : v.toFixed(2);
}

// The "M…L…" polyline through a series' points — the open top edge shared by the
// line geometry and the area's filled outline.
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

// Line geometry: one polyline path per series. Shared by "line" and (as the top
// stroke) "area".
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

// Area geometry: each series' line closed down to the zero baseline and filled
// with a faint single tint, then the crisp top strokes drawn over ALL fills so a
// base series' line is never occluded by the accent series' wash.
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

// The left edge and width of one bar: series `j` of `n` within the categorical
// slot at `slotIndex` of `slotCount`. Bars are centered in evenly-divided slots
// (not on the continuous point axis), so the edge bars stay fully inside the
// canvas and a multi-series group sits side-by-side within its slot. Shared by
// the geometry and the end-label placement so a label centers over its own bar.
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

// Bar geometry: vertical rects grown from the zero baseline (up for positive,
// down for negative).
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
  // Bar and area read magnitude against a real origin, so their domain always
  // includes zero (and a hairline baseline is drawn at y(0)); a line keeps its
  // data-fitted domain so a level series isn't squashed against an unused axis.
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

  // The three y-tick values (top / middle / bottom of the padded domain) and the
  // data area's right edge: reserve a gutter for the WIDEST tick string so the
  // ticks (anchored at W-4) never overlap the plotted data (see TICK_CHAR_W).
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

  // 4 hairline horizontal grid lines, matching the reference.
  const grid = [0, 1, 2, 3]
    .map((i) => {
      const gy = PAD + (i * (H - 2 * PAD)) / 3;
      return `<line class="chart-grid" x1="0" x2="${W}" y1="${coord(gy)}" y2="${coord(gy)}" />`;
    })
    .join("");

  // Zero baseline for bar/area only — the axis bars and areas grow from. A line
  // chart has no fixed origin, so it omits it.
  const baseline = anchorZero
    ? `<line class="chart-baseline" x1="0" x2="${W}" y1="${coord(y(0))}" y2="${coord(y(0))}" />`
    : "";

  // Series geometry branches on type; everything else (domain, grid, ticks,
  // end labels, aria, caption) is shared across the three types.
  const geometry =
    spec.type === "bar"
      ? barRects(spec.series, y, plotR)
      : spec.type === "area"
        ? areaShapes(spec.series, y, xScale)
        : linePaths(spec.series, y, xScale);

  // ~3 y-axis tick labels — top / middle / bottom of the padded domain (values +
  // strings computed above so the gutter could be sized to them).
  const ticks = tickVals
    .map((v, i) => {
      const ty = y(v) + 3;
      return `<text class="chart-tick" x="${W - 4}" y="${coord(ty)}" text-anchor="end">${escapeXml(tickStrings[i])}</text>`;
    })
    .join("");

  // End-of-series labels. Each label starts near its series' last point — the
  // emphasized one just above, the rest just below (reference idiom) — then ALL
  // of them (accent included) are decluttered together so no two stack, and the
  // cluster is nudged back inside the canvas if it would clip off either edge
  // (frontend-craft overflow handling). For line/area the labels ride the shared
  // right edge (anchor end, since every series ends at the same x); for bars they
  // center over each series' own last bar (anchor middle), so a label sits with
  // the bar it names rather than floating off in the right margin.
  const lastI = spec.series[0].points.length - 1;
  const isBar = spec.type === "bar";
  const n = spec.series.length;
  const lineEndX = coord(xScale(spec.series[0].points.length)(lastI) - 4);
  const anchor = isBar ? "middle" : "end";

  // Categorical bars suppress the on-canvas series end-label: it would ride over
  // the series' last bar (low-contrast fill-on-fill) and the category axis + title
  // already name the chart, with each series' identity preserved in the aria
  // description. (A dedicated multi-series categorical legend is a future
  // enhancement; for now a multi-series categorical chart leans on the aria.)
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
        // Center over the bar, but the last bar's center nears x≈W as the point
        // count grows; a middle-anchored label there would clip past the 720-wide
        // viewBox (the SVG hides overflow). Clamp by the (now length-bounded)
        // estimated half-width so the label always lands fully inside the canvas.
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

  // X-axis category labels (categorical bars only) — one centered under each
  // slot, STAGGERED across two rows (even indices upper, odd lower) so a label
  // clears its nearest same-row neighbour by two slots. That roughly doubles each
  // label's width budget over a single row, which is what lets two long
  // common-prefix names truncate to DISTINCT stubs ("Consumer Discretionary" /
  // "Consumer Staples" -> "Consumer Di…" / "Consumer St…") instead of colliding on
  // one "Consu…". The budget is the two-slot span capped by the distance to the
  // nearer viewBox edge, so a centered edge label is provably contained within the
  // canvas with no clamp/justification — and the conservative LABEL_CHAR_W upper
  // bound keeps that containment sound. Names past the (doubled) budget still
  // ellipsize, but the residual shrinks with cardinality and the full name always
  // reaches assistive tech via the aria description below.
  const xLabels =
    spec.categories !== undefined
      ? spec.categories
          .map((cat, i) => {
            const count = spec.categories!.length;
            const slot = (plotR - PAD) / count;
            const cx = PAD + (i + 0.5) * slot;
            // Half-width budget: the two-slot same-row spacing (a same-row
            // neighbour sits 2 slots away, so half-width <= one slot keeps them
            // apart), capped by the nearer viewBox edge so an edge label can't clip.
            // The x-axis band sits below the plot, clear of the y-tick gutter, so a
            // label may use the full width up to W (not just PLOT_R).
            const halfBudget = Math.min(slot, cx, W - cx);
            const maxChars = Math.max(1, Math.floor((2 * halfBudget) / LABEL_CHAR_W));
            const display = truncate(cat, maxChars);
            const y = H + X_LABEL_BASELINE + (i % 2) * X_LABEL_ROW_GAP;
            // When the name is truncated, carry the full label as a native hover
            // tooltip (SVG <title>). The tooltip is a pointer affordance only, but
            // the full text is ALSO in the figure's aria-label below (the accessible
            // name assistive tech reads), so the data is never hover-gated. We
            // deliberately don't make each label focusable — up to 16 axis labels
            // would add 16 tab stops, an a11y anti-pattern, for less than the
            // figure-level aria already gives.
            const tip = display !== cat ? `<title>${escapeXml(cat)}</title>` : "";
            return `<text class="chart-xlabel" x="${coord(cx)}" y="${coord(y)}" text-anchor="middle">${tip}${escapeXml(display)}</text>`;
          })
          .join("")
      : "";

  // A screen-reader description — the SVG is injected via v-html and is otherwise
  // invisible to assistive tech, so summarize each series. A time series gets its
  // span and direction; a categorical bar instead enumerates each category and
  // its value (a left-to-right "direction" is meaningless across categories).
  // Escape each agent-controlled field on its own (not the composed string) so a
  // later edit can't silently skip escaping; numeric values carry no metacharacters.
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

  // A category x-axis needs room below the plot; line/time charts keep H exactly.
  const vbH = spec.categories !== undefined ? H + X_AXIS_BAND : H;

  // A multi-series categorical bar is validated to exactly two series, one
  // emphasized, both non-blank and distinctly labeled (see parseSpec), so its legend always
  // renders two distinguishable, named swatches — ink for the base series, accent
  // for the emphasized one — saying which colour is which. It replaces the
  // per-series end label (suppressed above to avoid overlapping the bars).
  // Single-series categoricals show no legend: they lean on the title, and a
  // one-item legend would clutter the common case where the series label just
  // restates it. (Accepted residual: a lone series' optional qualifier then lives
  // only in the aria description, off-canvas.)
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
