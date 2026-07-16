// Component tests for RecentReportsSidebar.vue — props in, two events out
// (navigate / select). Mock-free like ResearchDocuments/Settings. These pin: the
// populated report list (one row each, the current-selection marking + aria-current,
// the select emit), the empty/failed fallback row (the "No reports yet" vs
// "Couldn't load reports" copy, the error styling + title, the navigate emit), and
// the bottom nav (active marking, count badges shown only when non-zero, navigate
// payloads).

import { test, expect } from "vitest";
import { mount, type VueWrapper } from "@vue/test-utils";
import RecentReportsSidebar from "../../src/components/RecentReportsSidebar.vue";
import { deepFreeze } from "../helpers/freeze";
import { localDateTime } from "../../src/format";
import type { PortfolioRunSummary, ReportSummary } from "../../src/types";

function summary(id: string, createdAt: string, title = "Sample headline"): ReportSummary {
  return {
    report_id: id,
    report_type: "weekly_market",
    created_at: createdAt,
    title,
    risk_posture: "mixed",
    market_cycle: "late-cycle",
    thesis_stance: "uncertain",
    header_summary_bullets: [],
    key_risks: [],
    unresolved_questions: [],
    forward_outlook_themes: [],
  };
}

// Two reports, newest first — read-only across wrappers, so freeze them.
const reports: ReportSummary[] = deepFreeze([
  summary("11111111-aaaa-4bbb-8ccc-000000000001", "2026-06-08T13:00:00Z", "Rotation, not rupture"),
  summary("22222222-aaaa-4bbb-8ccc-000000000002", "2026-06-01T13:00:00Z", "Credit cracks widen"),
]);

// Two portfolio runs, newest first — the swap-in history for the Portfolio view.
const portfolioRuns: PortfolioRunSummary[] = deepFreeze([
  {
    run_id: "prun-2",
    created_at: "2026-07-12T09:00:00Z",
    holdings_count: 23,
    graded_count: 19,
  },
  {
    run_id: "prun-1",
    created_at: "2026-07-05T09:00:00Z",
    holdings_count: 1,
    graded_count: 0,
  },
]);

const baseProps = deepFreeze({
  reports,
  selectedReportId: null as string | null,
  reportsError: null as string | null,
  portfolioRuns,
  selectedRunId: null as string | null,
  portfolioRunsError: null as string | null,
  view: "report" as const,
  inboxCount: 0,
  archiveCount: 0,
});

function makeWrapper(overrides: Partial<typeof baseProps> = {}) {
  return mount(RecentReportsSidebar, { props: { ...baseProps, ...overrides } });
}

function navItemByLabel(wrapper: VueWrapper, label: string) {
  const item = wrapper
    .findAll(".nav-item")
    .find((b) => b.find(".nav-label").text() === label);
  if (!item) throw new Error(`no nav item labelled "${label}"`);
  return item;
}

test("renders one report row per report, dated locally with a short id", () => {
  const wrapper = makeWrapper();
  const rows = wrapper.findAll(".sidebar-list .report-row");
  expect(rows).toHaveLength(2);
  // The row is labeled by the agent's per-issue headline, not the product name.
  expect(rows[0].find(".row-title").text()).toBe("Rotation, not rupture");
  expect(rows[0].find(".row-meta").text()).toBe(
    `${localDateTime(reports[0].created_at)} · #11111111`
  );
});

test("falls back to the product name when a report has no title (legacy row)", () => {
  const legacy = summary("33333333-aaaa-4bbb-8ccc-000000000003", "2026-05-25T13:00:00Z", "");
  const wrapper = makeWrapper({ reports: [legacy] });
  const rows = wrapper.findAll(".sidebar-list .report-row");
  expect(rows[0].find(".row-title").text()).toBe("Market Signal Report");
});

test("marks the selected row current (class + aria-current) only on the report view", () => {
  const selected = reports[1].report_id;
  const wrapper = makeWrapper({ view: "report", selectedReportId: selected });
  const rows = wrapper.findAll(".sidebar-list .report-row");
  expect(rows[1].classes()).toContain("is-current");
  expect(rows[1].attributes("aria-current")).toBe("true");
  expect(rows[0].attributes("aria-current")).toBeUndefined();

  // Same selection but viewing another surface: nothing is marked current.
  const elsewhere = makeWrapper({ view: "settings", selectedReportId: selected });
  expect(elsewhere.findAll(".sidebar-list .report-row")[1].attributes("aria-current"))
    .toBeUndefined();
});

test("selecting a row emits select with the report id", async () => {
  const wrapper = makeWrapper();
  await wrapper.findAll(".sidebar-list .report-row")[0].trigger("click");
  expect(wrapper.emitted("select")).toEqual([[reports[0].report_id]]);
});

test("the empty state shows a single 'No reports yet' row that navigates to the report view", async () => {
  const wrapper = makeWrapper({ reports: [] });
  const rows = wrapper.findAll(".sidebar-list .report-row");
  expect(rows).toHaveLength(1);
  const meta = rows[0].find(".row-meta");
  expect(meta.text()).toBe("No reports yet");
  expect(meta.classes()).not.toContain("is-error");

  await rows[0].trigger("click");
  expect(wrapper.emitted("navigate")).toEqual([["report"]]);
});

