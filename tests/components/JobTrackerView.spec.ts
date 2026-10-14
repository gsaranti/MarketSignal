// JobTrackerView is presentational, so no Tauri mocks. The auto-scroll watcher
// is deliberately not asserted: DOM-geometry behavior happy-dom cannot pin.

import { test, expect } from "vitest";
import { mount } from "@vue/test-utils";
import JobTrackerView from "../../src/components/JobTrackerView.vue";
import Icon from "../../src/components/Icon.vue";
import { deepFreeze } from "../helpers/freeze";
import type { RunTrace } from "../../src/types";

// The baseline step's request rows cover every reqTone branch
// (ok / empty=benign / rejected=fail / running).
const activeTrace: RunTrace = deepFreeze({
  runId: "run-1",
  label: "Weekly Market Report",
  terminal: null,
  unattributed: [],
  steps: [
    {
      key: "baseline",
      label: "Baseline scan",
      status: "ok",
      detail: null,
      agentText: "",
      agentThinking: "",
      requests: [
        { provider: "FMP", group: "indices", seriesId: "SPX", name: "S&P 500 index level, a deliberately long name that clips", status: "ok", detail: null },
        { provider: "FRED", group: "macro", seriesId: "DCOILWTICO", name: "WTI crude", status: "empty", detail: null },
        { provider: "FMP", group: "movers", seriesId: "GAIN", name: "Top gainers", status: "rejected", detail: "402 premium" },
        { provider: "Tavily", group: "news", seriesId: "q-oil", name: "oil price news", status: "running", detail: null },
      ],
    },
    { key: "route", label: "Research routing", status: "ok", detail: null, agentText: "", agentThinking: "", requests: [] },
    { key: "gather", label: "News gather", status: "failed", detail: "Tavily returned 500", agentText: "", agentThinking: "", requests: [] },
    { key: "report", label: "Drafting report", status: "running", detail: null, agentText: "## Market Signal Thesis\n\nThe tape is constructive.", agentThinking: "Weighing thin breadth against softening cut odds.", requests: [] },
    { key: "memory", label: "Memory write", status: "cancelled", detail: "Run cancelled", agentText: "", agentThinking: "", requests: [] },
  ],
});

function terminalTrace(status: string): RunTrace {
  return deepFreeze({
    runId: "run-1",
    label: "Weekly Market Report",
    terminal: { status, detail: null },
    unattributed: [],
    steps: [{ key: "report", label: "Drafting report", status: "ok", detail: null, agentText: "", agentThinking: "", requests: [] }],
  });
}

test("the headline tracks the running step while active", () => {
  const wrapper = mount(JobTrackerView, {
    props: { trace: activeTrace, active: true, cancelRequested: false },
  });
  expect(wrapper.find(".toolbar-label").text()).toBe("Drafting report");
  expect(wrapper.find(".toolbar-tag").exists()).toBe(false);
});

test("an active run with no running step yet falls back to 'Generating report'", () => {
  // Active but no step `running` yet: before/between steps.
  const pending: RunTrace = deepFreeze({
    runId: "run-1",
    label: "Weekly Market Report",
    terminal: null,
    unattributed: [],
    steps: [{ key: "baseline", label: "Baseline scan", status: "pending", detail: null, agentText: "", agentThinking: "", requests: [] }],
  });
  const wrapper = mount(JobTrackerView, {
    props: { trace: pending, active: true, cancelRequested: false },
  });
  expect(wrapper.find(".toolbar-label").text()).toBe("Generating report");
  expect(wrapper.find(".toolbar-tag").exists()).toBe(false);
});

test("a portfolio-owned tracker never announces itself as report generation", () => {
  // The owning-page label set (docs/run-tracking.md).
  const pending: RunTrace = deepFreeze({
    runId: "run-1",
    label: "Portfolio Analysis",
    terminal: null,
    unattributed: [],
    steps: [{ key: "holdings", label: "Pull holdings", status: "pending", detail: null, agentText: "", agentThinking: "", requests: [] }],
  });
  const active = mount(JobTrackerView, {
    props: { trace: pending, active: true, cancelRequested: false, kind: "portfolio" },
  });
  expect(active.find(".toolbar-label").text()).toBe("Analyzing portfolio");
  expect(active.find(".tracker-scroll").attributes("aria-label")).toBe(
    "Portfolio analysis progress"
  );

  const terminal = mount(JobTrackerView, {
    props: { trace: terminalTrace("successful"), active: false, cancelRequested: false, kind: "portfolio" },
  });
  expect(terminal.find(".btn-secondary").text()).toBe("Back to portfolio");
});

