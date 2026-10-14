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
import ConfirmDialog from "./components/ConfirmDialog.vue";
import type {
  AppView,
  AgentModels,
  ArchiveInfo,
  ConnectionTestResult,
  CredentialKey,
  CredentialUpdate,
  ExportSummary,
  GeneratedReport,
  HoldingsPull,
  ImportInspection,
  ImportSummary,
  InvestorProfileDisplay,
  JobStatus,
  LocalDaemonStatus,
  LocalModelSettings,
  WebResearchPreflight,
  WebResearchSettings,
  PortfolioResumeStatus,
  PortfolioRun,
  PortfolioRunSummary,
  QuickCheckState,
  ProgressMessage,
  ProviderCredentialUpdate,
  ReportSummary,
  ResearchDocument,
  RunTrace,
  SchwabCredentialUpdate,
  SchwabStatus,
  SettingsView,
  StepStatus,
  TrackerRequest,
  TrackerStep,
  TruncationStats,
  ValidationReport,
  WarningCategory,
} from "./types";
import { readDark, writeDark } from "./theme";
import { localDate } from "./format";

// The main surface showing — a ref switch, no router.
const view = ref<AppView>("report");

const appVersion = ref("");
const versionLabel = computed(() =>
  appVersion.value ? `Desk · v${appVersion.value}` : "Desk"
);

// Newest first, capped by the backend. Selection is held by id so the sidebar
// highlight and the pane share one source of truth.
const reports = ref<ReportSummary[]>([]);
const selectedReportId = ref<string | null>(null);
const selectedReport = ref<GeneratedReport | null>(null);
// Kept apart on purpose: `reportError` is a failure to open the selected
// report, `reportsError` a failure to list the sidebar, so a transient list
// refresh never masks a loaded report.
const reportError = ref<string | null>(null);
const reportsError = ref<string | null>(null);
const generating = ref(false);
const error = ref<string | null>(null);

// --- Live job tracker ------------------------------------------------------
// While a run is in flight the owning page shows the tracker; `runTrace` is
// in-session, latest-run-only.
const reportPaneMode = ref<"report" | "tracker">("report");
const runTrace = ref<RunTrace | null>(null);
const runActive = ref(false);
const cancelRequested = ref(false);
// The tracker renders on the run's owning page (docs/run-tracking.md); the
// footer's determinate fill applies only to the report's fixed pipeline.
const runTraceKind = ref<"report" | "portfolio" | "portfolio-quick-check" | null>(null);
const portfolioPaneMode = ref<"results" | "tracker">("results");
// Epoch ms, stamped at run-started; null until a run starts this session.
const runStartedAt = ref<number | null>(null);

// The synthetic first step: the execution gate passed before the run existed,
// so it shows as already complete rather than as a backend event.
const GATE_STEP_LABEL = "Credentials & configuration";

// The backend's step order (pipeline.rs) plus the synthesized gate; only the
// footer's progress denominator.
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

// The footer's determinate progress; null unless a report run is in flight.
const runProgress = computed<{
  fraction: number;
  stepNumber: number;
  total: number;
  label: string;
} | null>(() => {
  const trace = runTrace.value;
  if (!trace || !runActive.value) return null;
  // A portfolio run's steps are dynamic (one per holding), so it gets no
  // fraction.
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
  // The in-flight step counts a half step, so the bar never reads 100% until
  // the run actually completes.
  const fraction = (completed + (running ? 0.5 : 0)) / total;
  return {
    fraction,
    stepNumber: current.idx,
    total,
    label: current.label,
  };
});

// step-started always precedes a step's other events, so the create is a safety net.
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

