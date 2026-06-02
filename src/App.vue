<script setup lang="ts">
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import RecentReportsSidebar from "./components/RecentReportsSidebar.vue";
import LatestReportView from "./components/LatestReportView.vue";
import type { GeneratedReport } from "./types";

const report = ref<GeneratedReport | null>(null);
const generating = ref(false);
const error = ref<string | null>(null);

async function generate() {
  generating.value = true;
  error.value = null;
  try {
    report.value = await invoke<GeneratedReport>("generate_report_manual");
  } catch (e) {
    error.value = String(e);
  } finally {
    generating.value = false;
  }
}
</script>

<template>
  <div class="app-shell">
    <RecentReportsSidebar :report="report" />
    <LatestReportView
      :report="report"
      :generating="generating"
      :error="error"
      @generate="generate"
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
</style>

<style scoped>
.app-shell {
  display: flex;
  height: 100vh;
  background: var(--paper);
}
</style>
