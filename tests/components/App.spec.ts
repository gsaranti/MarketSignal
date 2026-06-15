// First SFC spec to exercise the `@tauri-apps/api` boundary. App.vue is the only
// component that imports it — four modules: core (`invoke`, 7 commands), event
// (`listen`), window (`getCurrentWindow().onFocusChanged`), and app (`getVersion`).
//
// This establishes the project's Tauri-mock pattern. `vi.mock` factories are
// hoisted above imports, so the mock *functions* are declared via `vi.hoisted`
// and their *implementations* come from `tests/helpers/tauri.ts` in `beforeEach`.
// `vitest.config.ts` sets `globals: false`, so every test helper (incl. `vi`) is
// imported explicitly.
//
// Three assertions: (1) App's `onMounted` bootstrap contract — the exact command /
// listener / window / version set it fires on mount, which doubles as proof the
// four-module mock is complete enough to mount the real App; (2) the `@save`
// wiring round-trips a child emit into `invoke("save_settings", payload)`; (3) the
// `@set-enabled` wiring round-trips into `invoke("set_job_enabled", { enabled })`.

import { describe, test, expect, beforeEach, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { makeInvokeRouter, unlisten, emitterFor } from "../helpers/tauri";
import type { GeneratedReport, ReportSummary } from "../../src/types";

// Hoisted above the imports below so the `vi.mock` factories can close over these
// mock fns. (`vi.hoisted` runs before module imports; `vi` is available in it.)
const tauri = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  getVersion: vi.fn(),
  getCurrentWindow: vi.fn(),
  onFocusChanged: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: tauri.listen }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: tauri.getCurrentWindow,
}));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: tauri.getVersion }));

// Resolved only after the mocks are registered (hoisting makes the order here
// cosmetic, but App.vue must bind the mocked modules at import time).
import App from "../../src/App.vue";
import RecentReportsSidebar from "../../src/components/RecentReportsSidebar.vue";
import Settings from "../../src/components/Settings.vue";
import JobTrackerView from "../../src/components/JobTrackerView.vue";
import LatestReportView from "../../src/components/LatestReportView.vue";
import JobStatusPanel from "../../src/components/JobStatusPanel.vue";

beforeEach(() => {
  // Clear call history between tests, then (re)apply implementations.
  vi.clearAllMocks();
  tauri.invoke.mockImplementation(makeInvokeRouter());
  tauri.listen.mockResolvedValue(unlisten);
  tauri.getVersion.mockResolvedValue("9.9.9-test");
  tauri.onFocusChanged.mockResolvedValue(unlisten);
  tauri.getCurrentWindow.mockReturnValue({ onFocusChanged: tauri.onFocusChanged });
});

const invokedCommands = () => tauri.invoke.mock.calls.map((c) => c[0]);
const cancelRunCalls = () =>
  tauri.invoke.mock.calls.filter((c) => c[0] === "cancel_run");

// Mount App, let the onMounted cascade settle, then put a live run on screen: a
// run-started event builds the trace, and @view-tracker (the footer handle) flips
// the report pane to the tracker so JobTrackerView renders. Returns the captured
// "job-progress" emitter so a test can fold in more events. `overrides` reach the
// invoke router (e.g. a cancel_run handler), re-applied over the beforeEach default.
async function mountWithTracker(
  overrides: Record<string, (args?: Record<string, unknown>) => unknown> = {}
) {
  tauri.invoke.mockImplementation(makeInvokeRouter(overrides));
  const wrapper = mount(App);
  await flushPromises();
  const emit = emitterFor(tauri.listen, "job-progress");
  emit({ run_id: "R1", seq: 1, kind: "run-started", label: "Weekly run" });
  wrapper.findComponent(JobStatusPanel).vm.$emit("view-tracker");
  await flushPromises();
  return { wrapper, emit };
}

const sampleSummary: ReportSummary = {
  report_id: "rep-1",
  report_type: "weekly_market",
  created_at: "2026-06-14T09:00:00Z",
  risk_posture: "risk-on",
  market_cycle: "late-cycle",
  thesis_stance: "mixed",
  header_summary_bullets: ["A", "B", "C"],
  key_risks: [],
  unresolved_questions: [],
  forward_outlook_themes: [],
};
const sampleReport: GeneratedReport = {
  report_id: "rep-1",
  markdown: "# Weekly Market Signal\n\nBody.",
  markdown_path: "/reports/2026-06-14-market-signal-weekly-report.md",
  summary: sampleSummary,
};

