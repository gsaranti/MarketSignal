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
import { makeInvokeRouter, unlisten, emitterFor, focusEmitter } from "../helpers/tauri";
import type { GeneratedReport, ReportSummary } from "../../src/types";

// A controllable promise for the interleaving tests below — kept local to this
// spec (test-mechanics, not a Tauri double, so it stays out of helpers/tauri.ts).
// Lets a test hold an invoke pending while it drives other events, then settle it
// to pin a guard that only a deterministic ordering can exercise without flake.
function deferred<T>(): {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason?: unknown) => void;
} {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

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

// A distinct second report for the no-yank and latest-load-wins tests. Read-only,
// like the fixtures above (the shallow module-level spread is fine while specs
// only read props).
const sampleSummary2: ReportSummary = {
  ...sampleSummary,
  report_id: "rep-2",
  created_at: "2026-06-21T09:00:00Z",
};
const sampleReport2: GeneratedReport = {
  report_id: "rep-2",
  markdown: "# Weekly Market Signal\n\nSecond body.",
  markdown_path: "/reports/2026-06-21-market-signal-weekly-report.md",
  summary: sampleSummary2,
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

// generate(): the manual-run kickoff wired from JobStatusPanel @generate. Covers
// the blocked-gate short-circuit, the happy path (fresh report into the pane), and
// the pre-tracker failure surfacing on the report pane's error channel. The
// interleaved cancel-suppression branch lives in the deferred-harness block below.
describe("App.vue generate", () => {
  test("a Generate emit invokes generate_report_manual and lands the fresh report", async () => {
    tauri.invoke.mockImplementation(
      makeInvokeRouter({ generate_report_manual: () => sampleReport })
    );
    const wrapper = mount(App);
    await flushPromises();

    wrapper.findComponent(JobStatusPanel).vm.$emit("generate");
    await flushPromises();

    expect(tauri.invoke).toHaveBeenCalledWith("generate_report_manual");
    // reportPaneMode was "tracker" at kickoff, so success swaps in the report and
    // settles the pane back to the report view.
    expect(wrapper.findComponent(LatestReportView).props("report")).toEqual(sampleReport);

    wrapper.unmount();
  });

  test("a blocked config short-circuits Generate before any invoke", async () => {
    tauri.invoke.mockImplementation(
      makeInvokeRouter({
        check_configuration: () => ({ categories: [], is_blocked: true }),
        generate_report_manual: () => sampleReport,
      })
    );
    const wrapper = mount(App);
    await flushPromises();

    wrapper.findComponent(JobStatusPanel).vm.$emit("generate");
    await flushPromises();

    // The `if (blocked.value) return` guard fires before the command.
    expect(invokedCommands()).not.toContain("generate_report_manual");

    wrapper.unmount();
  });

  test("a failure before run-started surfaces on the report pane's error", async () => {
    tauri.invoke.mockImplementation(
      makeInvokeRouter({
        generate_report_manual: () => {
          throw new Error("gate slammed");
        },
      })
    );
    const wrapper = mount(App);
    await flushPromises();

    wrapper.findComponent(JobStatusPanel).vm.$emit("generate");
    await flushPromises();

    // No run-started arrived, so runTrace stayed null and the catch routes the
    // reason to the report pane (the skip / pre-tracker-error branch).
    const latest = wrapper.findComponent(LatestReportView);
    expect(latest.exists()).toBe(true);
    expect(latest.props("error")).toContain("gate slammed");

    wrapper.unmount();
  });
});

// The job-finished listener: the scheduler emits a GeneratedReport (success) or
// null (failure / skip / missed) when a run ends, so an open window updates
// without a manual refresh. Driven through the captured "job-finished" emitter.
describe("App.vue job-finished listener", () => {
  test("a payload while watching the tracker lands the report", async () => {
    const wrapper = mount(App);
    await flushPromises();
    const finished = emitterFor(tauri.listen, "job-finished");

    // Put the user on the tracker pane (footer handle), then deliver the report.
    wrapper.findComponent(JobStatusPanel).vm.$emit("view-tracker");
    await flushPromises();
    finished(sampleReport);
    await flushPromises();

    const latest = wrapper.findComponent(LatestReportView);
    expect(latest.exists()).toBe(true);
    expect(latest.props("report")).toEqual(sampleReport);

    wrapper.unmount();
  });

  test("a payload while reading another report does not yank the pane", async () => {
    tauri.invoke.mockImplementation(
      makeInvokeRouter({
        load_report: () => sampleReport,
        // Keep rep-1 "still selected" so the post-finish list refresh doesn't
        // blank-pane auto-select onto the new report and confound the assertion.
        list_reports: () => [sampleSummary2, sampleSummary],
      })
    );
    const wrapper = mount(App);
    await flushPromises();
    const finished = emitterFor(tauri.listen, "job-finished");

    // Reading report rep-1 (reportPaneMode stays "report").
    wrapper.findComponent(RecentReportsSidebar).vm.$emit("select", "rep-1");
    await flushPromises();
    expect(wrapper.findComponent(LatestReportView).props("report")).toEqual(sampleReport);

    // A scheduled rep-2 finishes; the reader must not be moved off rep-1.
    finished(sampleReport2);
    await flushPromises();
    expect(wrapper.findComponent(LatestReportView).props("report")).toEqual(sampleReport);

    wrapper.unmount();
  });

  test("a null payload refreshes status but loads no report", async () => {
    const wrapper = mount(App);
    await flushPromises();
    const finished = emitterFor(tauri.listen, "job-finished");

    tauri.invoke.mockClear();
    finished(null);
    await flushPromises();

    // The null path refreshes validation / status / inbox / archive, but not the
    // report list — refreshReports lives inside the `if (event.payload)` arm.
    const cmds = invokedCommands();
    expect(cmds).toEqual(
      expect.arrayContaining([
        "check_configuration",
        "job_status",
        "list_research_inbox",
        "list_research_archive",
      ])
    );
    expect(cmds).not.toContain("list_reports");
    expect(wrapper.findComponent(LatestReportView).props("report")).toBeNull();

    wrapper.unmount();
  });
});

// The window focus-refresh path: regaining focus re-checks config / status and
// re-lists reports + both research folders, so a missed window or a background
// scheduled run surfaces on return. Driven through the captured onFocusChanged
// callback (focusEmitter).
describe("App.vue focus refresh", () => {
  test("regaining focus refreshes config, status, reports, and both folders", async () => {
    const wrapper = mount(App);
    await flushPromises();
    const focus = focusEmitter(tauri.onFocusChanged);

    tauri.invoke.mockClear();
    focus(true);
    await flushPromises();

    // Exactly the five refresh commands the focus handler issues — sorted-equality
    // so it's a true set check with no extras.
    expect([...invokedCommands()].sort()).toEqual([
      "check_configuration",
      "job_status",
      "list_reports",
      "list_research_archive",
      "list_research_inbox",
    ]);

    wrapper.unmount();
  });

  test("losing focus refreshes nothing", async () => {
    const wrapper = mount(App);
    await flushPromises();
    const focus = focusEmitter(tauri.onFocusChanged);

    tauri.invoke.mockClear();
    focus(false);
    await flushPromises();

    // The `if (focused)` guard short-circuits a blur.
    expect(invokedCommands()).toHaveLength(0);

    wrapper.unmount();
  });
});

// Interleaving guards that only a controllable promise can pin deterministically:
// generate()'s cancel-suppression and selectReport()'s latest-load-wins race.
describe("App.vue interleaved guards", () => {
  test("a cancelled generate suppresses the error surface", async () => {
    const pending = deferred<GeneratedReport>();
    tauri.invoke.mockImplementation(
      makeInvokeRouter({
        generate_report_manual: () => pending.promise,
        cancel_run: () => null,
      })
    );
    const wrapper = mount(App);
    await flushPromises();
    const emit = emitterFor(tauri.listen, "job-progress");

    // Kick off the run; it parks on the pending invoke. run-started then builds the
    // trace so JobTrackerView renders and its @cancel becomes reachable.
    wrapper.findComponent(JobStatusPanel).vm.$emit("generate");
    await flushPromises();
    emit({ run_id: "R1", seq: 1, kind: "run-started", label: "Weekly run" });
    await flushPromises();

    // User cancels (sets cancelRequested), then the run rejects — the cancel branch
    // must swallow the error rather than surface it.
    wrapper.findComponent(JobTrackerView).vm.$emit("cancel");
    await flushPromises();
    pending.reject(new Error("aborted by cancel"));
    await flushPromises();

    expect(cancelRunCalls()).toHaveLength(1);
    // Leave the tracker to reveal the report pane: its error channel stayed clean
    // (contrast the pre-tracker-error test, where the same throw surfaces).
    wrapper.findComponent(JobTrackerView).vm.$emit("close");
    await flushPromises();
    expect(wrapper.findComponent(LatestReportView).props("error")).toBeNull();

    wrapper.unmount();
  });

  test("a slow earlier load loses to a newer selection", async () => {
    const slow = deferred<GeneratedReport>();
    tauri.invoke.mockImplementation(
      makeInvokeRouter({
        load_report: (args) =>
          args?.reportId === "rep-1" ? slow.promise : sampleReport2,
      })
    );
    const wrapper = mount(App);
    await flushPromises();

    const sidebar = wrapper.findComponent(RecentReportsSidebar);
    sidebar.vm.$emit("select", "rep-1"); // parks on the slow load
    sidebar.vm.$emit("select", "rep-2"); // newer selection resolves first
    await flushPromises();
    expect(wrapper.findComponent(LatestReportView).props("report")).toEqual(sampleReport2);

    // The stale rep-1 load resolves late; the selectedReportId guard drops it so it
    // can't clobber the newer report.
    slow.resolve(sampleReport);
    await flushPromises();
    expect(wrapper.findComponent(LatestReportView).props("report")).toEqual(sampleReport2);

    wrapper.unmount();
  });
});
