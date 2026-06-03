<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import RecentReportsSidebar from "./components/RecentReportsSidebar.vue";
import LatestReportView from "./components/LatestReportView.vue";
import PersistentWarningArea from "./components/PersistentWarningArea.vue";
import type { GeneratedReport, ValidationReport } from "./types";

const report = ref<GeneratedReport | null>(null);
const generating = ref(false);
const error = ref<string | null>(null);

const validation = ref<ValidationReport | null>(null);
const validationError = ref<string | null>(null);

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
    // Re-check after a run: config may have changed, and later slices surface
    // failed/missed-job warnings here. Fire-and-forget — it owns its errors.
    void refreshValidation();
  }
}

onMounted(refreshValidation);
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
