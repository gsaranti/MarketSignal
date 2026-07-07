<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getVersion } from "@tauri-apps/api/app";
import RecentReportsSidebar from "./components/RecentReportsSidebar.vue";
import LatestReportView from "./components/LatestReportView.vue";
import JobTrackerView from "./components/JobTrackerView.vue";
import PortfolioView from "./components/PortfolioView.vue";
import ResearchDocuments from "./components/ResearchDocuments.vue";
import Settings from "./components/Settings.vue";
import PersistentWarningArea from "./components/PersistentWarningArea.vue";
import JobStatusPanel from "./components/JobStatusPanel.vue";
import type {
  AppView,
  AgentModels,
  ConnectionTestResult,
  CredentialKey,
  CredentialUpdate,
  GeneratedReport,
  HoldingsPull,
  JobStatus,
  PortfolioRun,
  ProgressMessage,
  ReportSummary,
  ResearchDocument,
  RunTrace,
  SchwabCredentialUpdate,
  SchwabStatus,
  SettingsView,
  StepStatus,
  TrackerStep,
  TruncationStats,
  ValidationReport,
} from "./types";
import { readDark, writeDark } from "./theme";

// Which main surface is showing. A plain ref switch (no router) — the app has a
// small fixed set of destinations and the kit models this as top-level state,
// not routes (see AppView in types.ts).
const view = ref<AppView>("report");

// App version for the titlebar masthead ("Desk · v1.0.0"); falls back to "Desk"
// if the version can't be read.
const appVersion = ref("");
const versionLabel = computed(() =>
  appVersion.value ? `Desk · v${appVersion.value}` : "Desk"
);

// The recent-reports list (newest first, capped at 30 by the backend) and the
// currently-selected report. `reports` drives the sidebar; `selectedReport`
// carries the loaded Markdown for the report pane. Selection is held by id so the
// sidebar highlight and the pane stay in sync from one source of truth.
const reports = ref<ReportSummary[]>([]);
const selectedReportId = ref<string | null>(null);
const selectedReport = ref<GeneratedReport | null>(null);
// Two distinct error channels, deliberately kept apart: `reportError` is a
// failure to OPEN the selected report (feeds the report pane's load-error
// state); `reportsError` is a failure to LIST the sidebar (a sidebar-level
// problem). Conflating them lets a transient list refresh mask a perfectly-valid
// loaded report — so the list error never reaches the report pane.
const reportError = ref<string | null>(null);
const reportsError = ref<string | null>(null);
const generating = ref(false);
const error = ref<string | null>(null);

// --- Live job tracker ------------------------------------------------------
// While a run is in flight the report pane shows the tracker instead of a report.
// `reportPaneMode` is the toggle; `runTrace` is the assembled event stream for the
// latest run (in-session, latest-run-only); `runActive` is true between
// run-started and run-finished; `cancelRequested` disables Cancel after a click.
const reportPaneMode = ref<"report" | "tracker">("report");
const runTrace = ref<RunTrace | null>(null);
const runActive = ref(false);
const cancelRequested = ref(false);
// Which kind of run the current trace belongs to (set by whichever starter
// kicked it off). The tracker is one shared component placed on the owning
// page — a portfolio run's log shows in the Portfolio view, a report run's in
// the report pane (docs/run-tracking.md) — and the footer's determinate /8
// fill applies only to the report's fixed pipeline.
const runTraceKind = ref<"report" | "portfolio" | null>(null);
// The Portfolio view's pane toggle, mirroring reportPaneMode: while a portfolio
// run is in flight (or its log is reopened) the page shows the tracker.
const portfolioPaneMode = ref<"results" | "tracker">("results");
// Wall-clock start of the in-flight run (epoch ms), stamped at `run-started`. Drives
// the footer's live elapsed timer; null when no run has started this session.
const runStartedAt = ref<number | null>(null);

// The synthetic first step: the run only ever starts once the execution gate has
// passed (`check_configuration`), so we show that as a completed step rather than
// emitting a backend event for an instant, already-done check.
const GATE_STEP_LABEL = "Credentials & configuration";

// The canonical coarse pipeline steps, in the order the backend emits them
// (`pipeline.rs` step_started calls) plus the synthesized `gate`. Used only as the
// denominator for the footer's determinate progress fill — the fine-grained per-
// request rows live in the tracker, not here.
const PIPELINE_STEP_KEYS = [
  "gate",
  "baseline",
  "coverage",
  "inbox",
  "research",
  "analysts",
  "agent",
  "persist",
] as const;

// Determinate run progress for the footer status row (the design kit's long-job
// component: a 1px fill whose width tracks real step completion, plus a "step N of T"
// caption). The fill credits completed steps fully and the in-flight step a half
// step, so it advances as each step begins yet never reads 100% until the run
// actually finishes. Honest — never a faked sweep. Null unless a run is in flight.
const runProgress = computed<{
  fraction: number;
  stepNumber: number;
  total: number;
  label: string;
} | null>(() => {
  const trace = runTrace.value;
  if (!trace || !runActive.value) return null;
  // The fixed 8-step denominator is the report pipeline's; a portfolio run's
  // steps are dynamic (one per holding), so it gets the running label + elapsed
  // timer without a bogus fraction.
  if (runTraceKind.value !== "report") return null;
  const total = PIPELINE_STEP_KEYS.length;
  let completed = 0; // count of canonical steps in a terminal state (steps finish in order)
  let running: { idx: number; label: string } | null = null;
  let lastTouched: { idx: number; label: string } | null = null;
  for (let i = 0; i < PIPELINE_STEP_KEYS.length; i++) {
    const step = trace.steps.find((s) => s.key === PIPELINE_STEP_KEYS[i]);
    if (!step || step.status === "pending") continue;
    lastTouched = { idx: i + 1, label: step.label };
    if (step.status === "running") running = { idx: i + 1, label: step.label };
    else completed = i + 1;
  }
  const current = running ?? lastTouched;
  if (!current) return null;
  // The in-flight step counts as a half step — credit for being underway, not for
  // finishing — so a running final "Saving the report" tops out below 100%; the bar
  // fills completely only when the run actually completes.
  const fraction = (completed + (running ? 0.5 : 0)) / total;
  return {
    fraction,
    stepNumber: current.idx,
    total,
    label: current.label,
  };
});

