// Unit tests for the pure chart renderer. Run via `npm test` — Node's built-in
// test runner imports the TypeScript source directly through type-stripping (no
// build step, no extra dependency). renderChart is a pure `string -> string|null`
// function, so these assert the SVG geometry, the validation/fail-soft contract,
// the accessibility output, and the v-html escaping that the markdown-it fence
// rule relies on. They replace the throwaway node smoke the chart slices used.

import { test } from "node:test";
import assert from "node:assert/strict";
import { renderChart } from "../src/renderChart.ts";

// --- helpers -----------------------------------------------------------------

const spec = (o: Record<string, unknown>): string => JSON.stringify(o);

// Every <rect>'s y + height (attribute order in the renderer is class,x,y,w,h).
function rects(svg: string): Array<{ y: number; h: number }> {
  const out: Array<{ y: number; h: number }> = [];
  const re = /<rect\b[^>]*\by="([-\d.]+)"[^>]*\bheight="([-\d.]+)"/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(svg)) !== null) out.push({ y: +m[1], h: +m[2] });
  return out;
}

const baselineY = (svg: string): number | null => {
  const m = svg.match(/<line class="chart-baseline"[^>]*\by1="([-\d.]+)"/);
  return m ? +m[1] : null;
};

const ariaLabel = (svg: string): string => svg.match(/aria-label="([^"]*)"/)?.[1] ?? "";

// The three y-tick label values (top / middle / bottom of the padded domain).
function tickValues(svg: string): number[] {
  return [...svg.matchAll(/<text class="chart-tick"[^>]*>([^<]+)<\/text>/g)].map(
    (m) => +m[1],
  );
}

const near = (a: number, b: number, eps = 0.6): boolean => Math.abs(a - b) <= eps;

// Each chart-xlabel's x, text-anchor, and text content, in document order.
function xLabels(svg: string): Array<{ x: number; anchor: string; text: string }> {
  const out: Array<{ x: number; anchor: string; text: string }> = [];
  const re =
    /<text class="chart-xlabel"[^>]*\bx="([-\d.]+)"[^>]*text-anchor="(\w+)"[^>]*>([^<]*)<\/text>/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(svg)) !== null) out.push({ x: +m[1], anchor: m[2], text: m[3] });
  return out;
}

const viewBoxH = (svg: string): number =>
  +(svg.match(/viewBox="0 0 \d+ (\d+)"/)?.[1] ?? "0");

// --- line --------------------------------------------------------------------

test("line: renders a stroked path, no fill / bar / baseline", () => {
  const out = renderChart(
    spec({ type: "line", title: "10Y vs 2Y", series: [{ label: "10Y", points: [4.1, 4.2, 4.3] }] }),
  );
  assert.ok(out !== null);
  assert.match(out, /class="chart-line"/);
  assert.doesNotMatch(out, /<rect/);
  assert.doesNotMatch(out, /chart-baseline/);
  assert.doesNotMatch(out, /chart-area/);
  assert.equal(ariaLabel(out), "Line chart: 10Y vs 2Y. 10Y from 4.10 to 4.30, rising");
});

test("line: keeps its data-fitted domain (not zero-anchored)", () => {
  const out = renderChart(spec({ type: "line", series: [{ points: [4400, 4500, 4600] }] }));
  assert.ok(out !== null);
  // A level series far from zero must not be squashed against a 0..4600 axis.
  assert.ok(Math.min(...tickValues(out)) > 1000);
});

test("line: emphasized series is drawn last (accent on top)", () => {
  const out = renderChart(
    spec({
      type: "line",
      series: [{ points: [1, 2, 3] }, { points: [3, 2, 1], emphasis: true }],
    }),
  );
  assert.ok(out !== null);
  assert.ok(out.indexOf("chart-line chart-line--accent") > out.indexOf('class="chart-line"'));
});

// --- bar ---------------------------------------------------------------------

test("bar: signed bars grow up / down from a zero baseline", () => {
  const out = renderChart(spec({ type: "bar", series: [{ points: [3, -2] }] }));
  assert.ok(out !== null);
  const y0 = baselineY(out);
  assert.ok(y0 !== null, "baseline present");
  const r = rects(out);
  assert.equal(r.length, 2);
  // Positive bar: bottom edge (y + h) sits on the baseline, growing up.
  assert.ok(near(r[0].y + r[0].h, y0), "positive bar bottom at baseline");
  // Negative bar: top edge (y) sits on the baseline, growing down.
  assert.ok(near(r[1].y, y0), "negative bar top at baseline");
});

