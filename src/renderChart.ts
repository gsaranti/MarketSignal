// Render a fenced ```chart block's JSON body into a restrained inline-SVG line
// figure, generalizing the design system's YieldChart reference
// (market-signal-design-system/.../market_signal_desktop/LatestReport.jsx):
// a monochrome ink series, at most one accent-emphasized line, a hairline grid,
// no fills / markers / shadows. Styling lives in colors_and_type.css under
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

interface ChartSpec {
  title?: string;
  series: ChartSeries[];
}

// Robustness caps — the journal-figure register is a handful of series over a
// bounded run, not a dashboard. Mirrors the reference's 2 series / 26 points.
const MAX_SERIES = 3;
const MAX_POINTS = 120;
const MIN_POINTS = 2;

// SVG geometry in viewBox units — mirrors the reference (720×130, 8px padding).
// The SVG scales uniformly to the reading column (width:100%, height:auto in
// CSS); strokes are pinned crisp via vector-effect:non-scaling-stroke there.
const W = 720;
const H = 130;
const PAD = 8;

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
  if (obj.type !== "line") return null;
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
  return { title, series };
}

// Compact coordinate — 2 decimals, no trailing-zero noise in the path string.
function coord(n: number): string {
  return (Math.round(n * 100) / 100).toString();
}

// Y-axis tick label: integer for index-scale levels, 2 decimals for rates/spreads.
function formatTick(v: number): string {
  return Math.abs(v) >= 100 ? Math.round(v).toString() : v.toFixed(2);
}

function buildSvg(spec: ChartSpec): string {
  const all = spec.series.flatMap((s) => s.points);
  let min = Math.min(...all);
  let max = Math.max(...all);
  if (min === max) {
    // A flat series — pad symmetrically so it renders as a centered line.
    min -= 1;
    max += 1;
  }
  const headroom = (max - min) * 0.08;
  min -= headroom;
  max += headroom;

  const y = (v: number): number =>
    PAD + (1 - (v - min) / (max - min)) * (H - 2 * PAD);
  const xScale =
    (count: number) =>
    (i: number): number =>
      count <= 1 ? W / 2 : PAD + i * ((W - 2 * PAD) / (count - 1));

  // 4 hairline horizontal grid lines, matching the reference.
  const grid = [0, 1, 2, 3]
    .map((i) => {
      const gy = PAD + (i * (H - 2 * PAD)) / 3;
      return `<line class="chart-grid" x1="0" x2="${W}" y1="${coord(gy)}" y2="${coord(gy)}" />`;
    })
    .join("");

  // Draw the emphasized series last so the accent line sits on top.
  const ordered = [...spec.series].sort(
    (a, b) => Number(a.emphasis) - Number(b.emphasis),
  );

  const paths = ordered
    .map((s) => {
      const x = xScale(s.points.length);
      const d = s.points
        .map((v, i) => `${i === 0 ? "M" : "L"}${coord(x(i))} ${coord(y(v))}`)
        .join(" ");
      const cls = s.emphasis ? "chart-line chart-line--accent" : "chart-line";
      return `<path class="${cls}" d="${d}" />`;
    })
    .join("");

  // ~3 y-axis tick labels — top / middle / bottom of the padded domain.
  const ticks = [max, (max + min) / 2, min]
    .map((v) => {
      const ty = y(v) + 3;
      return `<text class="chart-tick" x="${W - 4}" y="${coord(ty)}" text-anchor="end">${escapeXml(formatTick(v))}</text>`;
    })
    .join("");

  // End-of-series labels at the right edge. All series share one length (enforced
  // in validation), so every endpoint sits at the same x. Each label starts near
  // its line — the emphasized one just above, the rest just below (reference
  // idiom) — then ALL of them (accent included) are decluttered together so no
  // two stack, and the cluster is nudged back inside the canvas if it would clip
  // off either edge (frontend-craft overflow handling).
  const lastI = spec.series[0].points.length - 1;
  const endX = coord(xScale(spec.series[0].points.length)(lastI) - 4);

  const labels = spec.series
    .filter((s) => s.label !== undefined)
    .map((s) => ({
      text: escapeXml(s.label as string),
      cls: s.emphasis ? "chart-endlabel chart-endlabel--accent" : "chart-endlabel",
      y: y(s.points[lastI]) + (s.emphasis ? -5 : 12),
    }));
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
        `<text class="${lbl.cls}" x="${endX}" y="${coord(lbl.y)}" text-anchor="end">${lbl.text}</text>`,
    )
    .join("");

  // A screen-reader description — the SVG is injected via v-html and is otherwise
  // invisible to assistive tech, so summarize each series' span and direction;
  // an unlabeled chart then still announces its data, not just "Line chart".
  // Escape each agent-controlled field on its own (not the composed string) so a
  // later edit can't silently skip escaping; numeric spans carry no metacharacters.
  const safeTitle = spec.title !== undefined ? escapeXml(spec.title) : "";
  const seriesDesc = spec.series
    .map((s, i) => {
      const name = escapeXml(s.label ?? `series ${i + 1}`);
      const start = s.points[0];
      const end = s.points[s.points.length - 1];
      const direction = end > start ? "rising" : end < start ? "falling" : "flat";
      return `${name} from ${formatTick(start)} to ${formatTick(end)}, ${direction}`;
    })
    .join("; ");
  const aria = `Line chart${safeTitle ? `: ${safeTitle}` : ""}. ${seriesDesc}`;

  const caption =
    spec.title !== undefined
      ? `<figcaption class="chart-caption">${escapeXml(spec.title)}</figcaption>`
      : "";

  return (
    `<figure class="chart-figure">` +
    `<svg class="chart-svg" viewBox="0 0 ${W} ${H}" role="img" aria-label="${aria}">` +
    grid +
    paths +
    ticks +
    endLabels +
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