// Find or create a step by key. step-started always precedes its requests/tokens
// and step-finished, so the lookup normally hits; the create is a safety net.
function ensureStep(trace: RunTrace, key: string, label: string): TrackerStep {
  let step = trace.steps.find((s) => s.key === key);
  if (!step) {
    step = {
      key,
      label,
      status: "running",
      detail: null,
      requests: [],
      agentText: "",
      agentThinking: "",
      analystThinking: {},
    };
    trace.steps.push(step);
  }
  return step;
}

// Route a request row to the step that issued it, keyed off the event's `group`. The
// research half's request groups (Tavily news, the headline filter, the router, and the
// executor's searches) belong under the "research" step; every other group is a baseline
// scan series. Vector-memory embedding calls (group "memory") fire inside more than one
// step — the research step's Step-4/10 retrieval pulls and the persist step's summary
// write — so they follow the step that is running when they arrive (step-started always
// precedes a stage's request rows), with "persist" as the safety-net fallback. Without
// this routing, every request row would pile under "baseline" and the research step
// would render empty. The label is a fallback only — the owning step normally already
// exists with its backend label.
const RESEARCH_REQUEST_GROUPS = new Set(["news", "filter", "routing", "research"]);
function requestStep(trace: RunTrace, group: string): TrackerStep {
  if (RESEARCH_REQUEST_GROUPS.has(group)) return ensureStep(trace, "research", "Research");
  // The three analysts' per-call rows belong with their reasoning panes under the
  // "analysts" step, not the baseline fallback below.
  if (group === "analyst") return ensureStep(trace, "analysts", "Running the analyst agents");
  if (group === "memory") {
    const running = [...trace.steps].reverse().find((s) => s.status === "running");
    return running ?? ensureStep(trace, "persist", "Saving the report");
  }
  return ensureStep(trace, "baseline", "Baseline market data");
}

// Fold one streamed progress message into the trace. Events are filtered to the
// current run by `run_id`, so a straggler from a prior run can't corrupt it.
function handleProgress(msg: ProgressMessage) {
  if (msg.kind === "run-started") {
    runTrace.value = {
      runId: msg.run_id,
      label: msg.label ?? "Report run",
      steps: [
        {
          key: "gate",
          label: GATE_STEP_LABEL,
          status: "ok",
          detail: null,
          requests: [],
          agentText: "",
          agentThinking: "",
          analystThinking: {},
        },
      ],
      terminal: null,
    };
    runActive.value = true;
    runStartedAt.value = Date.now();
    cancelRequested.value = false;
    return;
  }

  const trace = runTrace.value;
  if (!trace || msg.run_id !== trace.runId) return;

  switch (msg.kind) {
    case "step-started":
      ensureStep(trace, msg.step ?? "", msg.label ?? msg.step ?? "").status = "running";
      break;
    case "step-finished": {
      const step = ensureStep(trace, msg.step ?? "", msg.step ?? "");
      step.status = (msg.status as StepStatus) ?? "ok";
      step.detail = msg.detail ?? null;
      break;
    }
    case "request-started": {
      // One row per actual HTTP request, shown in-flight ("running") until it
      // resolves. Skipped (no-request) series never emit this, so rows stay
      // one-to-one with network calls. Routed to its owning step by `group` so
      // research-half rows land under "research", not "baseline".
      const step = requestStep(trace, msg.group ?? "");
      step.requests.push({
        provider: msg.provider ?? "",
        group: msg.group ?? "",
        seriesId: msg.series_id ?? "",
        name: msg.name ?? msg.series_id ?? "",
        status: "running",
        detail: null,
      });
      break;
    }
    case "request-finished": {
      const step = requestStep(trace, msg.group ?? "");
      // Resolve the matching in-flight row. Requests are sequential, so the running
      // row for this group+series is the one to update; fall back to appending a
      // resolved row if a started was somehow missed.
      const row = step.requests.find(
        (r) =>
          r.status === "running" &&
          r.group === (msg.group ?? "") &&
          r.seriesId === (msg.series_id ?? "")
      );
      if (row) {
        row.status = msg.status ?? "ok";
        row.detail = msg.detail ?? null;
      } else {
        step.requests.push({
          provider: msg.provider ?? "",
          group: msg.group ?? "",
          seriesId: msg.series_id ?? "",
          name: msg.name ?? msg.series_id ?? "",
          status: msg.status ?? "ok",
          detail: msg.detail ?? null,
        });
      }
      break;
    }
    case "agent-token":
      ensureStep(trace, "agent", "Main agent").agentText += msg.delta ?? "";
      break;
    case "agent-thinking":
      ensureStep(trace, "agent", "Main agent").agentThinking += msg.delta ?? "";
      break;
    case "analyst-thinking": {
      // Route each analyst's reasoning to its own pane under the "analysts" step, keyed
      // by posture so the three concurrent streams stay separate.
      const step = ensureStep(trace, "analysts", "Running the analyst agents");
      const posture = msg.posture ?? "";
      step.analystThinking[posture] = (step.analystThinking[posture] ?? "") + (msg.delta ?? "");
      break;
    }
    case "run-finished": {
      trace.terminal = { status: msg.status ?? "", detail: msg.detail ?? null };
      runActive.value = false;
      // Reconcile any step still "running" at the end (a cancel mid-step) so it
      // doesn't read as in-progress forever.
      const fallback: StepStatus = msg.status === "cancelled" ? "cancelled" : "failed";
      for (const step of trace.steps) {
        if (step.status === "running") step.status = fallback;
      }
      break;
    }
  }
}

// Request cancellation of the in-flight run. The backend stops at its next
// checkpoint and emits run-finished{cancelled}; we mark cancelRequested so the
// Cancel button reads "Cancelling…" until then.
async function cancelRun() {
  if (!runActive.value) return;
  cancelRequested.value = true;
  try {
    await invoke("cancel_run");
  } catch (e) {
    // A failed cancel invoke is rare; surface it on the run-status channel and let
    // the user retry. Re-enable the button.
    cancelRequested.value = false;
    jobStatusError.value = String(e);
  }
}

// Return to the report from the run log, leaving the log behind. It stays
// reopenable from the footer ("Latest run log") for the rest of the session; it is
// only replaced when the next run begins (latest-run-only) or cleared when the app
// quits — never discarded by this action.
function closeRunLog() {
  reportPaneMode.value = "report";
}

// Open the tracker for the current/last run (the footer's "View progress" /
// "Latest run log" handle). Routes to the run's owning page — the tracker
// replaces the page whose job is running (docs/run-tracking.md).
function viewTracker() {
  if (runTraceKind.value === "portfolio") {
    view.value = "portfolio";
    portfolioPaneMode.value = "tracker";
  } else {
    view.value = "report";
    reportPaneMode.value = "tracker";
  }
}

