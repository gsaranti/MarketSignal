<script setup lang="ts">
import type { GeneratedReport } from "../types";

defineProps<{
  report: GeneratedReport | null;
}>();

function shortDate(iso: string): string {
  return iso.slice(0, 10);
}

function shortId(id: string): string {
  return id.slice(0, 8);
}
</script>

<template>
  <aside class="sidebar">
    <div class="sidebar-header">Recent Reports · last 30</div>
    <div class="sidebar-list">
      <div v-if="report" class="row is-current">
        <div class="row-main">
          <div class="row-title">Weekly Market Report</div>
          <div class="row-meta">
            {{ shortDate(report.summary.created_at) }} · #{{ shortId(report.report_id) }}
          </div>
        </div>
      </div>
      <div v-else class="sidebar-empty">No reports yet</div>
    </div>
  </aside>
</template>

<style scoped>
.sidebar {
  width: 280px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  border-right: var(--border);
  background: var(--paper);
}

.sidebar-header {
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  color: var(--ink-3);
  padding: var(--s-5) var(--s-5) var(--s-3);
  border-bottom: var(--border);
}

.sidebar-list {
  overflow-y: auto;
}

.row-main {
  min-width: 0;
}

.row-title {
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  font-weight: 600;
  color: var(--ink);
}

.row-meta {
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  color: var(--ink-3);
  margin-top: var(--s-1);
}

.sidebar-empty {
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  color: var(--ink-3);
  padding: var(--s-5);
}
</style>