describe("App.vue Tauri boundary", () => {
  test("mounts against the mock and fires the onMounted bootstrap cascade", async () => {
    const wrapper = mount(App);
    await flushPromises();

    // Version read, the focus subscription via getCurrentWindow(), and both
    // event listeners — all real onMounted glue.
    expect(tauri.getVersion).toHaveBeenCalledTimes(1);
    expect(tauri.getCurrentWindow).toHaveBeenCalled();
    expect(tauri.onFocusChanged).toHaveBeenCalledTimes(1);

    // Exactly the two listeners, no more — sorted-equality so the assertion is a
    // true set check (catches a dropped or duplicated listener), order-agnostic.
    const events = tauri.listen.mock.calls.map((c) => c[0]).sort();
    expect(events).toEqual(["job-finished", "job-progress"]);

    // Exactly the six refresh commands onMounted issues — sorted-equality enforces
    // the set with no extras or duplicates (not a subset), while tolerating any
    // reordering of the onMounted calls. Proves the four-module mock is complete
    // enough to mount the real App without a throw.
    expect([...invokedCommands()].sort()).toEqual([
      "check_configuration",
      "get_settings",
      "job_status",
      "list_reports",
      "list_research_archive",
      "list_research_inbox",
    ]);

    wrapper.unmount();
  });

  test("a Settings @save emit round-trips into invoke('save_settings', payload)", async () => {
    const wrapper = mount(App);
    await flushPromises();

    // Settings is v-if'd on view === 'settings'; the sidebar owns navigation.
    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "settings");
    await flushPromises();

    const settings = wrapper.findComponent(Settings);
    expect(settings.exists()).toBe(true);

    const payload = {
      models: { main: "gpt-x", bull: "gpt-x", bear: "gpt-x", balanced: "gpt-x" },
      credentials: {
        openai: null,
        anthropic: null,
        fmp: "new-fmp",
        fred: null,
        tavily: null,
      },
    };
    settings.vm.$emit("save", payload);
    await flushPromises();

    // The @save="saveSettings" wiring forwards the child payload verbatim.
    expect(tauri.invoke).toHaveBeenCalledWith("save_settings", payload);

    wrapper.unmount();
  });

  test("a Settings @set-enabled emit round-trips into invoke('set_job_enabled', { enabled })", async () => {
    const wrapper = mount(App);
    await flushPromises();

    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "settings");
    await flushPromises();

    const settings = wrapper.findComponent(Settings);
    expect(settings.exists()).toBe(true);

    // The default mock reports the job enabled; toggling it off must reach the
    // backend. This covers App's @set-enabled="setJobEnabled" binding + handler —
    // glue the child-level Settings spec (which stops at the emit) can't see.
    settings.vm.$emit("set-enabled", false);
    await flushPromises();

    expect(tauri.invoke).toHaveBeenCalledWith("set_job_enabled", {
      enabled: false,
    });

    wrapper.unmount();
  });
});