// Markdown export state, kept on its own channel (like the others above):
// `exportingMarkdown` drives the toolbar button's busy state; `exportError`
// surfaces a failed save as a slim alert under the report toolbar. PDF export is
// handled inside LatestReportView (window.print) and needs no state here.
const exportingMarkdown = ref(false);
const exportError = ref<string | null>(null);

// Whether the selected report is the newest one — drives the toolbar's "Latest"
// tag. The list is newest-first, so the head is the latest.
const selectedIsLatest = computed(
  () =>
    selectedReportId.value !== null &&
    reports.value.length > 0 &&
    reports.value[0].report_id === selectedReportId.value
);

// Research inbox state lives here (not in the inbox view) so the sidebar badge
// can show the count regardless of which view is active, and a single load path
// keeps badge and list in sync.
const documents = ref<ResearchDocument[]>([]);
const documentsLoading = ref(false);
const documentsError = ref<string | null>(null);
const inboxCount = computed(() => documents.value.length);

// Research archive state — the read-only twin of the inbox: the pipeline files
// processed documents here. Loaded up front (like the inbox) so the sidebar badge
// is populated regardless of which view is active.
const archiveDocuments = ref<ResearchDocument[]>([]);
const archiveLoading = ref(false);
const archiveError = ref<string | null>(null);
const archiveCount = computed(() => archiveDocuments.value.length);

const validation = ref<ValidationReport | null>(null);
const validationError = ref<string | null>(null);

// The local-suite presence gate (check_local_configuration — docs/interface.md
// §Persistent Warning Area), kept apart from the cloud `validation`: its
// categories join the warning band, but its is_blocked gates only the local
// jobs' triggers, never the report's Generate.
const localValidation = ref<ValidationReport | null>(null);

const jobStatus = ref<JobStatus | null>(null);
const jobStatusError = ref<string | null>(null);

// --- Portfolio page state ----------------------------------------------------
// The latest persisted analysis run + the latest standalone holdings pull
// (docs/portfolio-analysis.md §Storage and display, §Triggering), read on
// startup and on page entry; PortfolioView is presentational.
const portfolioRun = ref<PortfolioRun | null>(null);
const holdingsPull = ref<HoldingsPull | null>(null);
const portfolioLoading = ref(false);
const portfolioLoadError = ref<string | null>(null);
// A run-gate block or run/pull failure — the page's inline (ephemeral) error,
// never a persistent warning.
const portfolioError = ref<string | null>(null);
const portfolioRunning = ref(false);
const pullingHoldings = ref(false);

// Fail-safe like the cloud `blocked`: until the local check resolves, treat the
// local jobs as blocked so their triggers are never briefly clickable.
const localBlocked = computed(() => localValidation.value?.is_blocked ?? true);
const localCategories = computed(() => localValidation.value?.categories ?? []);
const schwabCategory = computed(
  () => localCategories.value.find((c) => c.kind === "schwab") ?? null
);
// The view-only pull needs only the Schwab connection (no model call), so it
// gates on that category alone — usable before local models are configured.
const pullBlocked = computed(() =>
  localValidation.value === null ? true : schwabCategory.value !== null
);
const localBlockedReason = computed(() => {
  const items = localCategories.value.flatMap((c) => c.items);
  return items.length > 0 ? items.join(" ") : null;
});
const pullBlockedReason = computed(
  () => schwabCategory.value?.items.join(" ") ?? null
);

// The warning band shows both gates' categories in one de-duplicated block; the
// cloud is_blocked keeps its report-gate meaning (the local one never blocks
// the report — docs/interface.md §Persistent Warning Area).
const displayedValidation = computed<ValidationReport | null>(() => {
  if (validation.value === null && localValidation.value === null) return null;
  return {
    categories: [
      ...(validation.value?.categories ?? []),
      ...(localValidation.value?.categories ?? []),
    ],
    is_blocked: validation.value?.is_blocked ?? true,
  };
});

async function refreshLocalValidation() {
  try {
    localValidation.value = await invoke<ValidationReport>(
      "check_local_configuration"
    );
  } catch {
    // Fail-safe: an unreadable local gate reads as blocked (localBlocked's
    // null fallback) rather than silently unlocked; the cloud validationError
    // channel already surfaces config-check faults.
    localValidation.value = null;
  }
}

// Invalidates in-flight portfolio reads: bumped when a refresh starts and when
// fresher state lands directly (a run's inline result, a completed pull), so an
// older latest_portfolio_run / latest_holdings_pull response resolving late can
// never overwrite newer state — the selectReport / settingsEpoch pattern.
let portfolioEpoch = 0;

async function refreshPortfolio() {
  const epoch = ++portfolioEpoch;
  portfolioLoading.value = true;
  portfolioLoadError.value = null;
  try {
    const [run, pull] = await Promise.all([
      invoke<PortfolioRun | null>("latest_portfolio_run"),
      invoke<HoldingsPull | null>("latest_holdings_pull"),
    ]);
    if (epoch !== portfolioEpoch) return;
    portfolioRun.value = run;
    holdingsPull.value = pull;
  } catch (e) {
    if (epoch !== portfolioEpoch) return;
    portfolioLoadError.value = String(e);
  } finally {
    // A superseded read leaves the flags to whichever refresh is current.
    if (epoch === portfolioEpoch) portfolioLoading.value = false;
  }
}

// Settings state lives here alongside the other surfaces' state; the Settings
// view is presentational. One `settingsError` carries both load and save errors.
const settings = ref<SettingsView | null>(null);
const settingsLoading = ref(false);
const settingsSaving = ref(false);
const settingsError = ref<string | null>(null);

// Aggregate truncation telemetry for the Settings diagnostics section, loaded
// alongside settings. `null` = not yet loaded / unavailable (the section shows
// nothing); a populated all-zero aggregate is the "no truncations recorded"
// empty state. Kept off `settingsError` so a diagnostics hiccup never blanks the
// settings form.
const truncationStats = ref<TruncationStats | null>(null);