// Ownership is stamped onto request events by the backend (progress.rs), never
// inferred here: an unstamped row renders in the unattributed list and can never
// conjure a step that later paints FAILED onto a successful run.
function requestHome(trace: RunTrace, step: string | undefined): TrackerRequest[] {
  return step ? ensureStep(trace, step, step).requests : trace.unattributed;
}

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
      unattributed: [],
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
    case "step-started": {
      const step = ensureStep(trace, msg.step ?? "", msg.label ?? msg.step ?? "");
      step.status = "running";
      // Upgrade a safety-net step's key-as-label to the backend's label.
      step.label = msg.label ?? step.label;
      break;
    }
    case "step-finished": {
      const step = ensureStep(trace, msg.step ?? "", msg.step ?? "");
      step.status = (msg.status as StepStatus) ?? "ok";
      step.detail = msg.detail ?? null;
      break;
    }
    case "request-started": {
      // One row per actual HTTP request; skipped series never emit this.
      requestHome(trace, msg.step).push({
        provider: msg.provider ?? "",
        group: msg.group ?? "",
        seriesId: msg.series_id ?? "",
        name: msg.name ?? msg.series_id ?? "",
        status: "running",
        detail: null,
        target: msg.target ?? null,
      });
      break;
    }
    case "request-finished": {
      const home = requestHome(trace, msg.step);
      // Requests are sequential within a step, so the running row for this
      // group+series is the one; append if its start was missed.
      const row = home.find(
        (r) =>
          r.status === "running" &&
          r.group === (msg.group ?? "") &&
          r.seriesId === (msg.series_id ?? "")
      );
      if (row) {
        row.status = msg.status ?? "ok";
        row.detail = msg.detail ?? null;
      } else {
        home.push({
          provider: msg.provider ?? "",
          group: msg.group ?? "",
          seriesId: msg.series_id ?? "",
          name: msg.name ?? msg.series_id ?? "",
          status: msg.status ?? "ok",
          detail: msg.detail ?? null,
          target: msg.target ?? null,
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
      const step = ensureStep(trace, "analysts", "Running the analyst agents");
      const posture = msg.posture ?? "";
      step.analystThinking[posture] = (step.analystThinking[posture] ?? "") + (msg.delta ?? "");
      break;
    }
    case "stream-restarted": {
      // Clear the panes so the re-attempt doesn't stream after the failed
      // attempt's text.
      if (msg.posture) {
        const step = ensureStep(trace, "analysts", "Running the analyst agents");
        step.analystThinking[msg.posture] = "";
      } else {
        const step = ensureStep(trace, "agent", "Main agent");
        step.agentText = "";
        step.agentThinking = "";
      }
      break;
    }
    case "step-thinking": {
      const key = msg.step ?? "";
      ensureStep(trace, key, key).agentThinking += msg.delta ?? "";
      break;
    }
    case "run-finished": {
      trace.terminal = { status: msg.status ?? "", detail: msg.detail ?? null };
      runActive.value = false;
      // Close any step still "running" with the terminal status, so a stage
      // that never emitted step-finished can't read in-progress forever or
      // paint FAILED onto a successful run.
      const fallback: StepStatus =
        msg.status === "cancelled" ? "cancelled" : msg.status === "successful" ? "ok" : "failed";
      for (const step of trace.steps) {
        if (step.status === "running") step.status = fallback;
      }
      break;
    }
  }
}

// The backend stops at its next checkpoint and emits run-finished{cancelled};
// until then Cancel reads "Cancelling…".
async function cancelRun() {
  if (!runActive.value) return;
  cancelRequested.value = true;
  try {
    await invoke("cancel_run");
  } catch (e) {
    // Surface on the run-status channel and re-enable the button for a retry.
    cancelRequested.value = false;
    jobStatusError.value = String(e);
  }
}

// The log stays reopenable from the footer; only the next run replaces it.
function closeRunLog() {
  reportPaneMode.value = "report";
}

// The tracker replaces the page whose job is running (docs/run-tracking.md).
function viewTracker() {
  if (
    runTraceKind.value === "portfolio" ||
    runTraceKind.value === "portfolio-quick-check"
  ) {
    view.value = "portfolio";
    portfolioPaneMode.value = "tracker";
  } else {
    view.value = "report";
    reportPaneMode.value = "tracker";
  }
}

// Markdown export only; PDF export lives in LatestReportView (window.print).
const exportingMarkdown = ref(false);
const exportError = ref<string | null>(null);

// The list is newest-first, so the head is the latest.
const selectedIsLatest = computed(
  () =>
    selectedReportId.value !== null &&
    reports.value.length > 0 &&
    reports.value[0].report_id === selectedReportId.value
);

// Held here, not in the inbox view, so the sidebar badge counts regardless of
// the active view.
const documents = ref<ResearchDocument[]>([]);
const documentsLoading = ref(false);
const documentsError = ref<string | null>(null);
const inboxCount = computed(() => documents.value.length);

// The inbox's read-only twin, loaded up front for the sidebar badge.
const archiveDocuments = ref<ResearchDocument[]>([]);
const archiveLoading = ref(false);
const archiveError = ref<string | null>(null);
const archiveCount = computed(() => archiveDocuments.value.length);

const validation = ref<ValidationReport | null>(null);
const validationError = ref<string | null>(null);

// The local-suite gate (check_local_configuration; docs/interface.md
// §Persistent Warning Area): its categories join the band, but its is_blocked
// gates only the local jobs, never the report.
const localValidation = ref<ValidationReport | null>(null);

const jobStatus = ref<JobStatus | null>(null);
const jobStatusError = ref<string | null>(null);

// --- Portfolio page state ----------------------------------------------------
// docs/portfolio-analysis.md §Storage and display, §Triggering; PortfolioView
// is presentational.
const portfolioRun = ref<PortfolioRun | null>(null);
const holdingsPull = ref<HoldingsPull | null>(null);
const portfolioLoading = ref(false);
const portfolioLoadError = ref<string | null>(null);
// The page's inline, ephemeral error — never a persistent warning.
const portfolioError = ref<string | null>(null);
const portfolioRunning = ref(false);
// Probed when a failed or cancelled run ends, cleared when any run starts
// (docs/portfolio-analysis.md §Failure posture); null = nothing to offer.
const portfolioResume = ref<PortfolioResumeStatus | null>(null);
const pullingHoldings = ref(false);
// docs/portfolio-analysis.md §The quick check; null until one runs, cleared by
// the next full run.
const quickCheck = ref<QuickCheckState | null>(null);
const quickChecking = ref(false);

// The sidebar's runs history (docs/interface.md §Main Layout);
// `historicalRun === null` means the page shows the live latest view.
const portfolioRuns = ref<PortfolioRunSummary[]>([]);
const portfolioRunsError = ref<string | null>(null);
// An unreadable history row makes the empty state read "unreadable" rather
// than "never ran".
const unreadableHistory = computed(() =>
  portfolioRuns.value.some((r) => !r.readable)
);
const historicalRun = ref<PortfolioRun | null>(null);
// Its own channel (never `portfolioError`, whose block reads "Couldn't run");
// cleared by the next selection or back-to-latest.
const historicalError = ref<string | null>(null);

const displayedPortfolioRun = computed(
  () => historicalRun.value ?? portfolioRun.value
);
const sidebarRunId = computed(
  () => historicalRun.value?.run_id ?? portfolioRun.value?.run_id ?? null
);

// Fail-safe: until the local check resolves the local jobs read blocked, so
// their triggers are never briefly clickable.
const localBlocked = computed(() => localValidation.value?.is_blocked ?? true);
const localCategories = computed(() => localValidation.value?.categories ?? []);
const schwabCategory = computed(
  () => localCategories.value.find((c) => c.kind === "schwab") ?? null
);
// The view-only pull needs only the Schwab connection, so it gates on that
// category alone.
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

// Both gates' categories in one block, de-duplicated by kind: the two gates
// legitimately name the same category (an unset FMP key fails both), and a
// plain concatenation would render it twice — a duplicate Vue key, against
// interface.md's one-row-per-category contract. Items merge in first-seen
// order with byte-identical lines collapsed (each gate composes its own
// sentence, so overlapping-but-different sets leave a verbose row; fixing that
// is a `WarningCategory` contract change, not this merge's job). The first
// `dismiss_id` wins; only the cloud gate produces the dismissible category.
// The cloud is_blocked keeps its report-gate meaning.
const displayedValidation = computed<ValidationReport | null>(() => {
  if (validation.value === null && localValidation.value === null) return null;
  const byKind = new Map<string, WarningCategory>();
  for (const cat of [
    ...(validation.value?.categories ?? []),
    ...(localValidation.value?.categories ?? []),
  ]) {
    const seen = byKind.get(cat.kind);
    if (seen === undefined) {
      byKind.set(cat.kind, { ...cat, items: [...cat.items] });
      continue;
    }
    for (const item of cat.items) {
      if (!seen.items.includes(item)) seen.items.push(item);
    }
    seen.dismiss_id ??= cat.dismiss_id;
  }
  return {
    categories: [...byKind.values()],
    is_blocked: validation.value?.is_blocked ?? true,
  };
});

async function refreshLocalValidation() {
  try {
    localValidation.value = await invoke<ValidationReport>(
      "check_local_configuration"
    );
  } catch {
    // An unreadable local gate reads as blocked (localBlocked's null fallback)
    // rather than silently unlocked; validationError already surfaces the fault.
    localValidation.value = null;
  }
}

// Invalidates in-flight portfolio reads so an older response resolving late
// never overwrites newer state.
let portfolioEpoch = 0;

async function refreshPortfolio() {
  const epoch = ++portfolioEpoch;
  portfolioLoading.value = true;
  portfolioLoadError.value = null;
  try {
    const [run, pull, quick] = await Promise.all([
      invoke<PortfolioRun | null>("latest_portfolio_run"),
      invoke<HoldingsPull | null>("latest_holdings_pull"),
      invoke<QuickCheckState | null>("latest_quick_check"),
    ]);
    if (epoch !== portfolioEpoch) return;
    portfolioRun.value = run;
    holdingsPull.value = pull;
    quickCheck.value = quick;
  } catch (e) {
    if (epoch !== portfolioEpoch) return;
    portfolioLoadError.value = String(e);
  }
  // Its own channel: a listing hiccup must never blank the run/pull state above.
  try {
    const runs = await invoke<PortfolioRunSummary[]>("list_portfolio_runs");
    if (epoch !== portfolioEpoch) return;
    portfolioRuns.value = runs;
    portfolioRunsError.value = null;
    // An open past run that fell out of retention closes back to the latest
    // view rather than lingering as an unlisted orphan.
    if (
      historicalRun.value !== null &&
      !runs.some((r) => r.run_id === historicalRun.value?.run_id)
    ) {
      historicalRun.value = null;
    }
  } catch (e) {
    if (epoch !== portfolioEpoch) return;
    portfolioRunsError.value = String(e);
  }
  // Loading clears only once both the page state and the listing have landed:
  // the unreadable-history empty state derives from the listing, so clearing
  // earlier would paint "No holdings yet." over an unreadable-only store.
  if (epoch === portfolioEpoch) portfolioLoading.value = false;
}

// Invalidates in-flight historical-run fetches so a slow older-run fetch never
// reopens the read-only view after the user moved on; separate from
// portfolioEpoch so closing the view never discards a page refresh.
let historicalSeq = 0;

// The live latest row shows the live view; any other row renders read-only. An
// id pruned between listing and click re-lists and stays on the latest.
async function selectPortfolioRun(runId: string) {
  view.value = "portfolio";
  const seq = ++historicalSeq;
  historicalError.value = null;
  if (runId === portfolioRun.value?.run_id) {
    historicalRun.value = null;
    return;
  }
  const epoch = portfolioEpoch;
  try {
    const run = await invoke<PortfolioRun | null>("get_portfolio_run", {
      runId,
    });
    // Superseded by a newer selection (seq) or fresher page state (epoch).
    if (seq !== historicalSeq || epoch !== portfolioEpoch) return;
    if (run === null) {
      historicalRun.value = null;
      void refreshPortfolio();
      return;
    }
    historicalRun.value = run;
  } catch (e) {
    if (seq !== historicalSeq || epoch !== portfolioEpoch) return;
    historicalError.value = String(e);
  }
}

function backToLatestRun() {
  historicalSeq++;
  historicalRun.value = null;
  historicalError.value = null;
}

// `settingsError` carries the cloud form's load and save errors; the other
// submissions ride their own channels.
const settings = ref<SettingsView | null>(null);
const settingsLoading = ref(false);
const settingsSaving = ref(false);
const settingsError = ref<string | null>(null);
const providersSaving = ref(false);
const providersError = ref<string | null>(null);
const localSaving = ref(false);
const localSaveError = ref<string | null>(null);
const webResearchSaving = ref(false);
const webResearchSaveError = ref<string | null>(null);

// Never probed at startup (docs/interface.md §Connection status); null reads
// untested. Reset on every settings reload.
const localDaemonTesting = ref(false);
const localDaemonStatus = ref<LocalDaemonStatus | null>(null);

// The same never-probed-at-startup posture as the daemon row.
const searxngTesting = ref(false);
const searxngStatus = ref<WebResearchPreflight | null>(null);

// The pre-run web-research notice (docs/interface.md §Pre-run web-research
// notice): a confirm-and-proceed dialog, never a block. Carries the launch
// arguments so a confirm resumes exactly the run asked for.
const portfolioNotice = ref<{
  preflight: WebResearchPreflight;
  selected?: string[];
  resume: boolean;
} | null>(null);

// null = not loaded or unavailable. Kept off `settingsError` so a diagnostics
// hiccup never blanks the form.
const truncationStats = ref<TruncationStats | null>(null);

// docs/configuration.md §Investor Profile; null = not loaded or IPC fault (the
// section is omitted).
const investorProfile = ref<InvestorProfileDisplay | null>(null);

// Per-credential test state, reset on every settings load so a stale chip
// never outlives the value it described.
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
// Bumped on every settings (re)load so a test still resolving against the old
// saved value is discarded.
const settingsEpoch = ref(0);

// Charles Schwab connection (docs/schwab-integration.md); `schwabError` is kept
// off settingsError.
const schwabStatus = ref<SchwabStatus | null>(null);
const schwabConnecting = ref(false);
const schwabError = ref<string | null>(null);

// Data portability (docs/data-portability.md). Both operations hold the backend
// run slot; `pendingImport` awaits the replace-all confirmation.
const dataBusy = ref<"export" | "import" | null>(null);
const dataError = ref<string | null>(null);
const dataStatus = ref<string | null>(null);
const pendingImport = ref<{
  path: string;
  passphrase: string;
  info: ArchiveInfo;
} | null>(null);
const importConfirmBusy = ref(false);

// The single run slot is held or about to be claimed; every other trigger
// disables while it is. The session flags cover the click-to-first-poll window.
const slotBusy = computed(
  () =>
    generating.value ||
    portfolioRunning.value ||
    quickChecking.value ||
    pullingHoldings.value ||
    schwabConnecting.value ||
    dataBusy.value !== null ||
    importConfirmBusy.value ||
    (jobStatus.value?.is_running ?? false)
);

// Connecting holds the single run slot, so Connect is disabled while any run is
// in flight.
const schwabBusy = computed(
  () => generating.value || portfolioRunning.value || quickChecking.value ||
    pullingHoldings.value || dataBusy.value !== null || importConfirmBusy.value ||
    (jobStatus.value?.is_running ?? false)
);

// The session flags cover the click-to-first-poll window (job_status refreshes
// on focus and at run edges, not on an interval); the backend's `running_kind`
// is the authoritative read that survives a webview reload.
const footerRunningLabel = computed(() => {
  const kind = jobStatus.value?.running_kind ?? null;
  if (schwabConnecting.value || kind === "schwab-connect")
    return "Connecting to Charles Schwab…";
  if (pullingHoldings.value || kind === "holdings-pull")
    return "Pulling holdings…";
  if (portfolioRunning.value || kind === "portfolio")
    return "Running Portfolio Analysis…";
  if (quickChecking.value || kind === "portfolio-quick-check")
    return "Running Portfolio quick check…";
  if (dataBusy.value === "export") return "Exporting data…";
  if (dataBusy.value === "import" || importConfirmBusy.value)
    return "Importing data…";
  // After a webview reload the session flag that told export from import is gone.
  if (kind === "data-portability") return "Moving data…";
  return "Generating report…";
});

// Which section's stamps the footer reports (docs/interface.md §Main Layout);
// the Generate trigger is gated separately.
const footerSection = computed<"report" | "portfolio">(() =>
  view.value === "portfolio" ? "portfolio" : "report"
);

// The backend is the authoritative guard. Fail-safe: until the first check
// resolves, Generate reads blocked.
const blocked = computed(() => validation.value?.is_blocked ?? true);

async function refreshValidation() {
  validationError.value = null;
  try {
    validation.value = await invoke<ValidationReport>("check_configuration");
  } catch (e) {
    validationError.value = String(e);
  }
}

// Dismiss by the identity the row rendered, so a stale click can't suppress a
// newer warning the backend would re-derive. Best-effort: on failure the
// refresh keeps the band truthful.
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

// Applied to <html> in main.ts before mount; this ref keeps the Settings switch
// in sync (see ./theme).
const dark = ref(readDark());
function setDark(value: boolean) {
  writeDark(value);
  dark.value = value;
}

async function generate() {
  if (blocked.value) return;
  generating.value = true;
  error.value = null;
  // run-started also resets cancelRequested; set it here for the gap before the
  // first event.
  runTraceKind.value = "report";
  reportPaneMode.value = "tracker";
  cancelRequested.value = false;
  // Dropped up front so a failure before run-started leaves runTrace null and
  // the catch surfaces the error instead of the previous run's log.
  runTrace.value = null;
  try {
    const fresh = await invoke<GeneratedReport>("generate_report_manual");
    upsertReportSummary(fresh.summary);
    void refreshReports();
    // Only if the user is still watching the run; otherwise the new row appears
    // in the sidebar and the log lingers.
    if (reportPaneMode.value === "tracker") {
      selectedReport.value = fresh;
      selectedReportId.value = fresh.report_id;
      reportError.value = null;
      reportPaneMode.value = "report";
    }
  } catch (e) {
    if (cancelRequested.value) {
      // Intentional cancel — the tracker's terminal state carries it.
    } else if (!runTrace.value) {
      // Failed before any event (a gate block, a concurrency skip): inline on
      // the report pane so the failure isn't silent.
      error.value = String(e);
      reportPaneMode.value = "report";
    }
    // Otherwise the tracker's failed terminal state + the warning area carry it.
  } finally {
    generating.value = false;
    // Refresh reports too: a report that persisted but whose command then
    // errored (a job-history write failure) still appears in the sidebar.
    void refreshValidation();
    void refreshJobStatus();
    void refreshReports();
    // The run may have archived inbox documents or recorded parse failures.
    void refreshDocuments();
    void refreshArchive();
  }
}

// Mirrors storage::RECENT_REPORTS_LIMIT, so the optimistic insert honors the
// cap even when the reconciling refresh fails.
const RECENT_REPORTS_LIMIT = 30;

// Optimistic head insert (the report is already persisted); refreshReports()
// reconciles the ordering, and the trim keeps the cap if that refresh fails.
function upsertReportSummary(summary: ReportSummary) {
  reports.value = [
    summary,
    ...reports.value.filter((r) => r.report_id !== summary.report_id),
  ].slice(0, RECENT_REPORTS_LIMIT);
}

async function refreshReports() {
  try {
    reports.value = await invoke<ReportSummary[]>("list_reports");
    reportsError.value = null;
  } catch (e) {
    reportsError.value = String(e);
    return;
  }
  // Default to the newest report when none is selected, but only into a blank
  // report pane: selectReport() would switch away from the tracker and clear a
  // surfaced generation error.
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
  // Leaves the tracker; the run keeps going and the footer returns to it.
  reportPaneMode.value = "report";
  // LatestReportView's `error` block has render precedence, so it would mask
  // the loaded report.
  error.value = null;
  // A prior export failure belonged to the issue open then.
  exportError.value = null;
  try {
    const loaded = await invoke<GeneratedReport>("load_report", {
      reportId: id,
    });
    // A slower earlier load may resolve after a newer selection.
    if (selectedReportId.value !== id) return;
    selectedReport.value = loaded;
  } catch (e) {
    if (selectedReportId.value !== id) return;
    // Clear the pane rather than show a stale body.
    selectedReport.value = null;
    reportError.value = String(e);
  }
}

function selectAndShow(id: string) {
  view.value = "report";
  void selectReport(id);
}

// docs/export.md; a cancelled Save dialog returns false, not an error.
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
  // Re-read either way so the list matches disk; refreshDocuments() clears
  // documentsError, so restore a delete failure after it.
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
  // Re-read either way; the refresh clears archiveError, so restore a delete
  // failure after it.
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
  // Bump the epoch first so an in-flight test resolving after this is discarded.
  settingsEpoch.value += 1;
  connectionTesting.value = emptyConnectionState(false);
  connectionTests.value = emptyConnectionState<ConnectionTestResult | null>(null);
  localDaemonTesting.value = false;
  localDaemonStatus.value = null;
  // The SearXNG status also carries the job-launch preflight the indicator
  // must keep showing (docs/interface.md §Connection status), so it is dropped
  // below only when the reload changed the saved endpoint.
  searxngTesting.value = false;
  const priorSearxngEndpoint = settings.value?.web_research.searxng_endpoint ?? null;
  try {
    settings.value = await invoke<SettingsView>("get_settings");
  } catch (e) {
    settingsError.value = String(e);
  } finally {
    settingsLoading.value = false;
  }
  const searxngEndpoint = settings.value?.web_research.searxng_endpoint ?? null;
  if (priorSearxngEndpoint !== null && searxngEndpoint !== priorSearxngEndpoint) {
    searxngStatus.value = null;
  }
  // A throw here is only an IPC-layer fault; null omits the section.
  try {
    truncationStats.value = await invoke<TruncationStats>("truncation_stats");
  } catch {
    truncationStats.value = null;
  }
  // Same fail-soft posture.
  try {
    investorProfile.value =
      await invoke<InvestorProfileDisplay>("get_investor_profile");
  } catch {
    investorProfile.value = null;
  }
  void refreshSchwabStatus();
}