test("a quick-check tracker carries its own labels and returns to the portfolio", () => {
  const pending: RunTrace = deepFreeze({
    runId: "run-1",
    label: "Quick check",
    terminal: null,
    unattributed: [],
    steps: [{ key: "sweep-prices", label: "Refresh prices (sweep)", status: "pending", detail: null, agentText: "", agentThinking: "", requests: [] }],
  });
  const active = mount(JobTrackerView, {
    props: { trace: pending, active: true, cancelRequested: false, kind: "portfolio-quick-check" },
  });
  expect(active.find(".toolbar-label").text()).toBe("Running quick check");
  expect(active.find(".tracker-scroll").attributes("aria-label")).toBe(
    "Quick check progress"
  );

  const terminal = mount(JobTrackerView, {
    props: { trace: terminalTrace("successful"), active: false, cancelRequested: false, kind: "portfolio-quick-check" },
  });
  expect(terminal.find(".btn-secondary").text()).toBe("Back to portfolio");
});

test("the report-owned tracker keeps its report labels (the default kind)", () => {
  const terminal = mount(JobTrackerView, {
    props: { trace: terminalTrace("successful"), active: false, cancelRequested: false },
  });
  expect(terminal.find(".btn-secondary").text()).toBe("Back to report");
  expect(terminal.find(".tracker-scroll").attributes("aria-label")).toBe(
    "Report generation progress"
  );
});

test("once terminal the headline reads 'Run log' and the tag maps + tones the outcome", () => {
  const completed = mount(JobTrackerView, {
    props: { trace: terminalTrace("successful"), active: false, cancelRequested: false },
  });
  expect(completed.find(".toolbar-label").text()).toBe("Run log");
  expect(completed.find(".toolbar-tag").text()).toBe("Completed");
  expect(completed.find(".toolbar-tag").classes()).not.toContain("is-alert");

  const failed = mount(JobTrackerView, {
    props: { trace: terminalTrace("failed"), active: false, cancelRequested: false },
  });
  expect(failed.find(".toolbar-tag").text()).toBe("Failed");
  expect(failed.find(".toolbar-tag").classes()).toContain("is-alert");

  const cancelled = mount(JobTrackerView, {
    props: { trace: terminalTrace("cancelled"), active: false, cancelRequested: false },
  });
  expect(cancelled.find(".toolbar-tag").text()).toBe("Cancelled");
  expect(cancelled.find(".toolbar-tag").classes()).toContain("is-alert");
});

test("each step shows its status word and the marker icon for its outcome", () => {
  const wrapper = mount(JobTrackerView, {
    props: { trace: activeTrace, active: true, cancelRequested: false },
  });
  const steps = wrapper.findAll(".step");
  // ok/pending carry meaning via the marker alone.
  expect(steps[3].find(".step-status").text()).toBe("Working…");
  expect(steps[2].find(".step-status").text()).toBe("Failed");
  expect(steps[4].find(".step-status").text()).toBe("Stopped");
  expect(steps[0].find(".step-status").exists()).toBe(false);
  // Asserted by icon name, so a check/warning swap is caught.
  expect(steps[0].find(".step-marker").findComponent(Icon).props("name")).toBe("check");
  expect(steps[2].find(".step-marker").findComponent(Icon).props("name")).toBe("warning");
  expect(steps[4].find(".step-marker").findComponent(Icon).props("name")).toBe("warning");
});