// Per-credential "Test connection" state, kept on its own channels (apart from
// settingsError, which is load/save only): which credential is being tested, and
// the last result for each. Ephemeral and Settings-local — reset on every fresh
// settings load so a stale chip never outlives the saved value it described.
const emptyConnectionState = <T,>(value: T): Record<CredentialKey, T> => ({
  openai: value,
  anthropic: value,
  fmp: value,
  fred: value,
  tavily: value,
});
const connectionTesting = ref<Record<CredentialKey, boolean>>(
  emptyConnectionState(false)
);
const connectionTests = ref<Record<CredentialKey, ConnectionTestResult | null>>(
  emptyConnectionState<ConnectionTestResult | null>(null)
);
// Bumped on every settings (re)load to invalidate in-flight tests: a reload can
// change which key is saved, so a test still resolving against the old saved
// value must be discarded rather than land as a result beside the new one. Each
// testConnection captures the epoch at start and only writes if it still matches.
const settingsEpoch = ref(0);

// Charles Schwab connection (docs/schwab-integration.md). Its own state channels,
// parallel to the credential machinery: `schwabStatus` is the connection view read
// from `schwab_status` (null until loaded / on failure), `schwabConnecting` is true
// while the interactive browser login is in flight, and `schwabError` carries a
// save/connect failure (kept off settingsError). Loaded alongside settings.
const schwabStatus = ref<SchwabStatus | null>(null);
const schwabConnecting = ref(false);
const schwabError = ref<string | null>(null);

// A workflow holds (or is about to claim) the single global run slot — report,
// portfolio run, holdings pull, or Schwab connect. Every other trigger disables
// while it does. The session flags cover the click-to-first-poll window; the
// backend's is_running is the authoritative read that survives a reload.
const slotBusy = computed(
  () =>
    generating.value ||
    portfolioRunning.value ||
    pullingHoldings.value ||
    schwabConnecting.value ||
    (jobStatus.value?.is_running ?? false)
);

// Connecting takes the single global run slot (schwab_connect holds the RunGuard),
// so Connect is disabled while any report/job run is in flight; the button reads this.
const schwabBusy = computed(
  () => generating.value || portfolioRunning.value || pullingHoldings.value ||
    (jobStatus.value?.is_running ?? false)
);

// What the footer calls the in-flight work. The run slot is shared (report /
// Portfolio / holdings pull / Schwab connect), so the label follows the workflow
// actually holding it: the session flags cover the click-to-first-poll window
// (job_status is refreshed on focus and at run edges, not on an interval), and
// the backend's `running_kind` is the authoritative read that survives a
// webview reload.
const footerRunningLabel = computed(() => {
  const kind = jobStatus.value?.running_kind ?? null;
  if (schwabConnecting.value || kind === "schwab-connect")
    return "Connecting to Charles Schwab…";
  if (pullingHoldings.value || kind === "holdings-pull")
    return "Pulling holdings…";
  if (portfolioRunning.value || kind === "portfolio")
    return "Running Portfolio Analysis…";
  return "Generating report…";
});

// The gate blocks generation when configuration is incomplete. The backend is
// the authoritative guard; this only disables the control and short-circuits.
// Fail safe: until the first check resolves (or if it errors), treat as blocked
// so Generate is never briefly clickable for an unverified config.
const blocked = computed(() => validation.value?.is_blocked ?? true);

async function refreshValidation() {
  validationError.value = null;
  try {
    validation.value = await invoke<ValidationReport>("check_configuration");
  } catch (e) {
    validationError.value = String(e);
  }
}

// Dismiss one Persistent Warning Area warning (failed job) by the identity
// the row rendered, then re-derive the band. Passing the shown `dismissId` (not just
// the kind) is what keeps a stale click from suppressing a newer warning the backend
// would otherwise re-derive. Best-effort: if the dismiss call fails the warning
// simply stays shown — the refresh keeps the band truthful, so no separate error
// channel is needed.
async function dismissWarning(kind: string, dismissId: string) {
  try {
    await invoke("dismiss_warning", { kind, id: dismissId });
  } catch {
    // ignore — refreshValidation below re-renders whatever is still active
  }
  await refreshValidation();
}

async function refreshJobStatus() {
  jobStatusError.value = null;
  try {
    jobStatus.value = await invoke<JobStatus>("job_status");
  } catch (e) {
    jobStatusError.value = String(e);
  }
}

// Appearance (Light/Dark) lives alongside the other surfaces' state. Unlike the
// gated config form, the toggle applies + persists instantly — see ./theme. The
// initial value was already applied to <html> in main.ts before mount; this ref
// just keeps the Settings switch in sync.
const dark = ref(readDark());
function setDark(value: boolean) {
  writeDark(value);
  dark.value = value;
}

async function generate() {
  if (blocked.value) return;
  generating.value = true;
  error.value = null;
  // Show the live tracker for the run the user just kicked off; run-started will
  // populate it (and reset cancelRequested, but set it here too for the gap before
  // the first event lands).
  runTraceKind.value = "report";
  reportPaneMode.value = "tracker";
  cancelRequested.value = false;
  // Drop any prior run's trace up front. The new run replaces it via run-started; but
  // if this attempt fails *before* run-started (a gate/key/adapter error, or a
  // concurrency skip), runTrace stays null so the catch surfaces the error rather than
  // leaving the previous run's (possibly successful) log on screen.
  runTrace.value = null;
  try {
    // A fresh run returns the full report (with Markdown). Surface its sidebar row
    // either way; refreshReports() reconciles ordering against the DB.
    const fresh = await invoke<GeneratedReport>("generate_report_manual");
    upsertReportSummary(fresh.summary);
    void refreshReports();
    // Show the new report only if the user is still watching the run. If they
    // navigated to an older report mid-run (reportPaneMode === "report"), leave
    // them there — the new row appears in the sidebar and the run log lingers.
    if (reportPaneMode.value === "tracker") {
      selectedReport.value = fresh;
      selectedReportId.value = fresh.report_id;
      reportError.value = null;
      reportPaneMode.value = "report";
    }
  } catch (e) {
    // A user-initiated cancel is intentional, not an error surface — the tracker
    // shows the cancelled terminal state and the footer offers the run log.
    if (cancelRequested.value) {
      // nothing to surface
    } else if (!runTrace.value) {
      // The run never produced a tracker (e.g. a concurrency skip) — surface the
      // reason on the report pane so the failure isn't silent.
      error.value = String(e);
      reportPaneMode.value = "report";
    }
    // Otherwise the tracker's failed terminal state + the warning area carry it.
  } finally {
    generating.value = false;
    // Re-check after a run: config may have changed, and a run updates job
    // history (failed-job warning, last-run status). Refresh reports too, so a
    // report that persisted but whose command then errored (e.g. a job-history write
    // failure after the report itself was written) still appears in the sidebar.
    void refreshValidation();
    void refreshJobStatus();
    void refreshReports();
    // The run may have consumed inbox documents (archived after persist) or
    // recorded parse failures — re-read both folders so the panel, error states,
    // and sidebar badge match disk without waiting for a focus change.
    void refreshDocuments();
    void refreshArchive();
  }
}