// No network; fail-soft to null, which omits the section.
async function refreshSchwabStatus() {
  try {
    schwabStatus.value = await invoke<SchwabStatus>("schwab_status");
  } catch {
    schwabStatus.value = null;
  }
}

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

// schwab_connect blocks until the browser login completes or times out.
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
  // A focus bounce during the login may have polled `is_running: true`; re-sync
  // so the footer doesn't show a running row until the next focus change.
  void refreshJobStatus();
  // A fresh connection clears the schwab presence warning without waiting for
  // a focus change.
  void refreshLocalValidation();
}

// Clears the OAuth session, keeping the saved credentials.
async function disconnectSchwab() {
  schwabError.value = null;
  try {
    await invoke("schwab_disconnect");
  } catch (e) {
    schwabError.value = String(e);
  }
  await refreshSchwabStatus();
  // Re-raises the schwab presence warning and re-locks the Portfolio triggers.
  void refreshLocalValidation();
}

// The job-launch probe, epoch-guarded like the manual test: a result resolving
// after a settings reload described the OLD endpoint, so a raced probe is
// discarded and re-run (bounded). The in-epoch result updates the Settings
// indicator (docs/interface.md §Connection status).
async function probeSearxngForLaunch(): Promise<WebResearchPreflight> {
  for (let attempt = 0; attempt < 3; attempt++) {
    const epoch = settingsEpoch.value;
    let preflight: WebResearchPreflight;
    try {
      preflight = await invoke<WebResearchPreflight>("test_searxng");
    } catch (e) {
      preflight = {
        status: "unreachable",
        detail: String(e),
        degraded: true,
      };
    }
    // A probe issued while a web-research save is mid-flight could describe
    // either endpoint.
    if (epoch === settingsEpoch.value && !webResearchSaving.value) {
      searxngStatus.value = preflight;
      return preflight;
    }
  }
  // Three raced attempts: proceed degraded-unknown rather than spin; the notice
  // is a consent step, never a block.
  return {
    status: "unreachable",
    detail: "settings changed while probing",
    degraded: true,
  };
}

