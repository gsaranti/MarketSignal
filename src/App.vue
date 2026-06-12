<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getVersion } from "@tauri-apps/api/app";
import RecentReportsSidebar from "./components/RecentReportsSidebar.vue";
import LatestReportView from "./components/LatestReportView.vue";
import JobTrackerView from "./components/JobTrackerView.vue";
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
  JobStatus,
  ProgressMessage,
  ReportSummary,
  ResearchDocument,
  RunTrace,
  SettingsView,
  StepStatus,
  TrackerStep,
  ValidationReport,
} from "./types";
import { readDark, writeDark } from "./theme";

// Which main surface is showing. A plain ref switch (no router) — the app has a
// small fixed set of destinations and the kit models this as top-level state,
// not routes (see AppView in types.ts).
const view = ref<AppView>("report");

// App version for the titlebar masthead ("Desk · v0.1.0"); falls back to "Desk"
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

// The synthetic first step: the run only ever starts once the execution gate has
// passed (`check_configuration`), so we show that as a completed step rather than
// emitting a backend event for an instant, already-done check.
const GATE_STEP_LABEL = "Credentials & configuration";

// Find or create a step by key. step-started always precedes its requests/tokens
// and step-finished, so the lookup normally hits; the create is a safety net.
function ensureStep(trace: RunTrace, key: string, label: string): TrackerStep {
  let step = trace.steps.find((s) => s.key === key);
  if (!step) {
    step = { key, label, status: "running", detail: null, requests: [], agentText: "" };
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
        },
      ],
      terminal: null,
    };
    runActive.value = true;
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
// "Latest run log" handle). Brings the report surface forward if another view is
// active, since the tracker lives in the report pane.
function viewTracker() {
  view.value = "report";
  reportPaneMode.value = "tracker";
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

const jobStatus = ref<JobStatus | null>(null);
const jobStatusError = ref<string | null>(null);
const jobBusy = ref(false);

// Settings state lives here alongside the other surfaces' state; the Settings
// view is presentational. One `settingsError` carries both load and save errors.
const settings = ref<SettingsView | null>(null);
const settingsLoading = ref(false);
const settingsSaving = ref(false);
const settingsError = ref<string | null>(null);

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

async function setJobEnabled(value: boolean) {
  jobBusy.value = true;
  let failure: string | null = null;
  try {
    await invoke("set_job_enabled", { enabled: value });
  } catch (e) {
    failure = String(e);
  } finally {
    jobBusy.value = false;
  }
  // Re-read the authoritative state, and refresh warnings: enabling/disabling
  // changes whether a missed-window warning applies. refreshJobStatus() clears
  // jobStatusError, so restore a toggle failure *after* it — otherwise a failed
  // toggle followed by a successful status read would silently swallow the error.
  await refreshJobStatus();
  if (failure !== null) jobStatusError.value = failure;
  void refreshValidation();
}

async function generate() {
  if (blocked.value) return;
  generating.value = true;
  error.value = null;
  // Show the live tracker for the run the user just kicked off; run-started will
  // populate it (and reset cancelRequested, but set it here too for the gap before
  // the first event lands).
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
    // history (failed/missed warnings, last-run status). Refresh reports too, so a
    // report that persisted but whose command then errored (e.g. a job-history write
    // failure after the report itself was written) still appears in the sidebar.
    void refreshValidation();
    void refreshJobStatus();
    void refreshReports();
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
  // to "saved") and re-check config so completing the gate clears the warnings.
  void refreshSettings();
  void refreshValidation();
}

// Switch surfaces. Fetch settings on entry so the configured-flags and model
// selections are fresh each time the view is opened.
function navigate(next: AppView) {
  view.value = next;
  if (next === "settings") void refreshSettings();
}

const unlisteners: UnlistenFn[] = [];

onMounted(async () => {
  getVersion()
    .then((v) => (appVersion.value = v))
    .catch(() => {});
  void refreshValidation();
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
  // The background scheduler emits this when a scheduled run finishes (or when it
  // detects an overslept window), so an open window reflects the new state
  // without a manual refresh. A successful run carries its report so the Latest
  // Report View updates too; failure/skip/missed send null.
  // Live run progress: the backend streams one of these per step / per baseline
  // request / per coalesced agent-token chunk while a run is in flight. They feed
  // the tracker for both manual and scheduled runs.
  unlisteners.push(
    await listen<ProgressMessage>("job-progress", (event) =>
      handleProgress(event.payload)
    )
  );
  unlisteners.push(
    await listen<GeneratedReport | null>("job-finished", (event) => {
      if (event.payload) {
        // Surface its row immediately (see generate) so a scheduled run's report
        // appears in the sidebar even if the list refresh below fails.
        upsertReportSummary(event.payload.summary);
        void refreshReports();
        // Switch to the fresh report only if the user was watching this run's
        // tracker; otherwise stay put (a scheduled run shouldn't yank a reader).
        if (reportPaneMode.value === "tracker") {
          selectedReport.value = event.payload;
          selectedReportId.value = event.payload.report_id;
          reportError.value = null;
          error.value = null;
          reportPaneMode.value = "report";
        }
      }
      void refreshValidation();
      void refreshJobStatus();
    })
  );
  // Closing to the tray hides the window but keeps this app mounted, so
  // onMounted won't fire again on reopen. Refresh when the window regains focus
  // so a missed-window warning surfaces on next open/resume, per
  // docs/scheduling.md §Missed Job Detection.
  unlisteners.push(
    await getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      if (focused) {
        void refreshValidation();
        void refreshJobStatus();
        // A scheduled run while backgrounded may have added a report; re-list so
        // the sidebar reflects it on return.
        void refreshReports();
        // The user may have dropped files into the inbox folder (via Finder)
        // while the app was in the background — pick those up on return. The
        // archive can change too (a background run files documents, or the user
        // deletes from it in Finder), so refresh it as well.
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
      <PersistentWarningArea :report="validation" :error="validationError" />
      <div class="view-area">
        <template v-if="view === 'report'">
          <!-- While a run is in flight (or its terminal log is reopened) the pane
               shows the tracker in place of a report; selecting any report row
               flips reportPaneMode back to "report". -->
          <JobTrackerView
            v-if="reportPaneMode === 'tracker' && runTrace"
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
          :job-enabled="jobStatus?.enabled ?? null"
          :job-busy="jobBusy"
          :dark="dark"
          :testing="connectionTesting"
          :test-results="connectionTests"
          @save="saveSettings"
          @set-enabled="setJobEnabled"
          @set-dark="setDark"
          @test="testConnection"
        />
      </div>
      <JobStatusPanel
        :status="jobStatus"
        :error="jobStatusError"
        :blocked="blocked"
        :generating="generating"
        :run-active="runActive"
        :has-run-log="runTrace !== null"
        :viewing-tracker="view === 'report' && reportPaneMode === 'tracker'"
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

  /* Let the column use the printable page width; the page margins come from the
     print panel (wry on macOS doesn't fully honor CSS @page margins). */
  .report-article {
    max-width: none !important;
    margin: 0 !important;
    padding: 0 !important;
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
