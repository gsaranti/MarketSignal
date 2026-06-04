<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import RecentReportsSidebar from "./components/RecentReportsSidebar.vue";
import LatestReportView from "./components/LatestReportView.vue";
import ResearchInbox from "./components/ResearchInbox.vue";
import PersistentWarningArea from "./components/PersistentWarningArea.vue";
import JobStatusPanel from "./components/JobStatusPanel.vue";
import type {
  AppView,
  GeneratedReport,
  JobStatus,
  ResearchDocument,
  ValidationReport,
} from "./types";

// Which main surface is showing. A plain ref switch (no router) — the app has a
// small fixed set of destinations and the kit models this as top-level state,
// not routes (see AppView in types.ts).
const view = ref<AppView>("report");

const report = ref<GeneratedReport | null>(null);
const generating = ref(false);
const error = ref<string | null>(null);

// Research inbox state lives here (not in the inbox view) so the sidebar badge
// can show the count regardless of which view is active, and a single load path
// keeps badge and list in sync.
const documents = ref<ResearchDocument[]>([]);
const documentsLoading = ref(false);
const documentsError = ref<string | null>(null);
const inboxCount = computed(() => documents.value.length);

const validation = ref<ValidationReport | null>(null);
const validationError = ref<string | null>(null);

const jobStatus = ref<JobStatus | null>(null);
const jobStatusError = ref<string | null>(null);
const jobBusy = ref(false);

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
    report.value = await invoke<GeneratedReport>("generate_report_manual");
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

const unlisteners: UnlistenFn[] = [];

onMounted(async () => {
  void refreshValidation();
  void refreshJobStatus();
  // Load the inbox up front so the sidebar badge is populated even on the report
  // view, before the user ever opens the inbox.
  void refreshDocuments();
  // The background scheduler emits this when a scheduled run finishes (or when it
  // detects an overslept window), so an open window reflects the new state
  // without a manual refresh. A successful run carries its report so the Latest
  // Report View updates too; failure/skip/missed send null.
  unlisteners.push(
    await listen<GeneratedReport | null>("job-finished", (event) => {
      if (event.payload) report.value = event.payload;
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
        // The user may have dropped files into the inbox folder (via Finder)
        // while the app was in the background — pick those up on return.
        void refreshDocuments();
      }
    })
  );
});

onUnmounted(() => unlisteners.forEach((u) => u()));
</script>

<template>
  <div class="app-shell">
    <RecentReportsSidebar
      :report="report"
      :view="view"
      :inbox-count="inboxCount"
      @navigate="view = $event"
    />
    <div class="main-column">
      <PersistentWarningArea :report="validation" :error="validationError" />
      <div class="view-area">
        <LatestReportView
          v-if="view === 'report'"
          :report="report"
          :generating="generating"
          :error="error"
          :blocked="blocked"
          @generate="generate"
        />
        <ResearchInbox
          v-else
          :documents="documents"
          :loading="documentsLoading"
          :error="documentsError"
          @delete="deleteDocument"
          @reveal="revealInbox"
        />
      </div>
      <JobStatusPanel
        :status="jobStatus"
        :error="jobStatusError"
        :busy="jobBusy"
        @set-enabled="setJobEnabled"
      />
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
.app-shell {
  display: flex;
  height: 100vh;
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