// The run tracker's event-folding (handleProgress) reducer, driven through the
// real "job-progress" listener via the captured emitter. JobTrackerView's `trace`
// prop is the observable — App.vue is <script setup> with no defineExpose, so the
// folded RunTrace is read through the child, never off `wrapper.vm`.
describe("App.vue run tracker", () => {
  test("folds run-started into a trace with the synthetic gate step, shown by @view-tracker", async () => {
    const { wrapper } = await mountWithTracker();

    const tracker = wrapper.findComponent(JobTrackerView);
    expect(tracker.exists()).toBe(true);
    expect(tracker.props("active")).toBe(true);

    const trace = tracker.props("trace");
    expect(trace.runId).toBe("R1");
    expect(trace.label).toBe("Weekly run");
    // The run only starts once the gate passed, so it's shown pre-completed.
    expect(trace.steps[0]).toMatchObject({ key: "gate", status: "ok" });

    wrapper.unmount();
  });

  test("routes request rows to their owning step by group", async () => {
    const { wrapper, emit } = await mountWithTracker();

    // A research-half group (news) lands under the research step; a baseline
    // series group (indices) under the baseline step — requestStep's routing.
    emit({ run_id: "R1", seq: 2, kind: "request-started", group: "news", provider: "tavily", series_id: "n1", name: "news gather" });
    emit({ run_id: "R1", seq: 3, kind: "request-started", group: "indices", provider: "fmp", series_id: "spx", name: "S&P 500" });
    // request-finished updates the matching in-flight row rather than appending.
    emit({ run_id: "R1", seq: 4, kind: "request-finished", group: "indices", series_id: "spx", status: "ok" });
    await flushPromises();

    const steps = wrapper.findComponent(JobTrackerView).props("trace").steps;
    const research = steps.find((s) => s.key === "research");
    const baseline = steps.find((s) => s.key === "baseline");
    expect(research?.requests.map((r) => r.group)).toEqual(["news"]);
    expect(baseline?.requests).toHaveLength(1);
    expect(baseline?.requests[0]).toMatchObject({ seriesId: "spx", status: "ok" });

    wrapper.unmount();
  });

  test("accumulates streamed agent tokens onto the agent step", async () => {
    const { wrapper, emit } = await mountWithTracker();

    emit({ run_id: "R1", seq: 2, kind: "agent-token", delta: "Hello " });
    emit({ run_id: "R1", seq: 3, kind: "agent-token", delta: "world" });
    await flushPromises();

    const agent = wrapper
      .findComponent(JobTrackerView)
      .props("trace")
      .steps.find((s) => s.key === "agent");
    expect(agent?.agentText).toBe("Hello world");

    wrapper.unmount();
  });

  test("step-finished applies the reported status and detail to its step", async () => {
    const { wrapper, emit } = await mountWithTracker();

    // step-started flips the step to running; step-finished then writes both the
    // reported status and detail (App.vue's step-finished branch). The run-finished
    // reconcile test deliberately leaves a step running, so this is the only test
    // exercising the normal finished path's status + detail assignment.
    emit({ run_id: "R1", seq: 2, kind: "step-started", step: "baseline", label: "Baseline market data" });
    emit({ run_id: "R1", seq: 3, kind: "step-finished", step: "baseline", status: "ok", detail: "42 series" });
    await flushPromises();

    const baseline = wrapper
      .findComponent(JobTrackerView)
      .props("trace")
      .steps.find((s) => s.key === "baseline");
    expect(baseline).toMatchObject({ status: "ok", detail: "42 series" });

    wrapper.unmount();
  });

  test("run-finished sets the terminal state, stops the run, and reconciles a still-running step", async () => {
    const { wrapper, emit } = await mountWithTracker();

    emit({ run_id: "R1", seq: 2, kind: "step-started", step: "baseline", label: "Baseline market data" });
    emit({ run_id: "R1", seq: 3, kind: "run-finished", status: "cancelled" });
    await flushPromises();

    const tracker = wrapper.findComponent(JobTrackerView);
    expect(tracker.props("active")).toBe(false);
    const trace = tracker.props("trace");
    expect(trace.terminal).toMatchObject({ status: "cancelled" });
    // A step left "running" at the end is reconciled to the run's terminal flavor.
    expect(trace.steps.find((s) => s.key === "baseline")?.status).toBe("cancelled");

    wrapper.unmount();
  });

  test("ignores progress messages from a different run_id", async () => {
    const { wrapper, emit } = await mountWithTracker();

    const before = wrapper.findComponent(JobTrackerView).props("trace").steps.length;
    emit({ run_id: "OTHER", seq: 9, kind: "step-started", step: "ghost", label: "Ghost" });
    await flushPromises();

    const after = wrapper.findComponent(JobTrackerView).props("trace").steps;
    expect(after).toHaveLength(before);
    expect(after.some((s) => s.key === "ghost")).toBe(false);

    wrapper.unmount();
  });
});