// Mirrors the backend's display cap (storage::RECENT_REPORTS_LIMIT) and the
// sidebar header's "last 30". Held here so the optimistic insert below honors the
// cap even when the reconciling refresh — which would otherwise re-impose it —
// fails.
const RECENT_REPORTS_LIMIT = 30;

// Place a report's summary at the head of the list (deduped, capped), so a
// freshly generated or just-finished report appears — selected — in the sidebar
// immediately, without waiting on (or depending on) the list refresh. The report
// is already persisted by the time we have its summary, so the optimistic row is
// always real; refreshReports() then reconciles the authoritative DB ordering.
// The trim keeps the list within "last 30" if a post-generate refresh fails
// (otherwise repeated failures could grow it past the cap).
function upsertReportSummary(summary: ReportSummary) {
  reports.value = [
    summary,
    ...reports.value.filter((r) => r.report_id !== summary.report_id),
  ].slice(0, RECENT_REPORTS_LIMIT);
}

async function refreshReports() {
  try {
    reports.value = await invoke<ReportSummary[]>("list_reports");
    // A recovered refresh clears a prior list error — the sidebar error state is
    // never left stuck once listing succeeds again. On failure the old list is
    // kept (the early return below leaves `reports` untouched).
    reportsError.value = null;
  } catch (e) {
    reportsError.value = String(e);
    return;
  }
  // On first load — or after the selected report fell out of the list — default
  // to the newest report so the pane is never blank when reports exist. But only to
  // fill a blank *report* pane: never when the tracker is showing or a generation
  // error is surfaced, since selectReport() would switch the pane and clear that
  // error (a post-failure list refresh must not erase the failure it follows).
  const stillSelected =
    selectedReportId.value !== null &&
    reports.value.some((r) => r.report_id === selectedReportId.value);
  if (
    !stillSelected &&
    reports.value.length > 0 &&
    reportPaneMode.value === "report" &&
    error.value === null
  ) {
    void selectReport(reports.value[0].report_id);
  }
}

async function selectReport(id: string) {
  selectedReportId.value = id;
  reportError.value = null;
  // Viewing a specific report leaves the run tracker (it keeps running in the
  // background; the footer's "View progress" returns to it).
  reportPaneMode.value = "report";
  // Viewing a specific report dismisses any prior generation-failure banner —
  // otherwise LatestReportView's `error` block (which has render precedence)
  // would mask the report we just loaded.
  error.value = null;
  // A prior export failure belonged to whatever issue was open then; clear it so
  // it doesn't linger over a freshly selected report.
  exportError.value = null;
  try {
    const loaded = await invoke<GeneratedReport>("load_report", {
      reportId: id,
    });
    // Guard against a slower earlier load resolving after a newer selection:
    // only apply the result if this is still the selected report.
    if (selectedReportId.value !== id) return;
    selectedReport.value = loaded;
  } catch (e) {
    if (selectedReportId.value !== id) return;
    // A report whose Markdown was removed out-of-band still lists but can't be
    // opened — surface the failure and clear the pane rather than show a stale body.
    selectedReport.value = null;
    reportError.value = String(e);
  }
}

// Sidebar row click: show the report surface and load the chosen issue.
function selectAndShow(id: string) {
  view.value = "report";
  void selectReport(id);
}

// Export the selected report as Markdown. The backend command opens a native
// Save dialog and writes the stored canonical Markdown to the chosen path
// (docs/export.md); it returns true when saved, false when the user cancels —
// both are non-errors. A real failure (unknown id, unreadable/unwritable file)
// surfaces on the dedicated exportError channel under the report toolbar.
async function exportMarkdown() {
  if (selectedReportId.value === null) return;
  exportingMarkdown.value = true;
  exportError.value = null;
  try {
    await invoke<boolean>("export_report_markdown", {
      reportId: selectedReportId.value,
    });
  } catch (e) {
    exportError.value = String(e);
  } finally {
    exportingMarkdown.value = false;
  }
}

async function refreshDocuments() {
  documentsLoading.value = true;
  documentsError.value = null;
  try {
    documents.value = await invoke<ResearchDocument[]>("list_research_inbox");
  } catch (e) {
    documentsError.value = String(e);
  } finally {
    documentsLoading.value = false;
  }
}

async function deleteDocument(name: string) {
  let failure: string | null = null;
  try {
    await invoke("delete_research_document", { name });
  } catch (e) {
    failure = String(e);
  }
  // Re-read the folder either way so the list matches disk (on a failure the
  // file may already be gone). refreshDocuments() clears documentsError, so
  // restore a delete failure *after* it — otherwise a failed delete followed by
  // a successful list silently swallows the error and leaves the file in place.
  await refreshDocuments();
  if (failure !== null) documentsError.value = failure;
}

async function revealInbox() {
  // Best-effort: opening the folder in Finder is a convenience, not a data path.
  try {
    await invoke("reveal_research_inbox");
  } catch (e) {
    documentsError.value = String(e);
  }
}

async function refreshArchive() {
  archiveLoading.value = true;
  archiveError.value = null;
  try {
    archiveDocuments.value = await invoke<ResearchDocument[]>(
      "list_research_archive"
    );
  } catch (e) {
    archiveError.value = String(e);
  } finally {
    archiveLoading.value = false;
  }
}

async function deleteArchiveDocument(name: string) {
  let failure: string | null = null;
  try {
    await invoke("delete_research_archive_document", { name });
  } catch (e) {
    failure = String(e);
  }
  // Re-read either way so the list matches disk (mirrors the inbox delete). The
  // refresh clears archiveError, so restore a delete failure *after* it.
  await refreshArchive();
  if (failure !== null) archiveError.value = failure;
}

async function revealArchive() {
  // Best-effort: opening the folder in Finder is a convenience, not a data path.
  try {
    await invoke("reveal_research_archive");
  } catch (e) {
    archiveError.value = String(e);
  }
}