test("bar: domain is zero-anchored even when all points are positive", () => {
  const out = renderChart(spec({ type: "bar", series: [{ points: [2, 5, 3] }] }));
  assert.ok(out !== null);
  // include-zero means the bottom tick is at / below 0.
  assert.ok(Math.min(...tickValues(out)) <= 0);
});

test("bar: aria announces the chart type", () => {
  const out = renderChart(spec({ type: "bar", title: "Weekly net change", series: [{ points: [1, -1] }] }));
  assert.ok(out !== null);
  assert.match(out, /aria-label="Bar chart: Weekly net change\./);
});

test("bar: end label centers over its bar (anchor middle), not the right edge", () => {
  const out = renderChart(spec({ type: "bar", series: [{ label: "SPX", points: [1, 2] }] }));
  assert.ok(out !== null);
  const m = out.match(/<text class="chart-endlabel"[^>]*\bx="([-\d.]+)"[^>]*text-anchor="(\w+)"/);
  assert.ok(m, "bar end label present");
  assert.equal(m[2], "middle");
  // The last bar lives in the right slot but its center is well left of the right
  // margin (~708); the label must sit with the bar, not float off in the margin.
  assert.ok(+m[1] < 690, `label x ${m[1]} should track the bar, not the right edge`);
});

test("bar: end label stays within the canvas at high point counts", () => {
  // The last bar's center nears x≈W (=720) as the point count grows; the centered
  // label must be clamped so it doesn't clip past the viewBox edge.
  const points = Array.from({ length: 120 }, (_, i) => (i % 2 ? 1 : -1));
  const out = renderChart(spec({ type: "bar", series: [{ label: "Energy", points }] }));
  assert.ok(out !== null);
  const m = out.match(/<text class="chart-endlabel"[^>]*\bx="([-\d.]+)"/);
  assert.ok(m, "bar end label present");
  const x = +m[1];
  const halfW = ("Energy".length * 10) / 2; // 10 = LABEL_CHAR_W upper bound
  assert.ok(x + halfW <= 720, `label right extent ${x + halfW} must stay within the viewBox`);
  assert.ok(x - halfW >= 0, "label left extent must stay within the viewBox");
});

test("truncates a long, wide-glyph label and keeps it within the canvas", () => {
  // Worst case: the widest glyph (W) AND the last bar near the right edge (many
  // points). LABEL_CHAR_W is an upper bound on glyph advance, so asserting against
  // it is a sound worst-case containment check — rendered-pixel fit is GUI-pass.
  const long = "W".repeat(200);
  const points = Array.from({ length: 120 }, () => 1);
  const out = renderChart(spec({ type: "bar", series: [{ label: long, points }] }));
  assert.ok(out !== null);
  const m = out.match(/<text class="chart-endlabel"[^>]*\bx="([-\d.]+)"[^>]*>([^<]*)<\/text>/);
  assert.ok(m, "end label present");
  const x = +m[1];
  const text = m[2];
  assert.ok(text.endsWith("…") && text.length <= 24, "label is ellipsized to a bounded width");
  const halfW = (text.length * 10) / 2; // 10 = LABEL_CHAR_W upper bound
  assert.ok(x - halfW >= 0 && x + halfW <= 720, `bounded label stays within the viewBox (x=${x})`);
  // The full label still reaches assistive tech via the aria description.
  assert.ok(ariaLabel(out).includes(long), "aria keeps the full untruncated label");
});

test("bar: multi-series renders base + accent bars, one rect per point per series", () => {
  const out = renderChart(
    spec({ type: "bar", series: [{ points: [1, 2, 3] }, { points: [-1, 0, 2], emphasis: true }] }),
  );
  assert.ok(out !== null);
  assert.match(out, /class="chart-bar"/);
  assert.match(out, /class="chart-bar chart-bar--accent"/);
  assert.equal(rects(out).length, 6);
});

// --- area --------------------------------------------------------------------

test("area: closed filled path + top stroke + baseline", () => {
  const out = renderChart(spec({ type: "area", title: "HY OAS", series: [{ label: "OAS", points: [3.1, 3.4, 3.2] }] }));
  assert.ok(out !== null);
  assert.match(out, /class="chart-area"/);
  assert.match(out, /Z" \/>/, "fill path is closed with Z");
  assert.match(out, /class="chart-line"/, "crisp top stroke drawn over the fill");
  assert.ok(baselineY(out) !== null);
  assert.match(out, /aria-label="Area chart: HY OAS\./);
});

test("area: end labels ride the right edge (anchor end)", () => {
  const out = renderChart(spec({ type: "area", series: [{ label: "OAS", points: [1, 2] }] }));
  assert.ok(out !== null);
  assert.match(out, /<text class="chart-endlabel"[^>]*text-anchor="end"/);
});

// --- categorical bar (optional x-axis category labels) -----------------------

test("categorical bar: one centered x-axis label per category, left-to-right", () => {
  const out = renderChart(
    spec({
      type: "bar",
      title: "Sector returns",
      categories: ["Tech", "Energy", "Financials"],
      series: [{ points: [2.1, -1.4, 0.6] }],
    }),
  );
  assert.ok(out !== null);
  const labels = xLabels(out);
  assert.deepEqual(
    labels.map((l) => l.text),
    ["Tech", "Energy", "Financials"],
  );
  assert.ok(labels.every((l) => l.anchor === "middle"));
  // Centered under evenly-divided slots: strictly increasing, all inside canvas.
  assert.ok(labels[0].x < labels[1].x && labels[1].x < labels[2].x);
  assert.ok(labels[0].x > 0 && labels[2].x < 720);
});

test("categorical bar: enumerated aria (category/value pairs, no direction)", () => {
  const out = renderChart(
    spec({
      type: "bar",
      title: "Sector returns",
      categories: ["Tech", "Energy"],
      series: [{ points: [2.1, -1.4] }],
    }),
  );
  assert.ok(out !== null);
  const aria = ariaLabel(out);
  assert.match(aria, /^Bar chart: Sector returns\. /);
  assert.ok(aria.includes("Tech 2.10") && aria.includes("Energy -1.40"));
  // A left-to-right "direction" is meaningless across categories.
  assert.doesNotMatch(aria, /rising|falling|flat/);
});

test("categorical bar: taller viewBox for the label band; non-categorical keeps H", () => {
  const cat = renderChart(
    spec({ type: "bar", categories: ["A", "B"], series: [{ points: [1, 2] }] }),
  );
  const plain = renderChart(spec({ type: "bar", series: [{ points: [1, 2] }] }));
  assert.ok(cat !== null && plain !== null);
  assert.equal(viewBoxH(plain), 130, "non-categorical bar keeps the plain height");
  assert.ok(viewBoxH(cat) > viewBoxH(plain), "categorical figure reserves an x-axis band");
});

test("categorical bar: still a zero-anchored bar (categories only relabel slots)", () => {
  const out = renderChart(
    spec({ type: "bar", categories: ["A", "B"], series: [{ points: [3, -2] }] }),
  );
  assert.ok(out !== null);
  assert.ok(baselineY(out) !== null, "baseline present");
  assert.equal(rects(out).length, 2);
});

test("categorical bar: dense wide labels truncate but stay within the canvas", () => {
  const categories = Array.from({ length: 16 }, () => "W".repeat(40));
  const points = categories.map(() => 1);
  const out = renderChart(spec({ type: "bar", categories, series: [{ points }] }));
  assert.ok(out !== null);
  const labels = xLabels(out);
  assert.equal(labels.length, 16);
  for (const l of labels) {
    assert.ok(l.text.endsWith("…"), "wide label is ellipsized");
    const halfW = (l.text.length * 10) / 2; // 10 = LABEL_CHAR_W upper bound
    assert.ok(
      l.x - halfW >= 0 && l.x + halfW <= 720,
      `label at x=${l.x} stays within the viewBox`,
    );
  }
});

test("categorical bar: multi-series enumerates each series in aria, one label per slot", () => {
  const out = renderChart(
    spec({
      type: "bar",
      categories: ["A", "B"],
      series: [
        { label: "This week", points: [1, 2] },
        { label: "Last week", points: [0, 3], emphasis: true },
      ],
    }),
  );
  assert.ok(out !== null);
  const aria = ariaLabel(out);
  assert.ok(aria.includes("This week: A 1.00, B 2.00"));
  assert.ok(aria.includes("Last week: A 0.00, B 3.00"));
  assert.equal(xLabels(out).length, 2, "one x-label per slot, not per bar");
  assert.equal(rects(out).length, 4, "two series x two slots");
});

test("categorical bar: trims surrounding whitespace so it can't eat the label budget", () => {
  const out = renderChart(
    spec({ type: "bar", categories: ["  Tech", "Energy  "], series: [{ points: [1, 2] }] }),
  );
  assert.ok(out !== null);
  assert.deepEqual(
    xLabels(out).map((l) => l.text),
    ["Tech", "Energy"],
  );
  assert.ok(ariaLabel(out).includes("Tech 1.00") && ariaLabel(out).includes("Energy 2.00"));
});

test("categorical bar: escapes category labels in svg text and aria (v-html safety)", () => {
  const out = renderChart(
    spec({ type: "bar", categories: ['<b>x', '"y'], series: [{ points: [1, 2] }] }),
  );
  assert.ok(out !== null);
  assert.doesNotMatch(out, /<b>x/);
  assert.doesNotMatch(ariaLabel(out), /[<>"]/);
});

// --- validation / fail-soft (every bad spec -> null -> code-block fallback) ---

test("rejects malformed / out-of-contract specs", () => {
  const bad: Array<[string, string]> = [
    ["malformed JSON", "{not json"],
    ["non-object", "42"],
    ["missing type", spec({ series: [{ points: [1, 2] }] })],
    ["unknown type", spec({ type: "scatter", series: [{ points: [1, 2] }] })],
    ["empty series", spec({ type: "line", series: [] })],
    ["series not array", spec({ type: "line", series: {} })],
    ["too many series", spec({ type: "line", series: [{ points: [1, 2] }, { points: [1, 2] }, { points: [1, 2] }, { points: [1, 2] }] })],
    ["too few points", spec({ type: "bar", series: [{ points: [1] }] })],
    ["non-finite point (null)", spec({ type: "line", series: [{ points: [1, null] }] })],
    ["non-numeric point", spec({ type: "line", series: [{ points: [1, "x"] }] })],
    ["unequal lengths", spec({ type: "bar", series: [{ points: [1, 2] }, { points: [1, 2, 3] }] })],
    ["two emphasis", spec({ type: "area", series: [{ points: [1, 2], emphasis: true }, { points: [3, 4], emphasis: true }] })],
    ["non-string label", spec({ type: "line", series: [{ label: 5, points: [1, 2] }] })],
  ];
  for (const [name, body] of bad) assert.equal(renderChart(body), null, name);
});

test("rejects invalid categories specs", () => {
  const bad: Array<[string, string]> = [
    ["categories on line", spec({ type: "line", categories: ["A", "B"], series: [{ points: [1, 2] }] })],
    ["categories on area", spec({ type: "area", categories: ["A", "B"], series: [{ points: [1, 2] }] })],
    ["empty categories", spec({ type: "bar", categories: [], series: [{ points: [1, 2] }] })],
    ["categories not array", spec({ type: "bar", categories: "A,B", series: [{ points: [1, 2] }] })],
    ["length mismatch", spec({ type: "bar", categories: ["A", "B", "C"], series: [{ points: [1, 2] }] })],
    ["non-string category", spec({ type: "bar", categories: ["A", 2], series: [{ points: [1, 2] }] })],
    ["empty-string category", spec({ type: "bar", categories: ["A", ""], series: [{ points: [1, 2] }] })],
    ["whitespace-only category", spec({ type: "bar", categories: ["A", "  "], series: [{ points: [1, 2] }] })],
    [
      "too many categories",
      spec({
        type: "bar",
        categories: Array.from({ length: 17 }, (_, i) => `c${i}`),
        series: [{ points: Array.from({ length: 17 }, (_, i) => i) }],
      }),
    ],
  ];
  for (const [name, body] of bad) assert.equal(renderChart(body), null, name);
});

test("accepts up to MAX_POINTS, rejects beyond", () => {
  const ok = Array.from({ length: 120 }, (_, i) => i);
  const tooMany = Array.from({ length: 121 }, (_, i) => i);
  assert.ok(renderChart(spec({ type: "line", series: [{ points: ok }] })) !== null);
  assert.equal(renderChart(spec({ type: "line", series: [{ points: tooMany }] })), null);
});

// --- escaping & title normalization ------------------------------------------

test("escapes agent-controlled title and label (v-html safety)", () => {
  const out = renderChart(
    spec({
      type: "bar",
      title: "<script>alert(1)</script>",
      series: [{ label: '"><img onerror=x>', points: [1, 2] }],
    }),
  );
  assert.ok(out !== null);
  assert.doesNotMatch(out, /<script/);
  assert.doesNotMatch(out, /<img/);
  // The raw quote/bracket must not break out of the aria-label attribute.
  assert.doesNotMatch(ariaLabel(out), /[<>"]/);
});

test("blank / whitespace title renders no caption and no dangling aria", () => {
  const out = renderChart(spec({ type: "line", title: "   ", series: [{ points: [1, 2] }] }));
  assert.ok(out !== null);
  assert.doesNotMatch(out, /figcaption/);
  // No "Line chart: ." with an empty title.
  assert.match(ariaLabel(out), /^Line chart\. /);
});
