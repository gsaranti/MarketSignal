// Component tests for JobStatusPanel.vue — props in, two events out (generate /
// view-tracker). Mock-free like ResearchDocuments/Settings. These pin: the
// visibility gate (silent until the first status resolves), the running indicator
// (which hides both the facts and the Generate control), the error-vs-facts branch,
// the run-history facts (the section-scoped stamps + the per-section "no run yet"
// fallback + the conditional failure/cancelled/skipped rows + formatLocal's
// raw-string fallback), the view-tracker handle's label swap + emit, and the
// Generate button's report-view-only rendering (showGenerate, independent of the
// stamp section) + disabled/label/title states + emit.

import { test, expect } from "vitest";
import { mount } from "@vue/test-utils";
import JobStatusPanel from "../../src/components/JobStatusPanel.vue";
import { deepFreeze } from "../helpers/freeze";
import type { JobStatus, SectionStamps } from "../../src/types";

function stamps(overrides: Partial<SectionStamps> = {}): SectionStamps {
  return {
    last_successful_at: null,
    last_failed_at: null,
    last_failure_detail: null,
    last_skipped_at: null,
    last_cancelled_at: null,
    ...overrides,
  };
}

function status(overrides: Partial<JobStatus> = {}): JobStatus {
  return {
    is_running: false,
    running_kind: null,
    report: stamps(),
    portfolio: stamps(),
    ...overrides,
  };
}

const baseProps = deepFreeze({
  status: null as JobStatus | null,
  section: "report" as "report" | "portfolio",
  showGenerate: true,
  error: null as string | null,
  blocked: false,
  generating: false,
  runActive: false,
  runningLabel: "Generating report…",
  progress: null as { fraction: number; stepNumber: number; total: number; label: string } | null,
  runStartedAt: null as number | null,
  hasRunLog: false,
  viewingTracker: false,
});

function makeWrapper(overrides: Partial<typeof baseProps> = {}) {
  return mount(JobStatusPanel, { props: { ...baseProps, ...overrides } });
}

test("stays hidden until there is status, an error, a live run, or a run log", () => {
  expect(makeWrapper().find(".job-panel").exists()).toBe(false);
  expect(makeWrapper({ status: status() }).find(".job-panel").exists()).toBe(true);
});

test("a live run shows the running indicator and hides both the facts and Generate", () => {
  const wrapper = makeWrapper({ runActive: true });
  expect(wrapper.find(".job-running").exists()).toBe(true);
  expect(wrapper.find(".job-facts").exists()).toBe(false);
  expect(wrapper.find(".btn-generate").exists()).toBe(false);

  // The backend `status.is_running` flag drives the same indicator independently of
  // the event-driven `runActive` prop (the `||` fallback in `isRunning`).
  const viaStatus = makeWrapper({ runActive: false, status: status({ is_running: true }) });
  expect(viaStatus.find(".job-running").exists()).toBe(true);
  expect(viaStatus.find(".btn-generate").exists()).toBe(false);
});

test("the running row's label is the caller's runningLabel, not a hardcoded string", () => {
  // The run slot is shared (report / Portfolio / Schwab connect); the row must say
  // what actually holds it — a Schwab login never reads as "Generating report…".
  expect(makeWrapper({ runActive: true }).find(".job-running-label").text()).toBe(
    "Generating report…"
  );
  expect(
    makeWrapper({ runActive: true, runningLabel: "Connecting to Charles Schwab…" })
      .find(".job-running-label")
      .text()
  ).toBe("Connecting to Charles Schwab…");
});

test("the determinate fill width tracks progress.fraction and clamps to [0, 100]%", () => {
  const fill = (fraction: number) =>
    makeWrapper({
      runActive: true,
      progress: { fraction, stepNumber: 5, total: 8, label: "Gathering and condensing research" },
    })
      .find(".job-running-fill")
      .attributes("style") ?? "";

  expect(fill(0)).toContain("width: 0%");
  expect(fill(0.5)).toContain("width: 50%");
  expect(fill(1)).toContain("width: 100%");
  // Out-of-range fractions are clamped, never < 0% or > 100%.
  expect(fill(1.4)).toContain("width: 100%");
  expect(fill(-0.3)).toContain("width: 0%");
});

test("the caption reports the step number, total, and label; absent without progress", () => {
  const withProgress = makeWrapper({
    runActive: true,
    progress: { fraction: 0.5, stepNumber: 5, total: 8, label: "Gathering and condensing research" },
  });
  expect(withProgress.find(".job-running-caption").text()).toBe(
    "Step 5 of 8 · Gathering and condensing research"
  );

  // A live run with no progress object yet shows the running block but no caption.
  const noProgress = makeWrapper({ runActive: true });
  expect(noProgress.find(".job-running").exists()).toBe(true);
  expect(noProgress.find(".job-running-caption").exists()).toBe(false);
});

test("the elapsed timer renders m:ss from runStartedAt, and is hidden when it's unset", () => {
  // ~65s ago -> "1:05" (allow ±1s for the tick between mount and assert).
  const timed = makeWrapper({ runActive: true, runStartedAt: Date.now() - 65_000 });
  expect(timed.find(".job-running-time").text()).toMatch(/^1:0[45]$/);

  // No start time -> no timer (the `== null` guard: never a NaN:NaN readout, even if
  // the prop is undefined rather than null).
  expect(makeWrapper({ runActive: true }).find(".job-running-time").exists()).toBe(false);
  expect(
    makeWrapper({ runActive: true, runStartedAt: undefined as unknown as number | null })
      .find(".job-running-time")
      .exists()
  ).toBe(false);
});

