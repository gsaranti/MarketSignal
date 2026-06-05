<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getVersion } from "@tauri-apps/api/app";
import RecentReportsSidebar from "./components/RecentReportsSidebar.vue";
import LatestReportView from "./components/LatestReportView.vue";
import ResearchDocuments from "./components/ResearchDocuments.vue";
import Settings from "./components/Settings.vue";
import PersistentWarningArea from "./components/PersistentWarningArea.vue";
import JobStatusPanel from "./components/JobStatusPanel.vue";
import type {
  AppView,
  AgentModels,
  CredentialUpdate,
  GeneratedReport,
  JobStatus,
  ReportSummary,
  ResearchDocument,
  SettingsView,
  ValidationReport,
} from "./types";

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
  try {
    // A fresh run returns the full report (with Markdown) — show it directly and
    // refresh the list so its new row appears, selected, at the top.
    const fresh = await invoke<GeneratedReport>("generate_report_manual");
    selectedReport.value = fresh;
    selectedReportId.value = fresh.report_id;
    reportError.value = null;
    // Surface its row immediately so the sidebar never lags the pane, even if the
    // refresh below fails; refreshReports() reconciles ordering against the DB.
    upsertReportSummary(fresh.summary);
    void refreshReports();
  } catch (e) {
    error.value = String(e);
  } finally {
    generating.value = false;
    // Re-check after a run: config may have changed, and a run updates job
    // history (failed/missed warnings, last-run status). Fire-and-forget.
    void refreshValidation();
    void refreshJobStatus();
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
  // to the newest report so the pane is never blank when reports exist.
  const stillSelected =
    selectedReportId.value !== null &&
    reports.value.some((r) => r.report_id === selectedReportId.value);
  if (!stillSelected && reports.value.length > 0) {
    void selectReport(reports.value[0].report_id);
  }
}

async function selectReport(id: string) {
  selectedReportId.value = id;
  reportError.value = null;
  // Viewing a specific report dismisses any prior generation-failure banner —
  // otherwise LatestReportView's `error` block (which has render precedence)
  // would mask the report we just loaded.
  error.value = null;
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
  try {
    settings.value = await invoke<SettingsView>("get_settings");
  } catch (e) {
    settingsError.value = String(e);
  } finally {
    settingsLoading.value = false;
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
  unlisteners.push(
    await listen<GeneratedReport | null>("job-finished", (event) => {
      if (event.payload) {
        selectedReport.value = event.payload;
        selectedReportId.value = event.payload.report_id;
        reportError.value = null;
        // A fresh report supersedes any stale generation-failure banner.
        error.value = null;
        // Surface its row immediately (see generate) so a scheduled run's report
        // appears in the sidebar even if the list refresh below fails.
        upsertReportSummary(event.payload.summary);
        void refreshReports();
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
        <LatestReportView
          v-if="view === 'report'"
          :report="selectedReport"
          :error="error"
          :load-error="reportError"
          :is-latest="selectedIsLatest"
        />
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
          @save="saveSettings"
          @set-enabled="setJobEnabled"
        />
      </div>
      <JobStatusPanel
        :status="jobStatus"
        :error="jobStatusError"
        :blocked="blocked"
        :generating="generating"
        @generate="generate"
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