// The one-touch trigger (docs/portfolio-analysis.md §Triggering); a gate block
// surfaces as the page's inline error.
async function generatePortfolio(selected?: string[], resume = false) {
  if (localBlocked.value || slotBusy.value || webResearchSaving.value) return;
  // Probe SearXNG before spending the run; degraded opens the notice
  // (docs/interface.md §Pre-run web-research notice), never a block. A failed
  // probe reads degraded-unknown rather than silently ok.
  const preflight = await probeSearxngForLaunch();
  if (preflight.degraded) {
    portfolioNotice.value = { preflight, selected, resume };
    return;
  }
  await launchPortfolio(selected, resume);
}

// Confirm relaunches exactly the run asked for; cancel just closes.
function confirmPortfolioNotice() {
  const notice = portfolioNotice.value;
  portfolioNotice.value = null;
  if (notice) void launchPortfolio(notice.selected, notice.resume);
}
function cancelPortfolioNotice() {
  portfolioNotice.value = null;
}

// docs/web-research.md §Tavily fallback.
const portfolioNoticeBody = computed(() => {
  const p = portfolioNotice.value?.preflight;
  if (!p) return "";
  const cause =
    p.status === "not_configured"
      ? "No SearXNG endpoint is configured"
      : "The SearXNG instance can't serve search";
  return `${cause}, so holdings will be analyzed without web research — the local suite is SearXNG-only, so evidence will be thinner and conviction lower. Running now is not recommended.`;
});