test("a config-check error replaces the facts with the error line", () => {
  const wrapper = makeWrapper({ error: "db locked" });
  expect(wrapper.find(".job-error").text()).toContain("db locked");
  expect(wrapper.find(".job-facts").exists()).toBe(false);
});

test("facts: last-run fallback, the conditional failure/cancelled/skipped rows, and formatLocal's raw fallback", () => {
  // No timestamps -> only the always-present "Last run" row, with its fallback copy.
  const empty = makeWrapper({ status: status() });
  expect(empty.findAll(".job-fact dt").map((dt) => dt.text())).toEqual(["Last run"]);
  expect(empty.find(".job-fact dd").text()).toBe("No report has run yet");

  // Each terminal timestamp adds its own labelled row, in template order. An
  // unparseable timestamp falls back to the raw string (locale formatting is
  // environment-dependent, so the raw fallback is the TZ-safe thing to assert).
  const populated = makeWrapper({
    status: status({
      report: stamps({
        last_successful_at: "not-a-date",
        last_failed_at: "also-bad",
        last_cancelled_at: "x",
        last_skipped_at: "y",
      }),
    }),
  });
  expect(populated.findAll(".job-fact dt").map((dt) => dt.text())).toEqual([
    "Last run",
    "Last failure",
    "Last cancelled",
    "Last skipped",
  ]);
  expect(populated.find(".job-fact dd").text()).toBe("not-a-date");
});

test("the facts read the active section's stamps — the other section's never leak in", () => {
  // A portfolio finish with an empty report history: report chrome keeps its
  // fallback copy (the mislabeling the first live run surfaced), while the
  // portfolio section renders the stamp.
  const mixed = status({
    portfolio: stamps({ last_successful_at: "not-a-date", last_failed_at: "also-bad" }),
  });
  const reportSide = makeWrapper({ status: mixed, section: "report" });
  expect(reportSide.findAll(".job-fact dt").map((dt) => dt.text())).toEqual(["Last run"]);
  expect(reportSide.find(".job-fact dd").text()).toBe("No report has run yet");

  const portfolioSide = makeWrapper({ status: mixed, section: "portfolio" });
  expect(portfolioSide.findAll(".job-fact dt").map((dt) => dt.text())).toEqual([
    "Last run",
    "Last failure",
  ]);
  expect(portfolioSide.find(".job-fact dd").text()).toBe("not-a-date");
});

test("the portfolio section's empty state talks about analysis, not reports", () => {
  const wrapper = makeWrapper({ status: status(), section: "portfolio" });
  expect(wrapper.find(".job-fact dd").text()).toBe("No analysis has run yet");
});

test("Generate renders only when the report view is the main window; the tracker handle stays", () => {
  // Each section keeps its own trigger — Run analysis lives on the Portfolio
  // page, so the footer offers no generate control there even when idle.
  const portfolio = makeWrapper({
    status: status(),
    section: "portfolio",
    showGenerate: false,
    hasRunLog: true,
  });
  expect(portfolio.find(".btn-generate").exists()).toBe(false);
  expect(portfolio.find(".btn-handle").exists()).toBe(true);

  // The neutral surfaces (inbox/archive/settings) share the report's stamps —
  // section "report" — but not its trigger: showGenerate alone gates the
  // button, so report-section chrome without it must stay button-free.
  const neutral = makeWrapper({ status: status(), section: "report", showGenerate: false });
  expect(neutral.find(".btn-generate").exists()).toBe(false);

  expect(makeWrapper({ status: status() }).find(".btn-generate").exists()).toBe(true);
});

test("the view-tracker handle swaps its label by run state and emits view-tracker", async () => {
  const running = makeWrapper({ runActive: true, hasRunLog: true });
  const handle = running.find(".btn-handle");
  expect(handle.text()).toBe("View progress");
  await handle.trigger("click");
  expect(running.emitted("view-tracker")).toHaveLength(1);

  const idle = makeWrapper({ status: status(), hasRunLog: true });
  expect(idle.find(".btn-handle").text()).toBe("Latest run log");

  // Already on the tracker -> the handle would be a no-op, so it's hidden.
  const viewing = makeWrapper({ runActive: true, hasRunLog: true, viewingTracker: true });
  expect(viewing.find(".btn-handle").exists()).toBe(false);
});

test("Generate is enabled by default and emits generate", async () => {
  const wrapper = makeWrapper({ status: status() });
  const btn = wrapper.find(".btn-generate");
  expect(btn.text()).toBe("Generate now");
  expect(btn.attributes("disabled")).toBeUndefined();
  await btn.trigger("click");
  expect(wrapper.emitted("generate")).toHaveLength(1);
});

test("Generate is disabled while generating and reports the busy label", () => {
  const btn = makeWrapper({ status: status(), generating: true }).find(".btn-generate");
  expect(btn.text()).toBe("Generating…");
  expect(btn.attributes("disabled")).toBeDefined();
});

test("Generate is disabled and titled with the reason when the run is blocked", () => {
  const btn = makeWrapper({ status: status(), blocked: true }).find(".btn-generate");
  expect(btn.attributes("disabled")).toBeDefined();
  expect(btn.attributes("title")).toContain("Resolve the configuration warnings");
});
