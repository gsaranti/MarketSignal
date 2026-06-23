// Component tests for JobTrackerView.vue — props in, two events out (cancel /
// close). Mock-free like ResearchDocuments/Settings. These pin the run-tracker's
// rendering contract: the headline (running step while active, "Run log" once
// terminal), the terminal tag mapping + alert tone, the per-step status word and
// marker icon, the failure-detail gate, the request-row tone mapping, the streamed
// agent-text block, the "Starting…" edge state, the scroll region's aria-busy, and
// the Cancel/Back action with its cancel-requested busy state. The auto-scroll
// watcher is deliberately not asserted — it is DOM-geometry behavior, not a
// contract worth pinning in happy-dom.

import { test, expect } from "vitest";
import { mount } from "@vue/test-utils";
import JobTrackerView from "../../src/components/JobTrackerView.vue";
import Icon from "../../src/components/Icon.vue";
import { deepFreeze } from "../helpers/freeze";
import type { RunTrace } from "../../src/types";

// An in-flight run: an ok baseline step whose request rows cover every reqTone
// branch (ok / empty=benign / rejected=fail / running), an ok routing step, a
// failed gather step carrying a detail, and the running report step streaming text.
const activeTrace: RunTrace = deepFreeze({
  runId: "run-1",
  label: "Weekly Market Report",
  terminal: null,
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
    steps: [{ key: "report", label: "Drafting report", status: "ok", detail: null, agentText: "", agentThinking: "", requests: [] }],
  });
}

test("the headline tracks the running step while active", () => {
  const wrapper = mount(JobTrackerView, {
    props: { trace: activeTrace, active: true, cancelRequested: false },
  });
  expect(wrapper.find(".toolbar-label").text()).toBe("Drafting report");
  // No terminal tag while the run is in flight.
  expect(wrapper.find(".toolbar-tag").exists()).toBe(false);
});

test("an active run with no running step yet falls back to 'Generating report'", () => {
  // The run owns the slot (active) but no step is `running` — before/between steps.
  // The headline's third arm; the report tag stays absent (terminal is null).
  const pending: RunTrace = deepFreeze({
    runId: "run-1",
    label: "Weekly Market Report",
    terminal: null,
    steps: [{ key: "baseline", label: "Baseline scan", status: "pending", detail: null, agentText: "", agentThinking: "", requests: [] }],
  });
  const wrapper = mount(JobTrackerView, {
    props: { trace: pending, active: true, cancelRequested: false },
  });
  expect(wrapper.find(".toolbar-label").text()).toBe("Generating report");
  expect(wrapper.find(".toolbar-tag").exists()).toBe(false);
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
  // running -> "Working…"; failed -> "Failed"; cancelled -> "Stopped"; ok/pending
  // carry meaning via the marker alone.
  expect(steps[3].find(".step-status").text()).toBe("Working…");
  expect(steps[2].find(".step-status").text()).toBe("Failed");
  expect(steps[4].find(".step-status").text()).toBe("Stopped");
  expect(steps[0].find(".step-status").exists()).toBe(false);
  // The marker icon distinguishes outcome by NAME (not just "an svg exists"), so a
  // check<->warning swap is caught: ok -> check, failed/cancelled -> warning.
  expect(steps[0].find(".step-marker").findComponent(Icon).props("name")).toBe("check");
  expect(steps[2].find(".step-marker").findComponent(Icon).props("name")).toBe("warning");
  expect(steps[4].find(".step-marker").findComponent(Icon).props("name")).toBe("warning");
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
  // The clipped name carries its full text as a title tooltip.
  expect(reqs[0].find(".req-name").attributes("title")).toBe(
    "S&P 500 index level, a deliberately long name that clips"
  );
  // ok -> a check icon (svg); the failure shows its reason word; running -> a dot.
  expect(reqs[0].find(".req-status svg").exists()).toBe(true);
  expect(reqs[2].find(".req-status").text()).toBe("rejected");
  expect(reqs[3].find(".req-status .req-dot").exists()).toBe(true);
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
  // Only the one step carrying reasoning shows the pane (empty agentThinking renders nothing).
  const reasoning = wrapper.findAll(".agent-thinking-body");
  expect(reasoning).toHaveLength(1);
  expect(reasoning[0].text()).toContain("Weighing thin breadth");
  // It is labeled (distinct from the report-text console) and sits above the report stream.
  const reportStep = wrapper.findAll(".step")[3];
  expect(reportStep.find(".agent-thinking-label").text()).toBe("Reasoning");
  const html = reportStep.html();
  expect(html.indexOf("agent-thinking")).toBeLessThan(html.indexOf("agent-stream"));
});

test("a run with no steps yet shows the Starting edge state", () => {
  const empty = deepFreeze({ runId: "run-1", label: "Weekly Market Report", terminal: null, steps: [] });
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