async function launchPortfolio(selected?: string[], resume = false) {
  // A mid-flight endpoint save blocks too: the notice's confirm would otherwise
  // launch against a config the probe never described.
  if (localBlocked.value || slotBusy.value || webResearchSaving.value) return;
  portfolioRunning.value = true;
  portfolioError.value = null;
  // Any starting run supersedes a standing resume offer; the post-failure
  // re-probe re-offers whatever trail survives.
  portfolioResume.value = null;
  runTraceKind.value = "portfolio";
  portfolioPaneMode.value = "tracker";
  cancelRequested.value = false;
  runTrace.value = null;
  try {
    const run = await invoke<PortfolioRun>("generate_portfolio_manual", {
      selected: selected && selected.length > 0 ? selected : null,
      resume: resume || null,
    });
    // The inline result is fresher than any read in flight.
    portfolioEpoch++;
    portfolioRun.value = run;
    // A fresh run closes an open past run and supersedes any historical fetch.
    historicalSeq++;
    historicalRun.value = null;
    // Only if the user is still watching; otherwise the log lingers.
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
    // A stopped run may have left a resumable trail (docs/portfolio-analysis.md
    // §Failure posture).
    void refreshPortfolioResume();
  } finally {
    portfolioRunning.value = false;
    void refreshJobStatus();
    void refreshValidation();
    void refreshLocalValidation();
    void refreshPortfolio();
  }
}

