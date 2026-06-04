<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import RecentReportsSidebar from "./components/RecentReportsSidebar.vue";
import LatestReportView from "./components/LatestReportView.vue";
import PersistentWarningArea from "./components/PersistentWarningArea.vue";
import JobStatusPanel from "./components/JobStatusPanel.vue";
import type { GeneratedReport, JobStatus, ValidationReport } from "./types";

const report = ref<GeneratedReport | null>(null);
const generating = ref(false);
const error = ref<string | null>(null);

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
  try {
    await invoke("set_job_enabled", { enabled: value });
  } catch (e) {
    jobStatusError.value = String(e);
  } finally {
    jobBusy.value = false;
    // Re-read the authoritative state, and refresh warnings: enabling/disabling
    // changes whether a missed-window warning applies.
    await refreshJobStatus();
    void refreshValidation();
  }
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

const unlisteners: UnlistenFn[] = [];

onMounted(async () => {
  void refreshValidation();
  void refreshJobStatus();
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
      }
    })
  );
});

onUnmounted(() => unlisteners.forEach((u) => u()));
</script>

<template>
  <div class="app-shell">
    <RecentReportsSidebar :report="report" />
    <div class="main-column">
      <PersistentWarningArea :report="validation" :error="validationError" />
      <LatestReportView
        :report="report"
        :generating="generating"
        :error="error"
        :blocked="blocked"
        @generate="generate"
      />
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
</style>