test("the quick check's sweep outcomes render as completed-with-outcome, never failure or not-started", () => {
  // Neither outcome may fall back to the screen-reader "Not started" branch.
  const sweep: RunTrace = deepFreeze({
    runId: "qc-1",
    label: "Portfolio quick check",
    terminal: null,
    unattributed: [],
    steps: [
      { key: "check-aapl", label: "Check AAPL", status: "flagged", detail: null, agentText: "", agentThinking: "", requests: [] },
      { key: "check-msft", label: "Check MSFT", status: "unknown", detail: null, agentText: "", agentThinking: "", requests: [] },
    ],
  });
  const wrapper = mount(JobTrackerView, {
    props: { trace: sweep, active: false, cancelRequested: false },
  });
  const steps = wrapper.findAll(".step");
  expect(steps[0].find(".step-status").text()).toBe("Flagged");
  expect(steps[0].find(".step-marker").findComponent(Icon).props("name")).toBe("warning");
  expect(steps[1].find(".step-status").text()).toBe("Unknown");
  expect(steps[1].find(".step-marker").findComponent(Icon).exists()).toBe(false);
  expect(steps[0].find(".sr-only").exists()).toBe(false);
  expect(steps[1].find(".sr-only").exists()).toBe(false);
});

test("a failure/stop detail renders on the steps that didn't complete cleanly, not on clean ones", () => {
  const wrapper = mount(JobTrackerView, {
    props: { trace: activeTrace, active: true, cancelRequested: false },
  });
  const steps = wrapper.findAll(".step");
  expect(steps[2].find(".step-detail").text()).toBe("Tavily returned 500");
  expect(steps[4].find(".step-detail").text()).toBe("Run cancelled");
  expect(steps[0].find(".step-detail").exists()).toBe(false);
});

test("request rows map status to tone and show the raw word for non-ok/non-running", () => {
  const wrapper = mount(JobTrackerView, {
    props: { trace: activeTrace, active: true, cancelRequested: false },
  });
  const reqs = wrapper.findAll(".req");
  expect(reqs.map((r) => r.attributes("data-tone"))).toEqual(["ok", "benign", "fail", "running"]);
  expect(reqs[0].find(".req-name").attributes("title")).toBe(
    "S&P 500 index level, a deliberately long name that clips"
  );
  expect(reqs[0].find(".req-status svg").exists()).toBe(true);
  expect(reqs[2].find(".req-status").text()).toBe("rejected");
  expect(reqs[3].find(".req-status .req-dot").exists()).toBe(true);
  const detail = reqs[2].find(".req-detail");
  expect(detail.text()).toBe("402 premium");
  expect(detail.attributes("title")).toBe("402 premium");
  expect(reqs[0].find(".req-detail").exists()).toBe(false);
  expect(reqs[1].find(".req-detail").exists()).toBe(false);
});

test("a research row names what it asked for, and a completed row shows its outcome note", () => {
  const longUrl =
    "https://www.sec.gov/Archives/edgar/data/1534701/000153470126000012/psx-20260630.htm";
  const trace: RunTrace = {
    runId: "r-web",
    label: "Portfolio run",
    steps: [
      {
        key: "holding-PSX",
        label: "Analyze PSX",
        status: "running",
        detail: null,
        agentText: "",
        agentThinking: "",
        analystThinking: {},
        requests: [
          { provider: "web", group: "research", seriesId: "search: PSX refining margins 2026", name: "competitive-position", status: "ok", detail: "12 hits", target: { kind: "search", text: "PSX refining margins 2026" } },
          { provider: "web", group: "research", seriesId: `fetch: ${longUrl}`, name: "competitive-position", status: "running", detail: null, target: { kind: "fetch", text: longUrl } },
          { provider: "web", group: "research", seriesId: "fetch: https://example.com/x", name: "competitive-position", status: "failed", detail: "SearXNG returned no usable results", target: { kind: "fetch", text: "https://example.com/x" } },
          { provider: "SEC", group: "company-facts", seriesId: "0000078214", name: "SEC company facts", status: "ok", detail: null, target: null },
        ],
      },
    ],
    unattributed: [],
    terminal: null,
  };
  const wrapper = mount(JobTrackerView, {
    props: { trace, active: true, cancelRequested: false },
  });
  const reqs = wrapper.findAll(".req");
  expect(reqs).toHaveLength(4);
  expect(reqs[0].find(".req-name").text()).toBe("competitive-position");
  expect(reqs[0].find(".req-target-kind").text()).toBe("search");
  expect(reqs[0].find(".req-target-text").text()).toBe("PSX refining margins 2026");
  expect(reqs[0].find(".req-target").attributes("title")).toBe("PSX refining margins 2026");
  expect(reqs[0].find(".req-target").attributes("aria-label")).toBe("Show full search query: PSX refining margins 2026");
  expect(reqs[1].find(".req-target-kind").text()).toBe("fetch");
  expect(reqs[1].find(".req-target-text").text()).toBe(longUrl);
  expect(reqs[1].find(".req-target").attributes("title")).toBe(longUrl);
  const order = reqs[0].findAll(".req-name, .req-target, .req-status").map((s) =>
    s.classes().find((c) => ["req-name", "req-target", "req-status"].includes(c))
  );
  expect(order.indexOf("req-name")).toBeLessThan(order.indexOf("req-target"));
  expect(order.indexOf("req-target")).toBeLessThan(order.indexOf("req-status"));
  expect(reqs[0].find(".req-note").text()).toBe("12 hits");
  expect(reqs[0].find(".req-note").attributes("title")).toBe("12 hits");
  expect(reqs[1].find(".req-note").exists()).toBe(false);
  expect(reqs[2].find(".req-note").exists()).toBe(false);
  expect(reqs[2].find(".req-detail").text()).toBe("SearXNG returned no usable results");
  expect(reqs[3].find(".req-target").exists()).toBe(false);
  expect(reqs[3].find(".req-note").exists()).toBe(false);
});