// Report selection: a sidebar @select round-trips into load_report and feeds the
// report pane, with the open-failure and the list-vs-load error channels kept
// distinct. LatestReportView / RecentReportsSidebar props are the observables.
describe("App.vue report selection", () => {
  test("a sidebar @select loads the report into the pane", async () => {
    tauri.invoke.mockImplementation(
      makeInvokeRouter({ load_report: () => sampleReport })
    );
    const wrapper = mount(App);
    await flushPromises();

    wrapper.findComponent(RecentReportsSidebar).vm.$emit("select", "rep-1");
    await flushPromises();

    expect(tauri.invoke).toHaveBeenCalledWith("load_report", { reportId: "rep-1" });
    const latest = wrapper.findComponent(LatestReportView);
    expect(latest.props("report")).toEqual(sampleReport);
    expect(latest.props("loadError")).toBeNull();

    wrapper.unmount();
  });

  test("a failed load surfaces on loadError and clears the pane", async () => {
    tauri.invoke.mockImplementation(
      makeInvokeRouter({
        load_report: () => {
          throw new Error("markdown gone");
        },
      })
    );
    const wrapper = mount(App);
    await flushPromises();

    wrapper.findComponent(RecentReportsSidebar).vm.$emit("select", "rep-1");
    await flushPromises();

    const latest = wrapper.findComponent(LatestReportView);
    expect(latest.props("report")).toBeNull();
    expect(latest.props("loadError")).toContain("markdown gone");

    wrapper.unmount();
  });

  test("a list failure feeds reportsError, never the report pane's loadError", async () => {
    tauri.invoke.mockImplementation(
      makeInvokeRouter({
        list_reports: () => {
          throw new Error("db down");
        },
      })
    );
    const wrapper = mount(App);
    await flushPromises();

    expect(
      wrapper.findComponent(RecentReportsSidebar).props("reportsError")
    ).toContain("db down");
    expect(wrapper.findComponent(LatestReportView).props("loadError")).toBeNull();

    wrapper.unmount();
  });
});

// The cancel path + the tracker view toggle glue. A live run is on screen via
// mountWithTracker; cancelRun rides @cancel into invoke("cancel_run").
describe("App.vue cancel + tracker toggle", () => {
  test("@cancel invokes cancel_run and marks the request pending", async () => {
    const { wrapper } = await mountWithTracker({ cancel_run: () => null });

    wrapper.findComponent(JobTrackerView).vm.$emit("cancel");
    await flushPromises();

    expect(tauri.invoke).toHaveBeenCalledWith("cancel_run");
    expect(wrapper.findComponent(JobTrackerView).props("cancelRequested")).toBe(true);

    wrapper.unmount();
  });

  test("a failed cancel re-enables the button and surfaces on the job-status channel", async () => {
    const { wrapper } = await mountWithTracker({
      cancel_run: () => {
        throw new Error("cancel failed");
      },
    });

    wrapper.findComponent(JobTrackerView).vm.$emit("cancel");
    await flushPromises();

    expect(wrapper.findComponent(JobTrackerView).props("cancelRequested")).toBe(false);
    expect(wrapper.findComponent(JobStatusPanel).props("error")).toContain(
      "cancel failed"
    );

    wrapper.unmount();
  });

  test("@cancel after the run ended does not invoke cancel_run", async () => {
    const { wrapper, emit } = await mountWithTracker();
    emit({ run_id: "R1", seq: 2, kind: "run-finished", status: "successful" });
    await flushPromises();

    wrapper.findComponent(JobTrackerView).vm.$emit("cancel");
    await flushPromises();

    // The !runActive guard short-circuits before any invoke.
    expect(cancelRunCalls()).toHaveLength(0);

    wrapper.unmount();
  });

  test("@view-tracker shows the tracker and @close returns to the report", async () => {
    const { wrapper } = await mountWithTracker();
    // view-tracker already flipped the pane in the helper.
    expect(wrapper.findComponent(JobTrackerView).exists()).toBe(true);

    wrapper.findComponent(JobTrackerView).vm.$emit("close");
    await flushPromises();

    expect(wrapper.findComponent(JobTrackerView).exists()).toBe(false);
    expect(wrapper.findComponent(LatestReportView).exists()).toBe(true);

    wrapper.unmount();
  });
});