// Fail-soft: an unreadable probe just withholds the offer.
async function refreshPortfolioResume() {
  try {
    portfolioResume.value = await invoke<PortfolioResumeStatus>(
      "portfolio_resume_status"
    );
  } catch {
    portfolioResume.value = null;
  }
}

// docs/portfolio-analysis.md §The quick check: engine-only, never rewrites a
// verdict.
async function runQuickCheck() {
  if (localBlocked.value || slotBusy.value) return;
  quickChecking.value = true;
  portfolioError.value = null;
  runTraceKind.value = "portfolio-quick-check";
  portfolioPaneMode.value = "tracker";
  cancelRequested.value = false;
  runTrace.value = null;
  try {
    const state = await invoke<QuickCheckState>("run_portfolio_quick_check");
    quickCheck.value = state;
    if (portfolioPaneMode.value === "tracker") portfolioPaneMode.value = "results";
  } catch (e) {
    if (cancelRequested.value) {
      // Intentional cancel — the tracker's terminal state carries it.
    } else if (!runTrace.value) {
      // Blocked before any event (gate / no prior run): inline on the page.
      portfolioError.value = String(e);
      portfolioPaneMode.value = "results";
    }
    // Otherwise the tracker's failed terminal state + failed-job warning carry it.
  } finally {
    quickChecking.value = false;
    void refreshJobStatus();
    void refreshLocalValidation();
  }
}

// The view-only pull (docs/portfolio-analysis.md §Triggering): never triggers
// analysis, never the diff baseline. No tracker; the footer labels the slot.
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
    // A failed pull can mean the connection lapsed; re-derive the presence gate
    // so the band and button locks don't go stale until the next focus change.
    void refreshLocalValidation();
  }
}

// The backend tests the saved value, never the form's; a failed invoke is
// itself a failed test. The epoch guard discards a result from before a reload.
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
    // After a reload a late finally must not touch the fresh state.
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
  // Both gates re-check so completing config clears the warnings.
  void refreshSettings();
  void refreshValidation();
  void refreshLocalValidation();
}

// docs/configuration.md §External Data Provider Credentials; both gates
// re-check since the shared category serves both.
async function saveProviderCredentials(payload: ProviderCredentialUpdate) {
  providersSaving.value = true;
  providersError.value = null;
  try {
    await invoke("save_provider_credentials", { credentials: payload });
  } catch (e) {
    // Error before the saving edge clears, so the view's saved-watch sees it.
    providersError.value = String(e);
    providersSaving.value = false;
    return;
  }
  providersSaving.value = false;
  void refreshSettings();
  void refreshValidation();
  void refreshLocalValidation();
}

// The presence gate re-checks right after, so filling the fields unlocks the
// local-suite Run buttons.
async function saveLocalModelSettings(payload: LocalModelSettings) {
  localSaving.value = true;
  localSaveError.value = null;
  try {
    await invoke("save_local_model_settings", { values: payload });
  } catch (e) {
    localSaveError.value = String(e);
    localSaving.value = false;
    return;
  }
  localSaving.value = false;
  void refreshSettings();
  void refreshLocalValidation();
}

// Off every gate, so no validation re-check follows the save.
async function saveWebResearchSettings(payload: WebResearchSettings) {
  webResearchSaving.value = true;
  webResearchSaveError.value = null;
  // The save supersedes the old endpoint the moment it starts: bump the epoch
  // now so an in-flight probe resolving mid-save is discarded; the refresh
  // below bumps again once the new value is readable. A standing pre-run
  // notice described the old endpoint, so it closes too.
  settingsEpoch.value += 1;
  portfolioNotice.value = null;
  try {
    await invoke("save_web_research_settings", { values: payload });
  } catch (e) {
    webResearchSaveError.value = String(e);
    webResearchSaving.value = false;
    return;
  }
  webResearchSaving.value = false;
  void refreshSettings();
}