test.each(["step", "unattributed"])("%s request subjects reveal their full text independently and keep focus", async (home) => {
  const url = `https://example.com/${"long-path-".repeat(30)}?document=123`;
  // Identical URLs are separate requests and must never share expansion state.
  const requests = Array.from({ length: 2 }, () => ({
    provider: "web", group: "research", seriesId: `fetch: ${url}`,
    name: "competitive-position", status: "running", detail: null,
    target: { kind: "fetch", text: url },
  }));
  const trace: RunTrace = {
    runId: "disclosure-run", label: "Portfolio run", terminal: null,
    steps: home === "step" ? [{
      key: "holding-PSX", label: "Analyze PSX", status: "running", detail: null,
      agentText: "", agentThinking: "", analystThinking: {}, requests,
    }] : [],
    unattributed: home === "unattributed" ? requests : [],
  };
  const wrapper = mount(JobTrackerView, {
    props: { trace, active: true, cancelRequested: false },
    attachTo: document.body,
  });
  try {
    const rows = wrapper.findAll(".req");
    const button = rows[0].get<HTMLButtonElement>(".req-target");
    // happy-dom does not synthesize native keyboard activation, so the
    // native-button assertions stand in for Enter/Space.
    expect(button.element.tagName).toBe("BUTTON");
    expect(button.attributes("type")).toBe("button");
    expect(button.element.disabled).toBe(false);
    expect(button.element.tabIndex).toBe(0);
    expect(button.attributes("aria-expanded")).toBe("false");
    expect(rows[0].find(".req-target-full").exists()).toBe(false);
    button.element.focus();
    await button.trigger("click");
    expect(button.attributes("aria-expanded")).toBe("true");
    expect(button.attributes("aria-label")).toBe(`Hide full fetch URL: ${url}`);
    expect(rows[0].get(".req-target-full").text()).toBe(url);
    expect(rows[1].find(".req-target-full").exists()).toBe(false);
    expect(document.activeElement).toBe(button.element);

    await button.trigger("click");
    expect(button.attributes("aria-expanded")).toBe("false");
    expect(button.attributes("aria-label")).toBe(`Show full fetch URL: ${url}`);
    expect(rows[0].find(".req-target-full").exists()).toBe(false);
    expect(document.activeElement).toBe(button.element);
  } finally {
    wrapper.unmount();
  }
});

test("the streamed agent text renders only where present", () => {
  const wrapper = mount(JobTrackerView, {
    props: { trace: activeTrace, active: true, cancelRequested: false },
  });
  const streams = wrapper.findAll(".agent-stream");
  expect(streams).toHaveLength(1);
  expect(streams[0].text()).toContain("Market Signal Thesis");
});

