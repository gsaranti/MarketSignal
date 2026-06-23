// Component tests for LatestReportView.vue — props in, one event out
// (export-markdown). Mock-free of Tauri (no @tauri-apps/api import); the single
// browser-global stub is `window.print`, which Tauri's macOS print shim replaces —
// it is a DOM global, not a Tauri mock, kept localized to the one PDF test. These
// pin: the four-way pane precedence (error > loadError > report > empty), the
// markdown-it rendering rules (heading/linkify, html:false, the table-scroll
// wrapper, the ```chart fence + its fallback), the toolbar label/tag, the
// canExport-driven export buttons, the export-markdown emit + busy label, the
// export-error strip, and the PDF print title-munging.

import { test, expect, vi, type VueWrapper } from "vitest";
import { mount } from "@vue/test-utils";
import LatestReportView from "../../src/components/LatestReportView.vue";
import { deepFreeze } from "../helpers/freeze";
import { localDate, localDateTime } from "../../src/format";
import type { GeneratedReport } from "../../src/types";

function makeReport(markdown: string, createdAt = "2026-06-08T13:00:00Z"): GeneratedReport {
  return {
    report_id: "abcdef12-3456-4789-8abc-def012345678",
    markdown,
    markdown_path: "/reports/2026-06-08-market-signal-weekly-report.md",
    summary: {
      report_id: "abcdef12-3456-4789-8abc-def012345678",
      report_type: "weekly_market",
      created_at: createdAt,
      title: "Rotation, not rupture",
      risk_posture: "risk-on",
      market_cycle: "late-cycle",
      thesis_stance: "bullish",
      header_summary_bullets: [],
      key_risks: [],
      unresolved_questions: [],
      forward_outlook_themes: [],
    },
  };
}

const baseProps = deepFreeze({
  report: null as GeneratedReport | null,
  error: null as string | null,
  loadError: null as string | null,
  isLatest: false,
  exportingMarkdown: false,
  exportError: null as string | null,
});

function makeWrapper(overrides: Partial<typeof baseProps> = {}) {
  return mount(LatestReportView, { props: { ...baseProps, ...overrides } });
}

function toolbarBtn(wrapper: VueWrapper, text: string) {
  const btn = wrapper.findAll(".toolbar-actions button").find((b) => b.text().includes(text));
  if (!btn) throw new Error(`no toolbar button containing "${text}"`);
  return btn;
}

test("pane precedence: a generation error outranks both a load error and a loaded report", () => {
  // Set all three competing inputs so the test pins the branch ORDER, not just
  // error-over-report: reversing the error/loadError branches would still show a
  // report-error block, but with the wrong label.
  const wrapper = makeWrapper({
    report: makeReport("# Body"),
    error: "model timed out",
    loadError: "file removed",
  });
  const block = wrapper.find(".report-error[role='alert']");
  expect(block.find(".report-error-label").text()).toBe("Generation failed");
  expect(block.find(".report-error-detail").text()).toBe("model timed out");
  expect(wrapper.find(".report-article").exists()).toBe(false);
});

test("pane precedence: a load error outranks a loaded report", () => {
  const wrapper = makeWrapper({ report: makeReport("# Body"), loadError: "file removed" });
  expect(wrapper.find(".report-error-label").text()).toBe("Couldn't open this report");
  expect(wrapper.find(".report-article").exists()).toBe(false);
});

test("the empty state shows when there is no report and no error", () => {
  const wrapper = makeWrapper();
  expect(wrapper.find(".report-empty").exists()).toBe(true);
  expect(wrapper.find(".report-empty-body").text()).toContain("No issue has been generated yet");
});

test("renders markdown: headings + linkified URLs, with raw HTML neutralized (html:false)", () => {
  const md = ["## Heading two", "", "Visit https://example.com today.", "", "<b>rawbold</b>"].join("\n");
  const article = makeWrapper({ report: makeReport(md) }).find(".report-article");
  expect(article.find("h2").text()).toBe("Heading two");
  expect(article.find("a").attributes("href")).toContain("example.com");
  // html:false escapes inline HTML, so no live <b> element is produced.
  expect(article.find("b").exists()).toBe(false);
});

test("wide tables are wrapped in a local horizontal-scroll container", () => {
  const md = ["| A | B |", "| --- | --- |", "| 1 | 2 |"].join("\n");
  const article = makeWrapper({ report: makeReport(md) }).find(".report-article");
  expect(article.find(".prose-table-wrap table").exists()).toBe(true);
});