async function refreshSettings() {
  settingsLoading.value = true;
  settingsError.value = null;
  // A fresh load supersedes any prior test state — a saved key may have changed.
  // Bump the epoch first so any in-flight test resolving after this is discarded
  // (it tested the old saved value), then clear the per-credential channels.
  settingsEpoch.value += 1;
  connectionTesting.value = emptyConnectionState(false);
  connectionTests.value = emptyConnectionState<ConnectionTestResult | null>(null);
  try {
    settings.value = await invoke<SettingsView>("get_settings");
  } catch (e) {
    settingsError.value = String(e);
  } finally {
    settingsLoading.value = false;
  }
  // Diagnostics telemetry loads on its own channel — the backend command is
  // fail-soft (an empty aggregate on a DB error), so a throw here is only an
  // IPC-layer fault; swallow it to a null the section simply omits, never
  // letting it disturb the settings form above.
  try {
    truncationStats.value = await invoke<TruncationStats>("truncation_stats");
  } catch {
    truncationStats.value = null;
  }
  void refreshSchwabStatus();
}

// The Schwab connection view, read from local storage/Keychain (no network). Its
// own channel — fail-soft to null (the section is then omitted, like diagnostics),
// never disturbing the settings form.
async function refreshSchwabStatus() {
  try {
    schwabStatus.value = await invoke<SchwabStatus>("schwab_status");
  } catch {
    schwabStatus.value = null;
  }
}

// Persist the Schwab developer-app credentials (client_id → app_settings,
// client_secret → Keychain), then re-read the status so the secret placeholder and
// the connect gating reflect the save. Errors land on schwabError, apart from the
// config form's settingsError.
async function saveSchwabCredentials(payload: SchwabCredentialUpdate) {
  schwabError.value = null;
  try {
    await invoke("save_schwab_credentials", {
      clientId: payload.client_id,
      clientSecret: payload.client_secret,
    });
  } catch (e) {
    schwabError.value = String(e);
    return;
  }
  await refreshSchwabStatus();
}

// Run the interactive OAuth connect: schwab_connect stands up the loopback, opens
// the browser, and blocks until the login completes (or times out). Refresh the
// status + the warning band afterward so a newly-connected account clears its gate.
async function connectSchwab() {
  if (schwabConnecting.value || schwabBusy.value) return;
  schwabConnecting.value = true;
  schwabError.value = null;
  try {
    await invoke("schwab_connect");
  } catch (e) {
    schwabError.value = String(e);
  } finally {
    schwabConnecting.value = false;
  }
  await refreshSchwabStatus();
  // Re-sync the run-slot view: a focus bounce during the browser login may have
  // polled `is_running: true`, and without this the footer would keep showing a
  // running row until the next focus change.
  void refreshJobStatus();
  // A fresh connection clears the schwab presence warning (and unlocks the
  // Portfolio triggers) without waiting for a focus change.
  void refreshLocalValidation();
}

// Clear the stored OAuth session (keeps the saved credentials), then refresh the
// status so the surface returns to its not-connected state.
async function disconnectSchwab() {
  schwabError.value = null;
  try {
    await invoke("schwab_disconnect");
  } catch (e) {
    schwabError.value = String(e);
  }
  await refreshSchwabStatus();
  // Disconnecting re-raises the schwab presence warning and re-locks the
  // Portfolio triggers immediately.
  void refreshLocalValidation();
}

// Run the Portfolio Analysis job (docs/portfolio-analysis.md §Triggering — the
// one-touch trigger: it pulls fresh holdings itself, never reusing a standalone
// pull). Mirrors generate(): the run streams into the shared tracker, which
// replaces the Portfolio page while it runs; a gate block (no run-started ever
// arrives) surfaces as the page's inline error, never a persistent warning.
async function generatePortfolio() {
  if (localBlocked.value || slotBusy.value) return;
  portfolioRunning.value = true;
  portfolioError.value = null;
  runTraceKind.value = "portfolio";
  portfolioPaneMode.value = "tracker";
  cancelRequested.value = false;
  runTrace.value = null;
  try {
    const run = await invoke<PortfolioRun>("generate_portfolio_manual");
    // The inline result is fresher than any read already in flight — invalidate
    // them so a slow pre-run read can't clobber it.
    portfolioEpoch++;
    portfolioRun.value = run;
    // Return to the results only if the user is still watching the run; if they
    // navigated elsewhere the log lingers, reopenable from the footer.
    if (portfolioPaneMode.value === "tracker") portfolioPaneMode.value = "results";
  } catch (e) {
    if (cancelRequested.value) {
      // Intentional cancel — the tracker's terminal state carries it.
    } else if (!runTrace.value) {
      // Blocked before any event (run-gate / connectivity): inline on the page.
      portfolioError.value = String(e);
      portfolioPaneMode.value = "results";
    }
    // Otherwise the tracker's failed terminal state + failed-job warning carry it.
  } finally {
    portfolioRunning.value = false;
    void refreshJobStatus();
    void refreshValidation();
    void refreshLocalValidation();
    void refreshPortfolio();
  }
}

// Standalone view-only holdings pull (docs/portfolio-analysis.md §Triggering):
// fetches + persists the latest snapshot, never triggers analysis, never
// becomes the diff baseline. Quick — no tracker; the footer labels the slot.
async function pullHoldings() {
  if (pullBlocked.value || slotBusy.value) return;
  pullingHoldings.value = true;
  portfolioError.value = null;
  try {
    const pull = await invoke<HoldingsPull>("pull_holdings");
    portfolioEpoch++;
    holdingsPull.value = pull;
    // The direct assignment supersedes any in-flight read, whose finally will
    // now skip the loading flag — settle it here so it can't strand true.
    portfolioLoading.value = false;
  } catch (e) {
    portfolioError.value = String(e);
  } finally {
    pullingHoldings.value = false;
    void refreshJobStatus();
    // A failed pull can mean the connection lapsed mid-session — re-derive the
    // presence gate so the warning band and button locks don't go stale until
    // the next focus change (generatePortfolio's finally does the same).
    void refreshLocalValidation();
  }
}

// Test one saved credential against its provider. Result lands on that
// credential's own channel; a failed invoke is itself a failed test (kept apart
// from settingsError). The backend reads the saved value — never anything typed
// in the form — so the Settings view only enables this when the field is empty.
// The epoch guard discards a result whose settings were reloaded mid-flight, so a
// stale test for a replaced key can't repopulate the cleared state.
async function testConnection(provider: CredentialKey) {
  const epoch = settingsEpoch.value;
  connectionTesting.value = { ...connectionTesting.value, [provider]: true };
  connectionTests.value = { ...connectionTests.value, [provider]: null };
  try {
    const result = await invoke<ConnectionTestResult>("test_connection", {
      provider,
    });
    if (epoch !== settingsEpoch.value) return;
    connectionTests.value = { ...connectionTests.value, [provider]: result };
  } catch (e) {
    if (epoch !== settingsEpoch.value) return;
    connectionTests.value = {
      ...connectionTests.value,
      [provider]: { ok: false, detail: String(e) },
    };
  } finally {
    // Only clear the busy flag if this test is still the current one; otherwise
    // a reload already reset it and a late finally must not touch fresh state.
    if (epoch === settingsEpoch.value) {
      connectionTesting.value = { ...connectionTesting.value, [provider]: false };
    }
  }
}