test("the streamed reasoning renders only where present, labeled and above the report", () => {
  const wrapper = mount(JobTrackerView, {
    props: { trace: activeTrace, active: true, cancelRequested: false },
  });
  const reasoning = wrapper.findAll(".agent-thinking-body");
  expect(reasoning).toHaveLength(1);
  expect(reasoning[0].text()).toContain("Weighing thin breadth");
  const reportStep = wrapper.findAll(".step")[3];
  expect(reportStep.find(".agent-thinking-label").text()).toBe("Reasoning");
  const html = reportStep.html();
  expect(html.indexOf("agent-thinking")).toBeLessThan(html.indexOf("agent-stream"));
});

test("each analyst's reasoning renders as its own labeled pane, in canonical order", () => {
  // Panes are ordered bull → bear → balanced regardless of the fixture's key order.
  const trace: RunTrace = deepFreeze({
    runId: "run-1",
    label: "Weekly Market Report",
    terminal: null,
    unattributed: [],
    steps: [
      {
        key: "analysts",
        label: "Running the analyst agents",
        status: "running",
        detail: null,
        agentText: "",
        agentThinking: "",
        analystThinking: {
          balanced: "Adjudicating the two strongest claims.",
          bull: "Is the rally real, or just a few names?",
          bear: "What breaks first under stress?",
        },
        requests: [],
      },
    ],
  });
  const wrapper = mount(JobTrackerView, {
    props: { trace, active: true, cancelRequested: false },
  });
  const labels = wrapper.findAll(".agent-thinking-label").map((l) => l.text());
  expect(labels).toEqual(["Bull · Reasoning", "Bear · Reasoning", "Balanced · Reasoning"]);
  const bodies = wrapper.findAll(".agent-thinking-body").map((b) => b.text());
  expect(bodies[0]).toContain("Is the rally real");
  expect(bodies[2]).toContain("Adjudicating the two strongest");
});

test("unattributed rows render as a neutral trailing list, absent when empty", () => {
  // Unattributed: the row's events carried no owning-step stamp.
  const withUnattributed: RunTrace = deepFreeze({
    runId: "run-1",
    label: "Portfolio Analysis",
    terminal: { status: "successful", detail: null },
    unattributed: [
      { provider: "FRED", group: "suite-rate", seriesId: "DGS10", name: "Rate anchor", status: "ok", detail: null },
    ],
    steps: [{ key: "rates", label: "Load rate anchors", status: "ok", detail: null, agentText: "", agentThinking: "", requests: [] }],
  });
  const wrapper = mount(JobTrackerView, {
    props: { trace: withUnattributed, active: false, cancelRequested: false, kind: "portfolio" },
  });
  const section = wrapper.find(".unattributed");
  expect(section.exists()).toBe(true);
  expect(section.find(".unattributed-label").text()).toBe("Requests outside any step");
  const row = section.find(".req");
  expect(row.attributes("data-tone")).toBe("ok");
  expect(row.find(".req-name").text()).toBe("Rate anchor");
  expect(section.find(".step-marker").exists()).toBe(false);
  expect(section.find(".step-status").exists()).toBe(false);

  const without = mount(JobTrackerView, {
    props: { trace: activeTrace, active: true, cancelRequested: false },
  });
  expect(without.find(".unattributed").exists()).toBe(false);
});

test("a run with no steps yet shows the Starting edge state", () => {
  const empty = deepFreeze({ runId: "run-1", label: "Weekly Market Report", terminal: null, unattributed: [], steps: [] });
  const wrapper = mount(JobTrackerView, {
    props: { trace: empty, active: true, cancelRequested: false },
  });
  expect(wrapper.find(".tracker-starting").text()).toBe("Starting…");
});

test("the scroll region reports aria-busy while the run is active", () => {
  const active = mount(JobTrackerView, {
    props: { trace: activeTrace, active: true, cancelRequested: false },
  });
  expect(active.find(".tracker-scroll").attributes("aria-busy")).toBe("true");

  const done = mount(JobTrackerView, {
    props: { trace: terminalTrace("successful"), active: false, cancelRequested: false },
  });
  expect(done.find(".tracker-scroll").attributes("aria-busy")).toBe("false");
});

