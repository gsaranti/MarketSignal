// First SFC spec to exercise the `@tauri-apps/api` boundary. App.vue is the only
// component that imports it — four modules: core (`invoke`), event (`listen`),
// window (`getCurrentWindow().onFocusChanged`), and app (`getVersion`).
//
// This establishes the project's Tauri-mock pattern. `vi.mock` factories are
// hoisted above imports, so the mock *functions* are declared via `vi.hoisted`
// and their *implementations* come from `tests/helpers/tauri.ts` in `beforeEach`.
// `vitest.config.ts` sets `globals: false`, so every test helper (incl. `vi`) is
// imported explicitly.
//
// Two assertions: (1) App's `onMounted` bootstrap contract — the exact command /
// listener / window / version set it fires on mount, which doubles as proof the
// four-module mock is complete enough to mount the real App; (2) the `@save`
// wiring round-trips a child emit into `invoke("save_settings", payload)`.

import { describe, test, expect, beforeEach, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { makeInvokeRouter, unlisten, emitterFor, focusEmitter } from "../helpers/tauri";
import { deepFreeze } from "../helpers/freeze";
import type {
  GeneratedReport,
  ReportSummary,
  TruncationStats,
} from "../../src/types";

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
import PortfolioView from "../../src/components/PortfolioView.vue";
import ConfirmDialog from "../../src/components/ConfirmDialog.vue";
import { sampleHoldingsPull, samplePortfolioRun } from "../helpers/tauri";

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

// A distinct second report for the no-yank and latest-load-wins tests.
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

// These fixtures are shared at module scope and spread shallowly (sampleSummary2
// reuses sampleSummary's arrays; both reports nest a summary; all are handed to
// child props / returned from the mocked invoke), so a single in-place mutation
// would leak across tests. They're read-only by design — deep-freezing makes that
// a guarantee, turning any future in-place write into a loud throw at the mutation
// site. Freezing each report transitively freezes its nested summary and arrays.
deepFreeze(sampleReport);
deepFreeze(sampleReport2);

describe("App.vue Tauri boundary", () => {
  test("mounts against the mock and fires the onMounted bootstrap cascade", async () => {
    const wrapper = mount(App);
    await flushPromises();

    // Version read, the focus subscription via getCurrentWindow(), and both
    // event listeners — all real onMounted glue.
    expect(tauri.getVersion).toHaveBeenCalledTimes(1);
    expect(tauri.getCurrentWindow).toHaveBeenCalled();
    expect(tauri.onFocusChanged).toHaveBeenCalledTimes(1);

    // Exactly the one listener — sorted-equality so the assertion is a true set
    // check (catches a dropped or duplicated listener), order-agnostic.
    const events = tauri.listen.mock.calls.map((c) => c[0]).sort();
    expect(events).toEqual(["job-progress"]);

    // Exactly the refresh commands onMounted issues — sorted-equality
    // enforces the set with no extras or duplicates (not a subset), while
    // tolerating any reordering of the onMounted calls. Proves the four-module
    // mock is complete enough to mount the real App without a throw.
    // `truncation_stats` and `schwab_status` ride in via refreshSettings (loaded up
    // front for the gate's config state), alongside get_settings; the local-suite
    // presence gate + the Portfolio page's persisted state load up front too.
    expect([...invokedCommands()].sort()).toEqual([
      "check_configuration",
      "check_local_configuration",
      "get_settings",
      "job_status",
      "latest_holdings_pull",
      "latest_portfolio_run",
      "list_portfolio_runs",
      "list_reports",
      "list_research_archive",
      "list_research_inbox",
      "schwab_status",
      "truncation_stats",
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

  test("the loaded truncation_stats reaches the Settings prop", async () => {
    const stats: TruncationStats = {
      total_truncations: 7,
      total_docs_parsed: 40,
      unaligned_truncations: 0,
      total_original_chars: 320000,
      parse_runs_missing_original_chars: 0,
      reports_affected: 4,
      total_chars_dropped: 54321,
      by_format: [{ format: "pdf", count: 7 }],
      latest_captured_at: "2026-06-08T09:00:00+00:00",
    };
    tauri.invoke.mockImplementation(
      makeInvokeRouter({ truncation_stats: () => stats })
    );
    const wrapper = mount(App);
    await flushPromises();

    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "settings");
    await flushPromises();

    // App.vue is <script setup> with no defineExpose — assert through the child
    // prop, the spec's load-bearing read idiom.
    const settings = wrapper.findComponent(Settings);
    expect(settings.props("truncationStats")).toEqual(stats);

    wrapper.unmount();
  });

  test("the loaded schwab_status reaches the Settings prop", async () => {
    const status = {
      client_id: "client-abc",
      secret_configured: true,
      connection: "connected",
      refresh_expires_at: "2026-07-09T00:00:00+00:00",
    };
    tauri.invoke.mockImplementation(
      makeInvokeRouter({ schwab_status: () => status })
    );
    const wrapper = mount(App);
    await flushPromises();

    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "settings");
    await flushPromises();

    expect(wrapper.findComponent(Settings).props("schwabStatus")).toEqual(status);

    wrapper.unmount();
  });

  test("a Settings @save-schwab emit round-trips into save_schwab_credentials (camelCase args)", async () => {
    const wrapper = mount(App);
    await flushPromises();
    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "settings");
    await flushPromises();

    const settings = wrapper.findComponent(Settings);
    settings.vm.$emit("save-schwab", {
      client_id: "client-abc",
      client_secret: "dev-secret",
    });
    await flushPromises();

    // Tauri maps JS camelCase args to the command's snake_case params.
    expect(tauri.invoke).toHaveBeenCalledWith("save_schwab_credentials", {
      clientId: "client-abc",
      clientSecret: "dev-secret",
    });

    wrapper.unmount();
  });

  test("a Settings @connect-schwab emit invokes schwab_connect then re-reads schwab_status", async () => {
    const wrapper = mount(App);
    await flushPromises();
    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "settings");
    await flushPromises();
    // Only count schwab_status reads that follow the connect, not the mount cascade.
    const before = tauri.invoke.mock.calls.filter(
      (c: unknown[]) => c[0] === "schwab_status"
    ).length;

    wrapper.findComponent(Settings).vm.$emit("connect-schwab");
    await flushPromises();

    expect(tauri.invoke).toHaveBeenCalledWith("schwab_connect");
    const after = tauri.invoke.mock.calls.filter(
      (c: unknown[]) => c[0] === "schwab_status"
    ).length;
    expect(after).toBeGreaterThan(before);

    wrapper.unmount();
  });

  test("a pending schwab_connect shows the footer's Schwab-labeled running row, then re-reads job_status", async () => {
    // The connect holds the single global run slot, so the footer's running row is
    // correct to appear — but it must say what's actually running, never
    // "Generating report…" (the original mislabel this pins against).
    const connect = deferred<null>();
    tauri.invoke.mockImplementation(
      makeInvokeRouter({ schwab_connect: () => connect.promise })
    );
    const wrapper = mount(App);
    await flushPromises();
    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "settings");
    await flushPromises();

    wrapper.findComponent(Settings).vm.$emit("connect-schwab");
    await flushPromises();
    const panel = wrapper.findComponent(JobStatusPanel);
    expect(panel.props("runActive")).toBe(true);
    expect(panel.props("runningLabel")).toBe("Connecting to Charles Schwab…");

    // Settling the connect drops the running row and re-syncs the run-slot view
    // (a focus bounce mid-login may have polled `is_running: true`).
    const before = tauri.invoke.mock.calls.filter(
      (c: unknown[]) => c[0] === "job_status"
    ).length;
    connect.resolve(null);
    await flushPromises();
    expect(panel.props("runActive")).toBe(false);
    const after = tauri.invoke.mock.calls.filter(
      (c: unknown[]) => c[0] === "job_status"
    ).length;
    expect(after).toBeGreaterThan(before);

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

// The window focus-refresh path: regaining focus re-checks config / status and
// re-lists reports + both research folders, so external changes (e.g. files
// dropped into the inbox via Finder) surface on return. Driven through the
// captured onFocusChanged callback (focusEmitter).
describe("App.vue focus refresh", () => {
  test("regaining focus refreshes config, status, reports, and both folders", async () => {
    const wrapper = mount(App);
    await flushPromises();
    const focus = focusEmitter(tauri.onFocusChanged);

    tauri.invoke.mockClear();
    focus(true);
    await flushPromises();

    // Exactly the refresh commands the focus handler issues — sorted-equality
    // so it's a true set check with no extras (the local presence gate rides
    // along so a Schwab/local-models change surfaces on return).
    expect([...invokedCommands()].sort()).toEqual([
      "check_configuration",
      "check_local_configuration",
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

// The Portfolio page's App-side wiring: navigation, the local-suite warning
// merge (band shows both gates' categories; the cloud report gate is
// untouched), the run flow, and the tracker's owning-page placement with no
// bogus /8 fraction (the fixed denominator is the report pipeline's).
describe("App.vue portfolio wiring", () => {
  test("sidebar Portfolio nav shows PortfolioView with the persisted run", async () => {
    tauri.invoke.mockImplementation(
      makeInvokeRouter({ latest_portfolio_run: () => samplePortfolioRun })
    );
    const wrapper = mount(App);
    await flushPromises();

    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "portfolio");
    await flushPromises();

    const portfolio = wrapper.findComponent(PortfolioView);
    expect(portfolio.exists()).toBe(true);
    expect(portfolio.props("run")).toEqual(samplePortfolioRun);
    wrapper.unmount();
  });

  test("opening a past run renders it read-only; the sidebar lists the history", async () => {
    const oldRun = { ...samplePortfolioRun, run_id: "prun-old" };
    tauri.invoke.mockImplementation(
      makeInvokeRouter({
        latest_portfolio_run: () => samplePortfolioRun,
        list_portfolio_runs: () => [
          {
            run_id: samplePortfolioRun.run_id,
            created_at: samplePortfolioRun.created_at,
            holdings_count: 1,
            graded_count: 1,
          },
          {
            run_id: "prun-old",
            created_at: "2026-06-01T12:00:00Z",
            holdings_count: 1,
            graded_count: 0,
          },
        ],
        get_portfolio_run: (args) =>
          args?.runId === "prun-old" ? oldRun : null,
      })
    );
    const wrapper = mount(App);
    await flushPromises();

    const sidebar = wrapper.findComponent(RecentReportsSidebar);
    expect(sidebar.props("portfolioRuns")).toHaveLength(2);

    // Selecting the older run opens it read-only on the Portfolio page.
    sidebar.vm.$emit("select-run", "prun-old");
    await flushPromises();
    const portfolio = wrapper.findComponent(PortfolioView);
    expect(portfolio.props("run")).toEqual(oldRun);
    expect(portfolio.props("historical")).toBe(true);
    expect(sidebar.props("selectedRunId")).toBe("prun-old");

    // Back to latest restores the live view.
    portfolio.vm.$emit("back-to-latest");
    await flushPromises();
    expect(portfolio.props("run")).toEqual(samplePortfolioRun);
    expect(portfolio.props("historical")).toBe(false);

    // Selecting the newest row never enters the historical state.
    sidebar.vm.$emit("select-run", samplePortfolioRun.run_id);
    await flushPromises();
    expect(portfolio.props("historical")).toBe(false);
    wrapper.unmount();
  });

  test("a past-run open failure lands on its own channel and clears on back-to-latest", async () => {
    tauri.invoke.mockImplementation(
      makeInvokeRouter({
        latest_portfolio_run: () => samplePortfolioRun,
        list_portfolio_runs: () => [
          {
            run_id: "prun-old",
            created_at: "2026-06-01T12:00:00Z",
            holdings_count: 1,
            graded_count: 0,
          },
        ],
        get_portfolio_run: () => {
          throw new Error("run row unreadable");
        },
      })
    );
    const wrapper = mount(App);
    await flushPromises();

    const sidebar = wrapper.findComponent(RecentReportsSidebar);
    sidebar.vm.$emit("select-run", "prun-old");
    await flushPromises();

    const portfolio = wrapper.findComponent(PortfolioView);
    expect(portfolio.props("historyError")).toContain("run row unreadable");
    // The general run-error channel stays untouched — the failure must never
    // read as a job failure.
    expect(portfolio.props("runError")).toBeNull();
    // The latest view stayed up; returning to latest clears the message.
    expect(portfolio.props("historical")).toBe(false);
    portfolio.vm.$emit("back-to-latest");
    await flushPromises();
    expect(portfolio.props("historyError")).toBeNull();
    wrapper.unmount();
  });

  test("a slow past-run fetch cannot reopen the view after a newer selection closed it", async () => {
    const oldRun = { ...samplePortfolioRun, run_id: "prun-old" };
    const slow = deferred<typeof samplePortfolioRun>();
    tauri.invoke.mockImplementation(
      makeInvokeRouter({
        latest_portfolio_run: () => samplePortfolioRun,
        list_portfolio_runs: () => [
          {
            run_id: samplePortfolioRun.run_id,
            created_at: samplePortfolioRun.created_at,
            holdings_count: 1,
            graded_count: 1,
          },
          {
            run_id: "prun-old",
            created_at: "2026-06-01T12:00:00Z",
            holdings_count: 1,
            graded_count: 0,
          },
        ],
        get_portfolio_run: () => slow.promise,
      })
    );
    const wrapper = mount(App);
    await flushPromises();

    const sidebar = wrapper.findComponent(RecentReportsSidebar);
    sidebar.vm.$emit("select-run", "prun-old"); // parks on the slow fetch
    sidebar.vm.$emit("select-run", samplePortfolioRun.run_id); // supersedes → latest
    await flushPromises();

    slow.resolve(oldRun); // the stale fetch lands late…
    await flushPromises();
    // …and is discarded: the page stays on the live latest view.
    const portfolio = wrapper.findComponent(PortfolioView);
    expect(portfolio.props("historical")).toBe(false);
    expect(portfolio.props("run")).toEqual(samplePortfolioRun);
    wrapper.unmount();
  });

  test("local presence categories join the warning band without blocking the report gate", async () => {
    tauri.invoke.mockImplementation(
      makeInvokeRouter({
        check_local_configuration: () => ({
          categories: [
            {
              kind: "schwab",
              title: "Charles Schwab connection",
              items: ["Schwab account not connected."],
              dismiss_id: null,
            },
          ],
          is_blocked: true,
        }),
      })
    );
    const wrapper = mount(App);
    await flushPromises();

    // The band renders the local category…
    expect(wrapper.text()).toContain("Charles Schwab connection");
    // …but the report gate stays unblocked (cloud validation is clean), and the
    // Portfolio triggers are locked.
    expect(wrapper.findComponent(JobStatusPanel).props("blocked")).toBe(false);
    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "portfolio");
    await flushPromises();
    const portfolio = wrapper.findComponent(PortfolioView);
    expect(portfolio.props("runBlocked")).toBe(true);
    expect(portfolio.props("pullBlocked")).toBe(true);
    wrapper.unmount();
  });

  test("a portfolio run lands its result on the page, and its tracker shows no /8 fraction", async () => {
    const pending = deferred<typeof samplePortfolioRun>();
    // Stateful like the real store: after the run resolves, the defensive
    // latest_portfolio_run re-read (generatePortfolio's finally) returns the
    // persisted run instead of clobbering the inline result with null.
    let persisted: typeof samplePortfolioRun | null = null;
    tauri.invoke.mockImplementation(
      makeInvokeRouter({
        generate_portfolio_manual: () => pending.promise,
        latest_portfolio_run: () => persisted,
      })
    );
    const wrapper = mount(App);
    await flushPromises();

    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "portfolio");
    await flushPromises();
    wrapper.findComponent(PortfolioView).vm.$emit("run");
    await flushPromises();

    // The run streams into the shared tracker, which replaces the Portfolio
    // page (owning-page placement) — and the footer's determinate fraction
    // stays null: the /8 denominator belongs to the report pipeline only.
    const emit = emitterFor(tauri.listen, "job-progress");
    emit({ run_id: "P1", seq: 1, kind: "run-started", label: "Portfolio Analysis" });
    await flushPromises();
    expect(wrapper.findComponent(JobTrackerView).exists()).toBe(true);
    expect(wrapper.findComponent(PortfolioView).exists()).toBe(false);
    expect(wrapper.findComponent(JobStatusPanel).props("progress")).toBeNull();

    emit({ run_id: "P1", seq: 2, kind: "run-finished", status: "successful" });
    persisted = samplePortfolioRun;
    pending.resolve(samplePortfolioRun);
    await flushPromises();

    const portfolio = wrapper.findComponent(PortfolioView);
    expect(portfolio.exists()).toBe(true);
    expect(portfolio.props("run")).toEqual(samplePortfolioRun);
    wrapper.unmount();
  });

  test("a failed pull re-derives the presence gate so the band can't go stale", async () => {
    tauri.invoke.mockImplementation(
      makeInvokeRouter({
        pull_holdings: () => {
          throw new Error("Schwab account not connected — weekly re-login required.");
        },
      })
    );
    const wrapper = mount(App);
    await flushPromises();
    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "portfolio");
    await flushPromises();

    tauri.invoke.mockClear();
    wrapper.findComponent(PortfolioView).vm.$emit("pull");
    await flushPromises();

    // The failure is inline on the page, and the presence gate is re-read so a
    // mid-session lapse re-locks the triggers without waiting for focus.
    expect(wrapper.findComponent(PortfolioView).props("runError")).toContain(
      "not connected"
    );
    expect(invokedCommands()).toContain("check_local_configuration");
    wrapper.unmount();
  });

  test("a stale portfolio read can't overwrite a fresher run result", async () => {
    // The mount-time read (#1) is held open across a full run; when it finally
    // resolves with pre-run (null) state, the epoch guard must discard it.
    const slow = deferred<typeof samplePortfolioRun | null>();
    let persisted: typeof samplePortfolioRun | null = null;
    let reads = 0;
    tauri.invoke.mockImplementation(
      makeInvokeRouter({
        latest_portfolio_run: () => (++reads === 1 ? slow.promise : persisted),
        generate_portfolio_manual: () => {
          persisted = samplePortfolioRun;
          return samplePortfolioRun;
        },
      })
    );
    const wrapper = mount(App);
    await flushPromises(); // read #1 hangs

    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "portfolio");
    await flushPromises(); // read #2 resolves (still null — nothing persisted)
    wrapper.findComponent(PortfolioView).vm.$emit("run");
    await flushPromises(); // run lands inline; finally's read #3 returns the run

    slow.resolve(null);
    await flushPromises();
    expect(wrapper.findComponent(PortfolioView).props("run")).toEqual(
      samplePortfolioRun
    );
    wrapper.unmount();
  });

  test("a successful pull settles the loading flag a superseded read left behind", async () => {
    // Mount's read (#1) hangs; the pull's direct assignment supersedes it, so
    // the discarded read's finally won't clear portfolioLoading — the pull
    // path must settle it itself, and the stale resolution must change nothing.
    const slow = deferred<null>();
    let reads = 0;
    tauri.invoke.mockImplementation(
      makeInvokeRouter({
        latest_portfolio_run: () => (++reads === 1 ? slow.promise : null),
      })
    );
    const wrapper = mount(App);
    await flushPromises();
    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "portfolio");
    await flushPromises();

    wrapper.findComponent(PortfolioView).vm.$emit("pull");
    await flushPromises();
    const view = wrapper.findComponent(PortfolioView);
    expect(view.props("loading")).toBe(false);
    expect(view.props("pull")).toEqual(sampleHoldingsPull);

    slow.resolve(null);
    await flushPromises();
    expect(view.props("loading")).toBe(false);
    expect(view.props("pull")).toEqual(sampleHoldingsPull);
    wrapper.unmount();
  });

  test("a run blocked before any event surfaces inline on the page, not a warning", async () => {
    tauri.invoke.mockImplementation(
      makeInvokeRouter({
        generate_portfolio_manual: () => {
          throw new Error("Daemon unreachable: connection refused.");
        },
      })
    );
    const wrapper = mount(App);
    await flushPromises();

    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "portfolio");
    await flushPromises();
    wrapper.findComponent(PortfolioView).vm.$emit("run");
    await flushPromises();

    expect(wrapper.findComponent(PortfolioView).props("runError")).toContain(
      "Daemon unreachable"
    );
    wrapper.unmount();
  });
});

// The data-import fork (docs/data-portability.md §Import flow): an inspected
// archive either loads straight into an empty store or parks on the replace-all
// ConfirmDialog, whose confirm is the only path to `import_data` with
// `replace: true`. Wired through the real Settings emit and the real dialog.
describe("App.vue data-import fork", () => {
  const inspection = {
    path: "/tmp/market-signal-export-2026-07-05.zip",
    store_empty: false,
    info: {
      encrypted: false,
      format_version: 1,
      app_version: "1.2.1",
      created_at: "2026-07-05T12:00:00Z",
      reports: 30,
      learnings: 214,
      snapshots: 14,
      portfolio_runs: 2,
      holdings_pulls: 1,
      files: 34,
    },
  };
  const importSummary = {
    reports: 30,
    learnings: 214,
    snapshots: 14,
    portfolio_runs: 2,
    holdings_pulls: 1,
    files: 34,
    skipped_reports: 0,
  };

  async function mountOnSettings(
    overrides: Record<string, (args?: Record<string, unknown>) => unknown>
  ) {
    tauri.invoke.mockImplementation(makeInvokeRouter(overrides));
    const wrapper = mount(App);
    await flushPromises();
    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "settings");
    await flushPromises();
    return wrapper;
  }

  test("a non-empty store parks on the dialog — with the archive's date and counts — and confirm commits with replace", async () => {
    const wrapper = await mountOnSettings({
      import_data_inspect: () => inspection,
      import_data: () => importSummary,
    });

    wrapper.findComponent(Settings).vm.$emit("import-data", "pw");
    await flushPromises();

    // Nothing destructive yet: the inspection parked on the open dialog, whose
    // detail line describes the actual picked artifact.
    expect(invokedCommands()).not.toContain("import_data");
    const dialog = wrapper.findComponent(ConfirmDialog);
    expect(dialog.props("open")).toBe(true);
    expect(dialog.props("detail")).toContain("2026-07-05");
    expect(dialog.props("detail")).toContain("30 reports");
    expect(dialog.props("detail")).toContain("214 learnings");

    dialog.vm.$emit("confirm");
    await flushPromises();

    // The confirmed commit carries the inspected path, the passphrase, and the
    // explicit replace; the dialog closes and the store-reading surfaces refetch.
    expect(tauri.invoke).toHaveBeenCalledWith("import_data", {
      path: inspection.path,
      passphrase: "pw",
      replace: true,
    });
    expect(wrapper.findComponent(ConfirmDialog).props("open")).toBe(false);
    expect(invokedCommands()).toContain("list_reports");
    expect(wrapper.findComponent(Settings).props("dataStatus")).toContain(
      "Imported 30 reports"
    );

    wrapper.unmount();
  });

  test("an empty store loads straight in — no dialog, replace stays false", async () => {
    const wrapper = await mountOnSettings({
      import_data_inspect: () => ({ ...inspection, store_empty: true }),
      import_data: () => importSummary,
    });

    wrapper.findComponent(Settings).vm.$emit("import-data", "");
    await flushPromises();

    expect(wrapper.findComponent(ConfirmDialog).props("open")).toBe(false);
    expect(tauri.invoke).toHaveBeenCalledWith("import_data", {
      path: inspection.path,
      passphrase: null,
      replace: false,
    });

    wrapper.unmount();
  });

  test("a cancelled dialog never reaches import_data", async () => {
    const wrapper = await mountOnSettings({
      import_data_inspect: () => inspection,
    });

    wrapper.findComponent(Settings).vm.$emit("import-data", "");
    await flushPromises();
    const dialog = wrapper.findComponent(ConfirmDialog);
    expect(dialog.props("open")).toBe(true);

    dialog.vm.$emit("cancel");
    await flushPromises();

    expect(wrapper.findComponent(ConfirmDialog).props("open")).toBe(false);
    expect(invokedCommands()).not.toContain("import_data");

    wrapper.unmount();
  });
});