async function saveSettings(payload: {
  models: AgentModels;
  credentials: CredentialUpdate;
}) {
  settingsSaving.value = true;
  settingsError.value = null;
  try {
    await invoke("save_settings", payload);
  } catch (e) {
    // Set the error before clearing `saving` so the Settings view's saved-edge
    // watch sees a failure and doesn't flash "Saved".
    settingsError.value = String(e);
    settingsSaving.value = false;
    return;
  }
  settingsSaving.value = false;
  // Re-read settings (resets the form baseline and flips credential placeholders
  // to "saved") and re-check both gates so completing config clears the warnings.
  void refreshSettings();
  void refreshValidation();
  void refreshLocalValidation();
}

// Switch surfaces. Fetch settings on entry so the configured-flags and model
// selections are fresh each time the view is opened; the Portfolio page
// re-reads its persisted state + presence gate the same way.
function navigate(next: AppView) {
  view.value = next;
  if (next === "settings") void refreshSettings();
  if (next === "portfolio") {
    void refreshPortfolio();
    void refreshLocalValidation();
  }
}

const unlisteners: UnlistenFn[] = [];

onMounted(async () => {
  getVersion()
    .then((v) => (appVersion.value = v))
    .catch(() => {});
  void refreshValidation();
  // The local-suite presence gate loads up front too, so the warning band is
  // proactive and the Portfolio triggers are correctly locked on first paint.
  void refreshLocalValidation();
  void refreshPortfolio();
  void refreshJobStatus();
  // Load the recent-reports list up front so the sidebar is populated and the
  // newest report shows in the pane on first paint.
  void refreshReports();
  // Load the inbox up front so the sidebar badge is populated even on the report
  // view, before the user ever opens the inbox. Same for the archive.
  void refreshDocuments();
  void refreshArchive();
  // Load settings up front so the gate's config state is known on first paint
  // (the report view's Generate button depends on it via the warning area).
  void refreshSettings();
  // Live run progress: the backend streams one of these per step / per baseline
  // request / per coalesced agent-token chunk while a run is in flight. They feed
  // the run tracker.
  unlisteners.push(
    await listen<ProgressMessage>("job-progress", (event) =>
      handleProgress(event.payload)
    )
  );
  // The window can be backgrounded while the app stays mounted, so onMounted
  // won't fire again on reopen. Re-sync on regaining focus — chiefly the research
  // inbox/archive, which the user can change in Finder while the app is unfocused;
  // config / status / reports are refreshed defensively too.
  unlisteners.push(
    await getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      if (focused) {
        void refreshValidation();
        void refreshLocalValidation();
        void refreshJobStatus();
        // Re-list defensively so the sidebar reflects the latest persisted state.
        void refreshReports();
        // The user may have dropped files into the inbox folder (via Finder)
        // while the app was unfocused — pick those up on return. The archive can
        // change too (the user deletes from it in Finder), so refresh it as well.
        void refreshDocuments();
        void refreshArchive();
      }
    })
  );
});

onUnmounted(() => unlisteners.forEach((u) => u()));
</script>