test("active runs offer Cancel (which emits + reports its busy state); terminal runs offer Back", async () => {
  const wrapper = mount(JobTrackerView, {
    props: { trace: activeTrace, active: true, cancelRequested: false },
  });
  const cancel = wrapper.find(".btn-cancel");
  expect(cancel.text()).toBe("Cancel run");
  expect(cancel.attributes("disabled")).toBeUndefined();
  await cancel.trigger("click");
  expect(wrapper.emitted("cancel")).toHaveLength(1);

  await wrapper.setProps({ cancelRequested: true });
  expect(wrapper.find(".btn-cancel").text()).toBe("Cancelling…");
  expect(wrapper.find(".btn-cancel").attributes("disabled")).toBeDefined();

  const done = mount(JobTrackerView, {
    props: { trace: terminalTrace("successful"), active: false, cancelRequested: false },
  });
  expect(done.find(".btn-cancel").exists()).toBe(false);
  await done.find(".toolbar-actions button").trigger("click");
  expect(done.emitted("close")).toHaveLength(1);
});

// ---- The interrupted-run Resume offer (docs/portfolio-analysis.md §Failure
// posture) ----

const resumable = deepFreeze({
  available: true,
  runId: "run-9",
  createdAt: "2026-08-21T12:00:00+00:00",
  completed: 3,
  total: 8,
  reason: null,
});

test("a stopped portfolio run with an eligible trail offers Resume and states what it keeps", async () => {
  const wrapper = mount(JobTrackerView, {
    props: {
      trace: terminalTrace("failed"),
      active: false,
      cancelRequested: false,
      kind: "portfolio" as const,
      resume: resumable,
    },
  });
  const buttons = wrapper.findAll(".toolbar-actions button");
  expect(buttons.map((b) => b.text())).toEqual(["Resume run", "Back to portfolio"]);
  const note = wrapper.find(".resume-note");
  expect(note.text()).toContain("3 of 8 holdings completed");
  // The trail is identified by its own as-of, never claimed as this run's work:
  // an earlier run's trail can outlive a run that failed before opening its own.
  expect(note.text()).toContain("An interrupted run from");
  expect(note.text()).toContain("2026");
  await buttons[0].trigger("click");
  expect(wrapper.emitted("resume")).toHaveLength(1);

  // A requested resume disables the button (the single run slot cannot race).
  await wrapper.setProps({ resumeRequested: true });
  const resume = wrapper.find(".toolbar-actions button");
  expect(resume.text()).toBe("Resuming…");
  expect(resume.attributes("disabled")).toBeDefined();
});

test("an ineligible trail states why instead of offering, and other states offer nothing", () => {
  const blocked = mount(JobTrackerView, {
    props: {
      trace: terminalTrace("cancelled"),
      active: false,
      cancelRequested: false,
      kind: "portfolio" as const,
      resume: deepFreeze({ ...resumable, available: false, reason: "a newer run has persisted since the interrupted run" }),
    },
  });
  expect(blocked.findAll(".toolbar-actions button").map((b) => b.text())).toEqual([
    "Back to portfolio",
  ]);
  expect(blocked.find(".resume-note").text()).toContain("Resume unavailable:");

  const none = mount(JobTrackerView, {
    props: {
      trace: terminalTrace("failed"),
      active: false,
      cancelRequested: false,
      kind: "portfolio" as const,
      resume: null,
    },
  });
  expect(none.find(".resume-note").exists()).toBe(false);

  // A successful run never offers resume, even with a stale status object.
  const done = mount(JobTrackerView, {
    props: {
      trace: terminalTrace("successful"),
      active: false,
      cancelRequested: false,
      kind: "portfolio" as const,
      resume: resumable,
    },
  });
  expect(done.findAll(".toolbar-actions button").map((b) => b.text())).toEqual([
    "Back to portfolio",
  ]);
  expect(done.find(".resume-note").exists()).toBe(false);

  const report = mount(JobTrackerView, {
    props: {
      trace: terminalTrace("failed"),
      active: false,
      cancelRequested: false,
      resume: resumable,
    },
  });
  expect(report.findAll(".toolbar-actions button").map((b) => b.text())).toEqual([
    "Back to report",
  ]);
});