test("a valid ```chart fence renders as an inline SVG figure; a malformed one falls back to a code block", () => {
  const valid = ["```chart", '{"type":"line","series":[{"points":[1,2,3,4],"emphasis":false}]}', "```"].join("\n");
  const validArticle = makeWrapper({ report: makeReport(valid) }).find(".report-article");
  expect(validArticle.find("svg.chart-svg").exists()).toBe(true);

  // No `series` -> renderChart returns null -> the fence renders as a plain code block.
  const malformed = ["```chart", '{"type":"line"}', "```"].join("\n");
  const malformedArticle = makeWrapper({ report: makeReport(malformed) }).find(".report-article");
  expect(malformedArticle.find("svg.chart-svg").exists()).toBe(false);
  expect(malformedArticle.find("pre").exists()).toBe(true);
});

test("the toolbar label reflects the loaded issue, and the Latest tag is gated on isLatest", () => {
  const report = makeReport("# Body");
  const latest = makeWrapper({ report, isLatest: true });
  // The label is the issue headline; the date-time + short id ride in the meta.
  expect(latest.find(".toolbar-label").text()).toBe("Rotation, not rupture");
  expect(latest.find(".toolbar-meta").text()).toBe(
    `${localDateTime(report.summary.created_at)} · #abcdef12`
  );
  expect(latest.find(".toolbar-tag").exists()).toBe(true);

  expect(makeWrapper({ report, isLatest: false }).find(".toolbar-tag").exists()).toBe(false);
  // No report -> the static fallback label, no tag.
  const none = makeWrapper();
  expect(none.find(".toolbar-label").text()).toBe("Latest report");
  expect(none.find(".toolbar-tag").exists()).toBe(false);
});

test("export buttons are enabled only when a report is on screen (canExport)", () => {
  const enabled = makeWrapper({ report: makeReport("# Body") });
  expect(toolbarBtn(enabled, "Export PDF").attributes("disabled")).toBeUndefined();
  expect(toolbarBtn(enabled, "Share as Markdown").attributes("disabled")).toBeUndefined();

  // A load error means no report is really shown -> both disabled.
  const blocked = makeWrapper({ report: makeReport("# Body"), loadError: "file removed" });
  expect(toolbarBtn(blocked, "Export PDF").attributes("disabled")).toBeDefined();
  expect(toolbarBtn(blocked, "Share as Markdown").attributes("disabled")).toBeDefined();
});

test("Share as Markdown emits export-markdown and reports its busy label", async () => {
  const wrapper = makeWrapper({ report: makeReport("# Body") });
  await toolbarBtn(wrapper, "Share as Markdown").trigger("click");
  expect(wrapper.emitted("export-markdown")).toHaveLength(1);

  const busy = makeWrapper({ report: makeReport("# Body"), exportingMarkdown: true });
  const btn = toolbarBtn(busy, "Saving…");
  expect(btn.text()).toContain("Saving…");
  expect(btn.attributes("disabled")).toBeDefined();
});

test("an export failure shows a non-destructive alert strip without unmounting the report", () => {
  const wrapper = makeWrapper({ report: makeReport("# Body"), exportError: "disk full" });
  const strip = wrapper.find(".export-error[role='alert']");
  expect(strip.text()).toContain("Couldn't export: disk full");
  // The report stays on screen — export is an action, not a load.
  expect(wrapper.find(".report-article").exists()).toBe(true);
});

test("Export PDF seeds the print title with the spec basename and restores it after", async () => {
  const report = makeReport("# Body");
  const wrapper = makeWrapper({ report });
  const expectedBase = `${localDate(report.summary.created_at)}-market-signal-report`;

  const original = window.print;
  const originalTitle = document.title;
  document.title = "Market Signal";
  let titleDuringPrint = "";
  const printSpy = vi.fn(() => {
    titleDuringPrint = document.title;
    return Promise.resolve();
  });
  (window as unknown as { print: typeof window.print }).print =
    printSpy as unknown as typeof window.print;
  try {
    await toolbarBtn(wrapper, "Export PDF").trigger("click");
    await Promise.resolve(); // let exportPdf's finally run after the awaited print
    expect(printSpy).toHaveBeenCalledTimes(1);
    expect(titleDuringPrint).toBe(expectedBase);
    expect(document.title).toBe("Market Signal"); // exportPdf restored its own change
  } finally {
    // Restore BOTH globals this test mutated, so nothing leaks to later tests.
    (window as unknown as { print: typeof window.print }).print = original;
    document.title = originalTitle;
  }
});