test("the empty-state row marks itself current (class + aria-current) only on the report view", () => {
  // On the report view the (empty) library row is the current surface...
  const onReport = makeWrapper({ reports: [], view: "report" });
  const reportRow = onReport.find(".sidebar-list .report-row");
  expect(reportRow.classes()).toContain("is-current");
  expect(reportRow.attributes("aria-current")).toBe("true");

  // ...but viewing another surface leaves it unmarked.
  const elsewhere = makeWrapper({ reports: [], view: "inbox" });
  const elsewhereRow = elsewhere.find(".sidebar-list .report-row");
  expect(elsewhereRow.classes()).not.toContain("is-current");
  expect(elsewhereRow.attributes("aria-current")).toBeUndefined();
});

test("a listing failure (no list to fall back on) reads as an error, not an empty library", () => {
  const wrapper = makeWrapper({ reports: [], reportsError: "permission denied" });
  const row = wrapper.find(".sidebar-list .report-row");
  const meta = row.find(".row-meta");
  expect(meta.text()).toBe("Couldn't load reports");
  expect(meta.classes()).toContain("is-error");
  expect(row.attributes("title")).toBe("permission denied");
});

// --- Portfolio-runs history (the shared-history swap per feature) ------------
// On the Portfolio view the same sidebar swaps its history content to the
// retained Portfolio runs (design kit Sidebar.jsx; docs/interface.md §Main
// Layout) — same rows, same selection accent, run-flavored copy.

test("the Portfolio view swaps the history list to portfolio runs", () => {
  const wrapper = makeWrapper({ view: "portfolio" });
  expect(wrapper.find(".sidebar-header").text()).toBe("Portfolio runs · last 10");
  const rows = wrapper.findAll(".sidebar-list .report-row");
  expect(rows).toHaveLength(2);
  expect(rows[0].find(".row-title").text()).toBe("Full book · 23 holdings");
  expect(rows[0].find(".row-meta").text()).toBe(
    `${localDateTime(portfolioRuns[0].created_at)} · rated 19`
  );
  // The report list is not rendered on this view.
  expect(wrapper.find(".sidebar-header").text()).not.toContain("Recent reports");
});

test("every other view keeps the report history visible", () => {
  for (const view of ["report", "inbox", "archive", "settings"] as const) {
    const wrapper = makeWrapper({ view });
    expect(wrapper.find(".sidebar-header").text()).toBe("Recent reports · last 30");
  }
});

test("selecting a run row emits select-run and the selected run is marked current", async () => {
  const wrapper = makeWrapper({ view: "portfolio", selectedRunId: "prun-2" });
  const rows = wrapper.findAll(".sidebar-list .report-row");
  expect(rows[0].classes()).toContain("is-current");
  expect(rows[0].attributes("aria-current")).toBe("true");
  expect(rows[1].attributes("aria-current")).toBeUndefined();

  await rows[1].trigger("click");
  expect(wrapper.emitted("select-run")).toEqual([["prun-1"]]);
});

test("the runs empty state and list failure mirror the report list's posture", async () => {
  const empty = makeWrapper({ view: "portfolio", portfolioRuns: [] });
  const emptyRow = empty.find(".sidebar-list .report-row");
  expect(emptyRow.find(".row-meta").text()).toBe("No runs yet");
  await emptyRow.trigger("click");
  expect(empty.emitted("navigate")).toEqual([["portfolio"]]);

  const failed = makeWrapper({
    view: "portfolio",
    portfolioRuns: [],
    portfolioRunsError: "db locked",
  });
  const failedMeta = failed.find(".sidebar-list .report-row .row-meta");
  expect(failedMeta.text()).toBe("Couldn't load runs");
  expect(failedMeta.classes()).toContain("is-error");
});

test("nav items mark the active view and emit navigate with its key", async () => {
  const wrapper = makeWrapper({ view: "inbox" });
  const inbox = navItemByLabel(wrapper, "Research Inbox");
  expect(inbox.classes()).toContain("is-active");
  // Nav items are view navigation, so aria-current="page" (report rows, which are
  // items-in-a-set, keep "true").
  expect(inbox.attributes("aria-current")).toBe("page");
  expect(navItemByLabel(wrapper, "Settings").attributes("aria-current")).toBeUndefined();

  await navItemByLabel(wrapper, "Settings").trigger("click");
  expect(wrapper.emitted("navigate")).toEqual([["settings"]]);
});

test("count badges show only when non-zero", () => {
  const wrapper = makeWrapper({ inboxCount: 3, archiveCount: 0 });
  expect(navItemByLabel(wrapper, "Research Inbox").find(".nav-badge").text()).toBe("3");
  expect(navItemByLabel(wrapper, "Research Archive").find(".nav-badge").exists()).toBe(false);
});