// Same epoch discipline as the daemon probe.
async function testSearxng() {
  const epoch = settingsEpoch.value;
  searxngTesting.value = true;
  searxngStatus.value = null;
  try {
    const result = await invoke<WebResearchPreflight>("test_searxng");
    if (epoch !== settingsEpoch.value) return;
    searxngStatus.value = result;
  } catch (e) {
    if (epoch !== settingsEpoch.value) return;
    searxngStatus.value = {
      status: "unreachable",
      detail: String(e),
      degraded: true,
    };
  } finally {
    if (epoch === settingsEpoch.value) searxngTesting.value = false;
  }
}

// Same epoch discipline as the credential tests.
async function testLocalDaemon() {
  const epoch = settingsEpoch.value;
  localDaemonTesting.value = true;
  localDaemonStatus.value = null;
  try {
    const result = await invoke<LocalDaemonStatus>("test_local_daemon");
    if (epoch !== settingsEpoch.value) return;
    localDaemonStatus.value = result;
  } catch (e) {
    if (epoch !== settingsEpoch.value) return;
    localDaemonStatus.value = {
      reachable: false,
      detail: String(e),
      missing_models: [],
    };
  } finally {
    if (epoch === settingsEpoch.value) localDaemonTesting.value = false;
  }
}

// --- Data portability (docs/data-portability.md) -----------------------------

// docs/data-portability.md §Export flow: success surfaces the path and counts.
function dataSummaryText(
  verb: "Exported" | "Imported",
  s: ExportSummary | ImportSummary,
  dest?: string
): string {
  const parts = `${s.reports} reports, ${s.learnings} learnings, and ${s.files} files`;
  const where = dest ? ` to ${dest}` : "";
  const skipped =
    "skipped_reports" in s && s.skipped_reports > 0
      ? ` Skipped ${s.skipped_reports} report record(s) whose Markdown was missing from the archive.`
      : "";
  return `${verb} ${parts}${where}.${skipped}`;
}

async function exportData(passphrase: string) {
  dataBusy.value = "export";
  dataError.value = null;
  dataStatus.value = null;
  try {
    // Null on a cancelled Save dialog — no status line, no error.
    const summary = await invoke<ExportSummary | null>("export_data", {
      passphrase: passphrase || null,
    });
    if (summary)
      dataStatus.value = dataSummaryText("Exported", summary, summary.path);
  } catch (e) {
    dataError.value = String(e);
  } finally {
    dataBusy.value = null;
    void refreshJobStatus();
  }
}

// Phase one: inspect; a non-empty store routes through the replace-all dialog.
async function importData(passphrase: string) {
  dataBusy.value = "import";
  dataError.value = null;
  dataStatus.value = null;
  try {
    const inspection = await invoke<ImportInspection | null>(
      "import_data_inspect",
      { passphrase: passphrase || null }
    );
    if (!inspection) return; // cancelled dialog
    if (inspection.store_empty) {
      await commitImport(inspection.path, passphrase, false);
    } else {
      pendingImport.value = {
        path: inspection.path,
        passphrase,
        info: inspection.info,
      };
    }
  } catch (e) {
    dataError.value = String(e);
  } finally {
    dataBusy.value = null;
    void refreshJobStatus();
  }
}

// Phase two. Every surface re-fetches in place: commands open the DB per call,
// so no backend state survives to go stale.
async function commitImport(path: string, passphrase: string, replace: boolean) {
  const summary = await invoke<ImportSummary>("import_data", {
    path,
    passphrase: passphrase || null,
    replace,
  });
  dataStatus.value = dataSummaryText("Imported", summary);
  // The selected report may no longer exist; refreshReports re-selects.
  selectedReportId.value = null;
  selectedReport.value = null;
  void refreshReports();
  void refreshPortfolio();
  void refreshDocuments();
  void refreshArchive();
  void refreshValidation();
  void refreshLocalValidation();
}

// Names the picked archive so the user confirms against the actual artifact;
// the real failure mode of a destructive restore is the wrong or older archive.
const importDetail = computed(() => {
  const pending = pendingImport.value;
  if (!pending) return "";
  const { info } = pending;
  return (
    `The selected archive was created ${localDate(info.created_at)} and holds ` +
    `${info.reports} reports, ${info.learnings} learnings, and ${info.files} files.`
  );
});

async function confirmImport() {
  const pending = pendingImport.value;
  if (!pending) return;
  importConfirmBusy.value = true;
  try {
    await commitImport(pending.path, pending.passphrase, true);
  } catch (e) {
    dataError.value = String(e);
  } finally {
    importConfirmBusy.value = false;
    pendingImport.value = null;
    void refreshJobStatus();
  }
}

