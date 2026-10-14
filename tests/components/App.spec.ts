// App.vue is the only component that imports `@tauri-apps/api`. `vi.mock`
// factories are hoisted above imports, so the mock functions are declared via
// `vi.hoisted` and their implementations come from `tests/helpers/tauri.ts` in
// `beforeEach`. `vitest.config.ts` sets `globals: false`, so `vi` is imported.

import { describe, test, expect, beforeEach, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import {
  makeInvokeRouter,
  defaultSettings,
  unlisten,
  emitterFor,
  focusEmitter,
} from "../helpers/tauri";
import { deepFreeze } from "../helpers/freeze";
import type {
  GeneratedReport,
  ReportSummary,
  TruncationStats,
} from "../../src/types";

// Holds an invoke pending while a test drives other events, then settles it.
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

// Mounts App, settles onMounted, puts a live run on screen and flips the report
// pane to the tracker; returns the captured "job-progress" emitter.
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

deepFreeze(sampleReport);
deepFreeze(sampleReport2);

describe("App.vue Tauri boundary", () => {
  test("mounts against the mock and fires the onMounted bootstrap cascade", async () => {
    const wrapper = mount(App);
    await flushPromises();

    expect(tauri.getVersion).toHaveBeenCalledTimes(1);
    expect(tauri.getCurrentWindow).toHaveBeenCalled();
    expect(tauri.onFocusChanged).toHaveBeenCalledTimes(1);

    const events = tauri.listen.mock.calls.map((c) => c[0]).sort();
    expect(events).toEqual(["job-progress"]);

    // Sorted equality: the exact set, no extras or duplicates, order-agnostic.
    expect([...invokedCommands()].sort()).toEqual([
      "check_configuration",
      "check_local_configuration",
      "get_investor_profile",
      "get_settings",
      "job_status",
      "latest_holdings_pull",
      "latest_portfolio_run",
      "latest_quick_check",
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

    // App.vue has no defineExpose, so state is read through child props.
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
    // The connect holds the single global run slot, so the running row is
    // correct to appear.
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

    // A focus bounce mid-login may have polled `is_running: true`, so settling
    // re-reads job_status.
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

  test("the footer's section + showGenerate props follow the view: stamps map per feature, the trigger is report-view-only", async () => {
    // The featureOf mapping (docs/interface.md §Main Layout); only App owns both
    // view→prop mappings.
    const wrapper = mount(App);
    await flushPromises();
    const panel = wrapper.findComponent(JobStatusPanel);
    expect(panel.props("section")).toBe("report");
    expect(panel.props("showGenerate")).toBe(true);

    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "portfolio");
    await flushPromises();
    expect(panel.props("section")).toBe("portfolio");
    expect(panel.props("showGenerate")).toBe(false);

    for (const view of ["inbox", "archive", "settings"] as const) {
      wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", view);
      await flushPromises();
      expect(panel.props("section")).toBe("report");
      expect(panel.props("showGenerate")).toBe(false);
    }

    wrapper.unmount();
  });
});

// The folded RunTrace is read through JobTrackerView's `trace` prop.
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

  test("request rows land in the step their event's stamp names", async () => {
    const { wrapper, emit } = await mountWithTracker();

    // Ownership is stamped by the backend (progress.rs); group names play no
    // routing role.
    emit({ run_id: "R1", seq: 2, kind: "step-started", step: "baseline", label: "Baseline market data" });
    emit({ run_id: "R1", seq: 3, kind: "request-started", step: "baseline", group: "indices", provider: "fmp", series_id: "spx", name: "S&P 500" });
    // request-finished updates the matching in-flight row rather than appending.
    emit({ run_id: "R1", seq: 4, kind: "request-finished", step: "baseline", group: "indices", series_id: "spx", status: "ok" });
    emit({ run_id: "R1", seq: 5, kind: "step-finished", step: "baseline", status: "ok" });
    emit({ run_id: "R1", seq: 6, kind: "step-started", step: "research", label: "Gathering and condensing research" });
    emit({ run_id: "R1", seq: 7, kind: "request-started", step: "research", group: "news", provider: "tavily", series_id: "n1", name: "news gather" });
    await flushPromises();

    const steps = wrapper.findComponent(JobTrackerView).props("trace").steps;
    const research = steps.find((s) => s.key === "research");
    const baseline = steps.find((s) => s.key === "baseline");
    expect(research?.requests.map((r) => r.group)).toEqual(["news"]);
    expect(baseline?.requests).toHaveLength(1);
    expect(baseline?.requests[0]).toMatchObject({ seriesId: "spx", status: "ok" });

    wrapper.unmount();
  });

  test("a local job's stamped rows land in their holding step, and no phantom is conjured", async () => {
    const { wrapper, emit } = await mountWithTracker();

    // Step key and groups are the production strings (`holding_step_key`
    // formats `holding-<SYMBOL>`).
    emit({ run_id: "R1", seq: 2, kind: "step-started", step: "holding-PSX", label: "Analyze PSX" });
    emit({ run_id: "R1", seq: 3, kind: "request-started", step: "holding-PSX", group: "company-facts", provider: "SEC", series_id: "0000078214", name: "SEC company facts" });
    emit({ run_id: "R1", seq: 4, kind: "request-started", step: "holding-PSX", group: "company-eod-deep", provider: "FMP", series_id: "PSX", name: "Deep price history" });
    emit({ run_id: "R1", seq: 5, kind: "request-finished", step: "holding-PSX", group: "company-facts", series_id: "0000078214", status: "ok" });
    await flushPromises();

    const steps = wrapper.findComponent(JobTrackerView).props("trace").steps;
    const holding = steps.find((s) => s.key === "holding-PSX");
    expect(holding?.label).toBe("Analyze PSX");
    expect(holding?.requests.map((r) => r.group)).toEqual(["company-facts", "company-eod-deep"]);
    expect(holding?.requests.map((r) => r.provider)).toEqual(["SEC", "FMP"]);
    expect(holding?.requests[0]).toMatchObject({ seriesId: "0000078214", status: "ok" });
    expect(steps.find((s) => s.key === "baseline")).toBeUndefined();

    wrapper.unmount();
  });

  test("a research row's subject rides the fold on both the start and a start-less finish", async () => {
    const { wrapper, emit } = await mountWithTracker();

    // The typed target is progress.rs `RequestTarget`.
    emit({ run_id: "R1", seq: 2, kind: "step-started", step: "holding-PSX", label: "Analyze PSX" });
    emit({ run_id: "R1", seq: 3, kind: "request-started", step: "holding-PSX", group: "research", provider: "web", series_id: "search: PSX refining margins", name: "competitive-position", target: { kind: "search", text: "PSX refining margins" } });
    emit({ run_id: "R1", seq: 4, kind: "request-finished", step: "holding-PSX", group: "research", provider: "web", series_id: "search: PSX refining margins", name: "competitive-position", status: "ok", detail: "12 hits", target: { kind: "search", text: "PSX refining margins" } });
    emit({ run_id: "R1", seq: 5, kind: "request-finished", step: "holding-PSX", group: "research", provider: "web", series_id: "fetch: https://example.com/a", name: "competitive-position", status: "ok", detail: "900 chars extracted", target: { kind: "fetch", text: "https://example.com/a" } });
    await flushPromises();

    const steps = wrapper.findComponent(JobTrackerView).props("trace").steps;
    const holding = steps.find((s) => s.key === "holding-PSX");
    expect(holding?.requests).toHaveLength(2);
    expect(holding?.requests[0]).toMatchObject({
      status: "ok",
      detail: "12 hits",
      target: { kind: "search", text: "PSX refining margins" },
    });
    expect(holding?.requests[1]).toMatchObject({
      status: "ok",
      target: { kind: "fetch", text: "https://example.com/a" },
    });

    wrapper.unmount();
  });

  test("an unstamped row lands in the unattributed list and can never fail the run's record", async () => {
    const { wrapper, emit } = await mountWithTracker();

    // A request emitted with no step open arrives without a stamp; the terminal
    // reconcile leaves it alone.
    emit({ run_id: "R1", seq: 2, kind: "request-started", group: "suite-rate", provider: "FRED", series_id: "DGS10", name: "Rate anchor" });
    emit({ run_id: "R1", seq: 3, kind: "request-finished", group: "suite-rate", series_id: "DGS10", status: "ok" });
    emit({ run_id: "R1", seq: 4, kind: "run-finished", status: "successful" });
    await flushPromises();

    const trace = wrapper.findComponent(JobTrackerView).props("trace");
    expect(trace.unattributed.map((r) => r.group)).toEqual(["suite-rate"]);
    expect(trace.unattributed[0]).toMatchObject({ seriesId: "DGS10", status: "ok" });
    expect(trace.steps.map((s) => s.key)).toEqual(["gate"]);
    expect(trace.steps.every((s) => s.status !== "failed")).toBe(true);

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

  test("a stream restart clears the retried call's panes and nothing else", async () => {
    const { wrapper, emit } = await mountWithTracker();

    emit({ run_id: "R1", seq: 2, kind: "analyst-thinking", posture: "bull", delta: "bull one" });
    emit({ run_id: "R1", seq: 3, kind: "analyst-thinking", posture: "bear", delta: "bear one" });
    // The bull analyst's bounded retry restarts its pane alone.
    emit({ run_id: "R1", seq: 4, kind: "stream-restarted", posture: "bull" });
    emit({ run_id: "R1", seq: 5, kind: "analyst-thinking", posture: "bull", delta: "bull two" });
    emit({ run_id: "R1", seq: 6, kind: "agent-thinking", delta: "weighing" });
    emit({ run_id: "R1", seq: 7, kind: "agent-token", delta: "A first draft" });
    // The main agent's restart (no posture) clears both its report text and reasoning.
    emit({ run_id: "R1", seq: 8, kind: "stream-restarted" });
    emit({ run_id: "R1", seq: 9, kind: "agent-token", delta: "The report" });
    await flushPromises();

    const steps = wrapper.findComponent(JobTrackerView).props("trace").steps;
    const analysts = steps.find((s) => s.key === "analysts");
    expect(analysts?.analystThinking).toEqual({ bull: "bull two", bear: "bear one" });
    const agent = steps.find((s) => s.key === "agent");
    expect(agent?.agentText).toBe("The report");
    expect(agent?.agentThinking).toBe("");

    wrapper.unmount();
  });

  test("step-finished applies the reported status and detail to its step", async () => {
    const { wrapper, emit } = await mountWithTracker();

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

  test("step-thinking folds into the owning step's reasoning, keeping its label", async () => {
    const { wrapper, emit } = await mountWithTracker();

    emit({ run_id: "R1", seq: 2, kind: "step-started", step: "holding-AAPL", label: "Analyze AAPL" });
    emit({ run_id: "R1", seq: 3, kind: "step-thinking", step: "holding-AAPL", delta: "Weighing the trim " });
    emit({ run_id: "R1", seq: 4, kind: "step-thinking", step: "holding-AAPL", delta: "against the tilt" });
    await flushPromises();

    const steps = wrapper.findComponent(JobTrackerView).props("trace").steps;
    const holding = steps.find((s) => s.key === "holding-AAPL");
    expect(holding?.agentThinking).toBe("Weighing the trim against the tilt");
    expect(holding?.label).toBe("Analyze AAPL");
    expect(steps.filter((s) => s.key === "holding-AAPL")).toHaveLength(1);
    expect(steps.some((s) => s.key === "agent" || s.key === "analysts")).toBe(false);

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

  test("a successful run closes a still-running step ok, never failed", async () => {
    const { wrapper, emit } = await mountWithTracker();

    emit({ run_id: "R1", seq: 2, kind: "step-started", step: "persist", label: "Saving the report" });
    emit({ run_id: "R1", seq: 3, kind: "run-finished", status: "successful", report_id: "rep-1" });
    await flushPromises();

    const trace = wrapper.findComponent(JobTrackerView).props("trace");
    expect(trace.terminal).toMatchObject({ status: "successful" });
    expect(trace.steps.find((s) => s.key === "persist")?.status).toBe("ok");

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

    const latest = wrapper.findComponent(LatestReportView);
    expect(latest.exists()).toBe(true);
    expect(latest.props("error")).toContain("gate slammed");

    wrapper.unmount();
  });
});

// Regaining focus refreshes so external changes (files dropped into the inbox
// via Finder) surface on return.
describe("App.vue focus refresh", () => {
  test("regaining focus refreshes config, status, reports, and both folders", async () => {
    const wrapper = mount(App);
    await flushPromises();
    const focus = focusEmitter(tauri.onFocusChanged);

    tauri.invoke.mockClear();
    focus(true);
    await flushPromises();

    // Sorted equality: the exact set, no extras.
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

    expect(invokedCommands()).toHaveLength(0);

    wrapper.unmount();
  });
});

// Interleaving guards only a controllable promise can pin deterministically.
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

    // run-started builds the trace so JobTrackerView's @cancel becomes reachable.
    wrapper.findComponent(JobStatusPanel).vm.$emit("generate");
    await flushPromises();
    emit({ run_id: "R1", seq: 1, kind: "run-started", label: "Weekly run" });
    await flushPromises();

    wrapper.findComponent(JobTrackerView).vm.$emit("cancel");
    await flushPromises();
    pending.reject(new Error("aborted by cancel"));
    await flushPromises();

    expect(cancelRunCalls()).toHaveLength(1);
    // Leaving the tracker reveals the report pane.
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

    slow.resolve(sampleReport);
    await flushPromises();
    expect(wrapper.findComponent(LatestReportView).props("report")).toEqual(sampleReport2);

    wrapper.unmount();
  });
});

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
            readable: true,
          },
          {
            run_id: "prun-old",
            created_at: "2026-06-01T12:00:00Z",
            holdings_count: 1,
            graded_count: 0,
            readable: true,
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

    sidebar.vm.$emit("select-run", "prun-old");
    await flushPromises();
    const portfolio = wrapper.findComponent(PortfolioView);
    expect(portfolio.props("run")).toEqual(oldRun);
    expect(portfolio.props("historical")).toBe(true);
    expect(sidebar.props("selectedRunId")).toBe("prun-old");

    portfolio.vm.$emit("back-to-latest");
    await flushPromises();
    expect(portfolio.props("run")).toEqual(samplePortfolioRun);
    expect(portfolio.props("historical")).toBe(false);

    sidebar.vm.$emit("select-run", samplePortfolioRun.run_id);
    await flushPromises();
    expect(portfolio.props("historical")).toBe(false);
    wrapper.unmount();
  });

  test("an unreadable row classifies as unreadable, not never-ran", async () => {
    tauri.invoke.mockImplementation(
      makeInvokeRouter({
        latest_portfolio_run: () => null,
        list_portfolio_runs: () => [
          {
            run_id: "prun-corrupt",
            created_at: "2026-08-11T12:00:00Z",
            holdings_count: 0,
            graded_count: 0,
            readable: false,
          },
        ],
      })
    );
    const wrapper = mount(App);
    await flushPromises();
    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "portfolio");
    await flushPromises();
    const portfolio = wrapper.findComponent(PortfolioView);
    expect(portfolio.props("unreadableHistory")).toBe(true);
    expect(portfolio.text()).toContain("A prior run couldn't be read.");
    expect(portfolio.text()).not.toContain("No holdings yet.");
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
            readable: true,
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
    // A past-run open failure must never read as a job failure.
    expect(portfolio.props("runError")).toBeNull();
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
            readable: true,
          },
          {
            run_id: "prun-old",
            created_at: "2026-06-01T12:00:00Z",
            holdings_count: 1,
            graded_count: 0,
            readable: true,
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

    expect(wrapper.text()).toContain("Charles Schwab connection");
    expect(wrapper.findComponent(JobStatusPanel).props("blocked")).toBe(false);
    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "portfolio");
    await flushPromises();
    const portfolio = wrapper.findComponent(PortfolioView);
    expect(portfolio.props("runBlocked")).toBe(true);
    expect(portfolio.props("pullBlocked")).toBe(true);
    wrapper.unmount();
  });

  test("a credential both gates name renders one row, not two", async () => {
    // The two gates legitimately overlap (an unset FMP key fails both), and the
    // band's list keys on kind.
    tauri.invoke.mockImplementation(
      makeInvokeRouter({
        check_configuration: () => ({
          categories: [
            {
              kind: "provider-credentials",
              title: "Provider credentials",
              items: ["Missing for FMP, Tavily."],
              dismiss_id: null,
            },
          ],
          is_blocked: true,
        }),
        check_local_configuration: () => ({
          categories: [
            {
              kind: "provider-credentials",
              title: "Provider credentials",
              items: ["Missing for FMP, FRED."],
              dismiss_id: null,
            },
          ],
          is_blocked: true,
        }),
      })
    );
    const wrapper = mount(App);
    await flushPromises();

    const rows = wrapper.findAll(".warning-row");
    const credentialRows = rows.filter((r) =>
      r.text().includes("Provider credentials")
    );
    expect(credentialRows).toHaveLength(1);
    expect(credentialRows[0]!.text()).toContain("Tavily");
    expect(credentialRows[0]!.text()).toContain("FRED");
    wrapper.unmount();
  });

  test("an identical item named by both gates is listed once", async () => {
    const category = {
      kind: "provider-credentials",
      title: "Provider credentials",
      items: ["Missing for FMP."],
      dismiss_id: null,
    };
    tauri.invoke.mockImplementation(
      makeInvokeRouter({
        check_configuration: () => ({ categories: [category], is_blocked: true }),
        check_local_configuration: () => ({
          categories: [category],
          is_blocked: true,
        }),
      })
    );
    const wrapper = mount(App);
    await flushPromises();

    const row = wrapper
      .findAll(".warning-row")
      .find((r) => r.text().includes("Provider credentials"));
    expect(row).toBeDefined();
    expect(row!.text().match(/Missing for FMP\./g) ?? []).toHaveLength(1);
    wrapper.unmount();
  });

  test("a portfolio run lands its result on the page, and its tracker shows no /8 fraction", async () => {
    const pending = deferred<typeof samplePortfolioRun>();
    // Stateful like the real store, so the post-run latest_portfolio_run re-read
    // returns the persisted run instead of null.
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

    // The footer's /8 denominator belongs to the report pipeline, so progress
    // stays null here.
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

  test("a run emit's selection payload reaches the invoke; none sends null", async () => {
    tauri.invoke.mockImplementation(
      makeInvokeRouter({ generate_portfolio_manual: () => samplePortfolioRun })
    );
    const wrapper = mount(App);
    await flushPromises();
    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "portfolio");
    await flushPromises();

    wrapper.findComponent(PortfolioView).vm.$emit("run", ["AAPL", "MSFT"]);
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("generate_portfolio_manual", {
      selected: ["AAPL", "MSFT"],
      resume: null,
    });

    // The Run-analysis triggers always start a new run, so resume is null.
    wrapper.findComponent(PortfolioView).vm.$emit("run");
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("generate_portfolio_manual", {
      selected: null,
      resume: null,
    });
    wrapper.unmount();
  });

  test("a quick check's trace never renders on the report pane left in tracker mode", async () => {
    // A failed report run leaves the report pane in tracker mode; a quick check
    // then replaces the shared trace, and the pane's kind gate must exclude it.
    const pending = deferred<unknown>();
    tauri.invoke.mockImplementation(
      makeInvokeRouter({ run_portfolio_quick_check: () => pending.promise })
    );
    const wrapper = mount(App);
    await flushPromises();

    const emit = emitterFor(tauri.listen, "job-progress");
    emit({ run_id: "R1", seq: 1, kind: "run-started", label: "Weekly run" });
    wrapper.findComponent(JobStatusPanel).vm.$emit("view-tracker");
    await flushPromises();
    expect(wrapper.findComponent(JobTrackerView).exists()).toBe(true);
    emit({ run_id: "R1", seq: 2, kind: "run-finished", status: "failed" });
    await flushPromises();

    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "portfolio");
    await flushPromises();
    wrapper.findComponent(PortfolioView).vm.$emit("quick-check");
    await flushPromises();
    emit({ run_id: "QC1", seq: 1, kind: "run-started", label: "Portfolio quick check" });
    await flushPromises();

    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "report");
    await flushPromises();
    expect(wrapper.findComponent(JobTrackerView).exists()).toBe(false);
    expect(wrapper.findComponent(LatestReportView).exists()).toBe(true);
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

    // Re-read so a mid-session lapse re-locks the triggers without waiting for
    // focus.
    expect(wrapper.findComponent(PortfolioView).props("runError")).toContain(
      "not connected"
    );
    expect(invokedCommands()).toContain("check_local_configuration");
    wrapper.unmount();
  });

  test("a stale portfolio read can't overwrite a fresher run result", async () => {
    // The mount-time read (#1) is held open across a full run.
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
    // The superseded read's finally will not clear portfolioLoading, so the pull
    // path must settle it itself.
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

  test("a degraded SearXNG preflight parks the run behind the notice; confirm launches it once", async () => {
    // The pre-run web-research notice (docs/interface.md §Pre-run web-research
    // notice).
    tauri.invoke.mockImplementation(
      makeInvokeRouter({
        test_searxng: () => ({
          status: "unreachable",
          detail: "SearXNG rejected the JSON request (HTTP 403)",
          degraded: true,
        }),
        generate_portfolio_manual: () => samplePortfolioRun,
      })
    );
    const wrapper = mount(App);
    await flushPromises();
    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "portfolio");
    await flushPromises();

    wrapper.findComponent(PortfolioView).vm.$emit("run", ["AAPL"]);
    await flushPromises();
    expect(invokedCommands()).not.toContain("generate_portfolio_manual");
    // The second ConfirmDialog; the first is the import dialog.
    const notice = wrapper.findAllComponents(ConfirmDialog)[1];
    expect(notice.props("open")).toBe(true);
    expect(notice.props("body")).toContain("SearXNG-only");
    expect(notice.props("detail")).toContain("HTTP 403");

    notice.vm.$emit("confirm");
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("generate_portfolio_manual", {
      selected: ["AAPL"],
      resume: null,
    });
    wrapper.unmount();
  });

  test("a cancelled notice never launches, and the SearXNG-only copy flags not-recommended", async () => {
    tauri.invoke.mockImplementation(
      makeInvokeRouter({
        test_searxng: () => ({
          status: "not_configured",
          detail: null,
          degraded: true,
        }),
      })
    );
    const wrapper = mount(App);
    await flushPromises();
    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "portfolio");
    await flushPromises();

    wrapper.findComponent(PortfolioView).vm.$emit("run");
    await flushPromises();
    const notice = wrapper.findAllComponents(ConfirmDialog)[1];
    expect(notice.props("open")).toBe(true);
    expect(notice.props("body")).toContain("not recommended");

    notice.vm.$emit("cancel");
    await flushPromises();
    expect(invokedCommands()).not.toContain("generate_portfolio_manual");
    expect(wrapper.findAllComponents(ConfirmDialog)[1].props("open")).toBe(false);
    wrapper.unmount();
  });

  test("the engine-only quick check never probes SearXNG or opens the notice", async () => {
    tauri.invoke.mockImplementation(
      makeInvokeRouter({ run_portfolio_quick_check: () => null })
    );
    const wrapper = mount(App);
    await flushPromises();
    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "portfolio");
    await flushPromises();

    tauri.invoke.mockClear();
    wrapper.findComponent(PortfolioView).vm.$emit("quick-check");
    await flushPromises();
    expect(invokedCommands()).toContain("run_portfolio_quick_check");
    expect(invokedCommands()).not.toContain("test_searxng");
    wrapper.unmount();
  });

  test("the launch preflight updates the Settings indicator; an endpoint change clears it", async () => {
    const preflight = {
      status: "unreachable",
      detail: "SearXNG rejected the JSON request (HTTP 403)",
      degraded: true,
    };
    // Stateful, so the reload can tell a same-endpoint save from a changed one.
    let endpoint = "";
    tauri.invoke.mockImplementation(
      makeInvokeRouter({
        test_searxng: () => preflight,
        save_web_research_settings: (args) => {
          endpoint = (args?.values as { searxng_endpoint: string })
            .searxng_endpoint;
          return null;
        },
        get_settings: () => ({
          ...defaultSettings,
          web_research: { searxng_endpoint: endpoint },
        }),
      })
    );
    const wrapper = mount(App);
    await flushPromises();
    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "portfolio");
    await flushPromises();
    wrapper.findComponent(PortfolioView).vm.$emit("run");
    await flushPromises();

    // The launch probe's result reaches the Settings indicator
    // (docs/interface.md §Connection status), and a same-endpoint reload must
    // not discard it.
    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "settings");
    await flushPromises();
    expect(wrapper.findComponent(Settings).props("searxngStatus")).toEqual(
      preflight
    );

    wrapper
      .findComponent(Settings)
      .vm.$emit("save-web-research", { searxng_endpoint: "http://127.0.0.1:9999" });
    await flushPromises();
    expect(wrapper.findComponent(Settings).props("searxngStatus")).toBeNull();
    wrapper.unmount();
  });

  test("a settings reload during the launch probe discards the raced result and re-probes", async () => {
    // A settings reload bumps the epoch, superseding the config the in-flight
    // probe described.
    const stale = {
      status: "ok",
      detail: null,
      degraded: false,
    };
    const fresh = {
      status: "unreachable",
      detail: "new endpoint down",
      degraded: true,
    };
    const resolvers: Array<(v: unknown) => void> = [];
    tauri.invoke.mockImplementation(
      makeInvokeRouter({
        test_searxng: () => new Promise((resolve) => resolvers.push(resolve)),
        generate_portfolio_manual: () => samplePortfolioRun,
      })
    );
    const wrapper = mount(App);
    await flushPromises();
    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "portfolio");
    await flushPromises();

    wrapper.findComponent(PortfolioView).vm.$emit("run");
    await flushPromises();
    expect(resolvers.length).toBe(1);

    // Opening Settings reloads them, bumping the epoch while the probe is pending.
    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "settings");
    await flushPromises();

    resolvers[0](stale);
    await flushPromises();
    expect(wrapper.findComponent(Settings).props("searxngStatus")).toBeNull();
    expect(resolvers.length).toBe(2);
    expect(invokedCommands()).not.toContain("generate_portfolio_manual");

    resolvers[1](fresh);
    await flushPromises();
    expect(wrapper.findComponent(Settings).props("searxngStatus")).toEqual(fresh);
    const notice = wrapper.findAllComponents(ConfirmDialog)[1];
    expect(notice.props("open")).toBe(true);
    expect(invokedCommands()).not.toContain("generate_portfolio_manual");
    wrapper.unmount();
  });

  test("a probe resolving while an endpoint save is mid-flight never grounds the launch", async () => {
    // While a save's invoke is in flight the backend is between endpoints, so
    // any probe resolving in that window could describe either config; the
    // launch refuses acceptance while webResearchSaving holds.
    const ambiguous = {
      status: "ok",
      detail: null,
      degraded: false,
    };
    const probeResolvers: Array<(v: unknown) => void> = [];
    const saveResolvers: Array<(v: unknown) => void> = [];
    tauri.invoke.mockImplementation(
      makeInvokeRouter({
        test_searxng: () => new Promise((resolve) => probeResolvers.push(resolve)),
        save_web_research_settings: () =>
          new Promise((resolve) => saveResolvers.push(resolve)),
        generate_portfolio_manual: () => samplePortfolioRun,
      })
    );
    const wrapper = mount(App);
    await flushPromises();
    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "portfolio");
    await flushPromises();
    wrapper.findComponent(PortfolioView).vm.$emit("run");
    await flushPromises();
    expect(probeResolvers.length).toBe(1);

    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "settings");
    await flushPromises();
    wrapper
      .findComponent(Settings)
      .vm.$emit("save-web-research", { searxng_endpoint: "http://127.0.0.1:9999" });
    await flushPromises();

    // Bounded retries: every probe resolving during the save is discarded, then
    // the degraded-unknown notice.
    for (let i = 0; i < 3; i++) {
      expect(probeResolvers.length).toBe(i + 1);
      probeResolvers[i](ambiguous);
      await flushPromises();
    }
    expect(wrapper.findComponent(Settings).props("searxngStatus")).toBeNull();
    expect(invokedCommands()).not.toContain("generate_portfolio_manual");
    const notice = wrapper.findAllComponents(ConfirmDialog)[1];
    expect(notice.props("open")).toBe(true);
    expect(notice.props("detail")).toContain("settings changed while probing");

    // The confirm path holds the same mid-save block as the probe.
    notice.vm.$emit("confirm");
    await flushPromises();
    expect(invokedCommands()).not.toContain("generate_portfolio_manual");

    saveResolvers[0](null);
    await flushPromises();
    wrapper.unmount();
  });
});

// (docs/data-portability.md §Import flow)
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

    expect(invokedCommands()).not.toContain("import_data");
    const dialog = wrapper.findComponent(ConfirmDialog);
    expect(dialog.props("open")).toBe(true);
    expect(dialog.props("detail")).toContain("2026-07-05");
    expect(dialog.props("detail")).toContain("30 reports");
    expect(dialog.props("detail")).toContain("214 learnings");

    dialog.vm.$emit("confirm");
    await flushPromises();

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