<template>
  <div class="app-root">
    <!-- Masthead titlebar (design kit Window.jsx): native traffic lights remain
         via titleBarStyle:Overlay; we draw the centered wordmark + hairline. The
         whole bar is a drag region. -->
    <header class="titlebar" data-tauri-drag-region>
      <div class="titlebar-brand">
        <span class="wordmark">Market Signal</span>
        <span class="wordmark-sub">{{ versionLabel }}</span>
      </div>
    </header>
    <div class="app-shell">
    <RecentReportsSidebar
      :reports="reports"
      :selected-report-id="selectedReportId"
      :reports-error="reportsError"
      :view="view"
      :inbox-count="inboxCount"
      :archive-count="archiveCount"
      @navigate="navigate"
      @select="selectAndShow"
    />
    <div class="main-column">
      <PersistentWarningArea
        :report="displayedValidation"
        :error="validationError"
        @dismiss="dismissWarning"
      />
      <div class="view-area">
        <template v-if="view === 'report'">
          <!-- While a run is in flight (or its terminal log is reopened) the pane
               shows the tracker in place of a report; selecting any report row
               flips reportPaneMode back to "report". Kind-gated like the
               Portfolio branch, so a portfolio run's trace never renders here
               (e.g. a failed report run left the pane in tracker mode, then a
               portfolio run replaced the trace). -->
          <JobTrackerView
            v-if="
              reportPaneMode === 'tracker' &&
              runTrace &&
              runTraceKind !== 'portfolio'
            "
            :trace="runTrace"
            :active="runActive"
            :cancel-requested="cancelRequested"
            @cancel="cancelRun"
            @close="closeRunLog"
          />
          <LatestReportView
            v-else
            :report="selectedReport"
            :error="error"
            :load-error="reportError"
            :is-latest="selectedIsLatest"
            :exporting-markdown="exportingMarkdown"
            :export-error="exportError"
            @export-markdown="exportMarkdown"
          />
        </template>
        <template v-else-if="view === 'portfolio'">
          <!-- The shared tracker replaces the Portfolio page while its run is
               in flight (docs/run-tracking.md — the run's owning page). -->
          <JobTrackerView
            v-if="
              portfolioPaneMode === 'tracker' &&
              runTrace &&
              runTraceKind === 'portfolio'
            "
            :trace="runTrace"
            :active="runActive"
            :cancel-requested="cancelRequested"
            @cancel="cancelRun"
            @close="portfolioPaneMode = 'results'"
          />
          <PortfolioView
            v-else
            :run="portfolioRun"
            :pull="holdingsPull"
            :loading="portfolioLoading"
            :load-error="portfolioLoadError"
            :run-error="portfolioError"
            :run-blocked="localBlocked"
            :run-blocked-reason="localBlockedReason"
            :pull-blocked="pullBlocked"
            :pull-blocked-reason="pullBlockedReason"
            :busy="slotBusy"
            :running="portfolioRunning"
            :pulling="pullingHoldings"
            @run="generatePortfolio"
            @pull="pullHoldings"
          />
        </template>
        <ResearchDocuments
          v-else-if="view === 'inbox'"
          :documents="documents"
          :loading="documentsLoading"
          :error="documentsError"
          title="Research inbox"
          lede="Filed research — read by the pipeline at the start of the next run. Nothing leaves your machine until you generate."
          empty-title="No documents"
          empty-body="Use “Add files…” to open the inbox folder, then drop in your PDFs, transcripts, or notes. The pipeline reads them at the start of the next run."
          error-label="Couldn't read the inbox"
          reveal-label="Add files…"
          reveal-title="Opens the inbox folder so you can drop documents in"
          reveal-icon="plus"
          reveal-variant="btn-primary"
          @delete="deleteDocument"
          @reveal="revealInbox"
        />
        <ResearchDocuments
          v-else-if="view === 'archive'"
          :documents="archiveDocuments"
          :loading="archiveLoading"
          :error="archiveError"
          title="Research archive"
          lede="Processed research — filed here automatically after the pipeline reads it, and kept on your machine for later citation."
          empty-title="No archived documents"
          empty-body="Documents move here from the inbox automatically once the pipeline has processed them at the start of a run. Nothing has been archived yet."
          error-label="Couldn't read the archive"
          reveal-label="Show in Finder"
          reveal-title="Opens the archive folder in Finder"
          reveal-variant="btn-secondary"
          @delete="deleteArchiveDocument"
          @reveal="revealArchive"
        />
        <Settings
          v-else-if="view === 'settings'"
          :settings="settings"
          :loading="settingsLoading"
          :saving="settingsSaving"
          :error="settingsError"
          :dark="dark"
          :testing="connectionTesting"
          :test-results="connectionTests"
          :truncation-stats="truncationStats"
          :schwab-status="schwabStatus"
          :schwab-connecting="schwabConnecting"
          :schwab-busy="schwabBusy"
          :schwab-error="schwabError"
          @save="saveSettings"
          @set-dark="setDark"
          @test="testConnection"
          @save-schwab="saveSchwabCredentials"
          @connect-schwab="connectSchwab"
          @disconnect-schwab="disconnectSchwab"
        />
      </div>
      <JobStatusPanel
        :status="jobStatus"
        :error="jobStatusError"
        :blocked="blocked"
        :generating="generating"
        :run-active="
          runActive || schwabConnecting || pullingHoldings || portfolioRunning
        "
        :running-label="footerRunningLabel"
        :progress="runProgress"
        :run-started-at="runStartedAt"
        :has-run-log="runTrace !== null"
        :viewing-tracker="
          (view === 'report' && reportPaneMode === 'tracker') ||
          (view === 'portfolio' && portfolioPaneMode === 'tracker')
        "
        @generate="generate"
        @view-tracker="viewTracker"
      />
      </div>
    </div>
  </div>
</template>

<style>
html,
body,
#app {
  margin: 0;
  height: 100%;
}

#app {
  height: 100vh;
}

body {
  background: var(--paper);
  color: var(--ink);
  font-family: var(--font-sans);
}

/* Print treatment — a design-system extension (the package defines no print
   surface). PDF export is the webview's native print-to-PDF (window.print →
   macOS "Save as PDF"), which prints the whole webview, so isolate the report
   body: hide every app-chrome surface and let the report flow and paginate
   instead of living in a fixed-height, internally-scrolling pane. !important is
   needed to win over the components' scoped (attribute-qualified) rules. */
@media print {
  .titlebar,
  .sidebar,
  .warning-area,
  .toolbar,
  .export-error,
  .job-panel {
    display: none !important;
  }

  html,
  body,
  #app,
  .app-root,
  .app-shell,
  .main-column,
  .view-area,
  .report-pane,
  .report-scroll {
    display: block !important;
    height: auto !important;
    min-height: 0 !important;
    overflow: visible !important;
    background: var(--paper) !important;
  }

  /* Print margins. We deliberately do NOT use @page margins: WebKit's
     print-to-PDF path drops content when an @page margin shrinks a page's
     capacity enough to need another page — it fails to create that page and
     silently discards the overflow (verified in demo: a 2cm @page margin ate a
     report's trailing table + Sources). So @page stays at 0 and the margins
     come from the report article's padding, which paginates normally and never
     drops content. The trade-off WebKit forces here: padding gives reliable
     left/right margins on every page and a top margin on the first page, but
     interior pages run to the top/bottom edge (no per-page vertical margin is
     possible without @page). Horizontal is the readability-critical axis and
     the one this fixes. The 2cm value is --print-page-margin in the design
     system. */
  @page {
    margin: 0;
  }

  .report-article {
    max-width: none !important;
    margin: 0 !important;
    /* left/right on every page + top on page 1; no bottom padding — it would
       overflow into a blank trailing page. */
    padding: var(--print-page-margin, 2cm) var(--print-page-margin, 2cm) 0 !important;
  }
}
</style>

<style scoped>
.app-root {
  display: flex;
  flex-direction: column;
  height: 100vh;
  background: var(--paper);
}

/* Masthead titlebar — full width above the sidebar/content row. The native
   traffic lights overlay its left (titleBarStyle:Overlay); the wordmark is
   absolutely centered so the lights never shift it. */
.titlebar {
  position: relative;
  flex-shrink: 0;
  height: 38px;
  background: var(--paper);
  border-bottom: var(--border);
}

.titlebar-brand {
  position: absolute;
  left: 50%;
  top: 50%;
  transform: translate(-50%, -50%);
  display: flex;
  align-items: baseline;
  gap: var(--s-3);
  white-space: nowrap;
  /* Let drags pass through to the bar's drag region. */
  pointer-events: none;
}

.wordmark {
  font-family: var(--font-serif);
  font-size: 15px;
  font-weight: 600;
  line-height: 1;
  color: var(--ink);
}

.wordmark-sub {
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: 0.18em;
  text-transform: uppercase;
  color: var(--ink-3);
}

.app-shell {
  display: flex;
  flex: 1;
  min-height: 0;
  background: var(--paper);
}

.main-column {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
}

/* Holds the active view between the (global) warning area above and the
   (global) job-status footer below. A flex container with one flex:1 child so
   the report pane or inbox fills the height and scrolls internally rather than
   pushing the footer off-screen. */
.view-area {
  flex: 1;
  min-height: 0;
  min-width: 0;
  display: flex;
}
</style>