function cancelImport() {
  pendingImport.value = null;
}

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
  void refreshLocalValidation();
  void refreshPortfolio();
  void refreshJobStatus();
  void refreshReports();
  void refreshDocuments();
  void refreshArchive();
  void refreshSettings();
  unlisteners.push(
    await listen<ProgressMessage>("job-progress", (event) =>
      handleProgress(event.payload)
    )
  );
  // onMounted never fires again on reopen; re-sync on focus, chiefly the inbox
  // and archive the user can change in Finder while the app is unfocused.
  unlisteners.push(
    await getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      if (focused) {
        void refreshValidation();
        void refreshLocalValidation();
        void refreshJobStatus();
        void refreshReports();
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
    <!-- The native traffic lights overlay the left (titleBarStyle:Overlay); the
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
      :portfolio-runs="portfolioRuns"
      :selected-run-id="sidebarRunId"
      :portfolio-runs-error="portfolioRunsError"
      :view="view"
      :inbox-count="inboxCount"
      :archive-count="archiveCount"
      @navigate="navigate"
      @select="selectAndShow"
      @select-run="selectPortfolioRun"
    />
    <div class="main-column">
      <PersistentWarningArea
        :report="displayedValidation"
        :error="validationError"
        @dismiss="dismissWarning"
      />
      <div class="view-area">
        <template v-if="view === 'report'">
          <!-- Kind-gated against both portfolio kinds: a failed report run can
               leave the pane in tracker mode while a portfolio job replaces the
               trace. -->
          <JobTrackerView
            v-if="
              reportPaneMode === 'tracker' &&
              runTrace &&
              runTraceKind !== 'portfolio' &&
              runTraceKind !== 'portfolio-quick-check'
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
          <!-- The tracker replaces the page whose job is running (docs/run-tracking.md). -->
          <JobTrackerView
            v-if="
              portfolioPaneMode === 'tracker' &&
              runTrace &&
              (runTraceKind === 'portfolio' ||
                runTraceKind === 'portfolio-quick-check')
            "
            :trace="runTrace"
            :active="runActive"
            :cancel-requested="cancelRequested"
            :kind="
              runTraceKind === 'portfolio-quick-check'
                ? 'portfolio-quick-check'
                : 'portfolio'
            "
            :resume="runTraceKind === 'portfolio' ? portfolioResume : null"
            :resume-requested="portfolioRunning"
            @cancel="cancelRun"
            @close="portfolioPaneMode = 'results'"
            @resume="generatePortfolio(undefined, true)"
          />
          <PortfolioView
            v-else
            :run="displayedPortfolioRun"
            :pull="holdingsPull"
            :loading="portfolioLoading"
            :load-error="portfolioLoadError"
            :run-error="portfolioError"
            :run-blocked="localBlocked"
            :run-blocked-reason="localBlockedReason"
            :pull-blocked="pullBlocked"
            :pull-blocked-reason="pullBlockedReason"
            :busy="slotBusy || webResearchSaving"
            :running="portfolioRunning"
            :pulling="pullingHoldings"
            :historical="historicalRun !== null"
            :history-error="historicalError"
            :quick="quickCheck"
            :quick-checking="quickChecking"
            :unreadable-history="unreadableHistory"
            :history-unknown="portfolioRunsError !== null"
            @run="generatePortfolio"
            @quick-check="runQuickCheck"
            @pull="pullHoldings"
            @back-to-latest="backToLatestRun"
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
          :saving-providers="providersSaving"
          :providers-error="providersError"
          :saving-local="localSaving"
          :local-error="localSaveError"
          :local-testing="localDaemonTesting"
          :local-daemon="localDaemonStatus"
          :saving-web-research="webResearchSaving"
          :web-research-error="webResearchSaveError"
          :searxng-testing="searxngTesting"
          :searxng-status="searxngStatus"
          :truncation-stats="truncationStats"
          :investor-profile="investorProfile"
          :schwab-status="schwabStatus"
          :schwab-connecting="schwabConnecting"
          :schwab-busy="schwabBusy"
          :schwab-error="schwabError"
          :data-busy="dataBusy"
          :slot-busy="slotBusy"
          :data-error="dataError"
          :data-status="dataStatus"
          @save="saveSettings"
          @save-providers="saveProviderCredentials"
          @save-local="saveLocalModelSettings"
          @test-local="testLocalDaemon"
          @save-web-research="saveWebResearchSettings"
          @test-searxng="testSearxng"
          @set-dark="setDark"
          @test="testConnection"
          @save-schwab="saveSchwabCredentials"
          @connect-schwab="connectSchwab"
          @disconnect-schwab="disconnectSchwab"
          @export-data="exportData"
          @import-data="importData"
        />
      </div>
      <JobStatusPanel
        :status="jobStatus"
        :section="footerSection"
        :show-generate="view === 'report'"
        :error="jobStatusError"
        :blocked="blocked"
        :generating="generating"
        :run-active="
          runActive ||
          schwabConnecting ||
          pullingHoldings ||
          portfolioRunning ||
          quickChecking
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
    <!-- Replace-all import confirmation (docs/data-portability.md §Import flow). -->
    <ConfirmDialog
      :open="pendingImport !== null"
      title="Replace all analytical data?"
      body="Importing this archive replaces all existing reports, learnings, snapshots, and portfolio runs. Your API keys and settings are untouched. This cannot be undone."
      :detail="importDetail"
      confirm-label="Replace and import"
      :busy="importConfirmBusy"
      busy-status="Replacing all analytical data. This may take a moment."
      @confirm="confirmImport"
      @cancel="cancelImport"
    />
    <!-- Pre-run web-research notice (docs/interface.md §Pre-run web-research
         notice). Placed after the import dialog so specs that
         findComponent(ConfirmDialog) keep addressing the import flow. -->
    <ConfirmDialog
      :open="portfolioNotice !== null"
      title="Run with degraded web research?"
      :body="portfolioNoticeBody"
      :detail="portfolioNotice?.preflight.detail ?? undefined"
      confirm-label="Run anyway"
      :busy="false"
      busy-status=""
      @confirm="confirmPortfolioNotice"
      @cancel="cancelPortfolioNotice"
    />
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

/* Print treatment, a design-system extension. PDF export is the webview's
   native print, which prints the whole webview, so hide the app chrome and let
   the report flow and paginate. !important wins over the components' scoped
   rules. */
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

  /* No @page margins: WebKit's print-to-PDF silently drops overflow when an
     @page margin shrinks a page enough to need another page. The margins come
     from the article's padding instead, which paginates normally — reliable
     left/right margins on every page and a top margin on the first, interior
     pages running to the vertical edges. */
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

/* The native traffic lights overlay the left; the wordmark is absolutely
   centered so they never shift it. */
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

/* One flex:1 child fills the height and scrolls internally rather than pushing
   the footer off-screen. */
.view-area {
  flex: 1;
  min-height: 0;
  min-width: 0;
  display: flex;
}
</style>
