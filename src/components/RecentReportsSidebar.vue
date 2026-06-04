<script setup lang="ts">
import Icon from "./Icon.vue";
import type { AppView, GeneratedReport } from "../types";

defineProps<{
  report: GeneratedReport | null;
  view: AppView;
  inboxCount: number;
}>();

defineEmits<{ (e: "navigate", view: AppView): void }>();

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
      <!-- The report row doubles as the "back to report" nav: clickable (and
           keyboard-operable) even in the empty state, so a path back from the
           inbox always exists. -->
      <button
        type="button"
        class="row report-row"
        :class="{ 'is-current': view === 'report' }"
        :aria-current="view === 'report' ? 'true' : undefined"
        @click="$emit('navigate', 'report')"
      >
        <div class="row-main">
          <div class="row-title">Weekly Market Report</div>
          <div class="row-meta">
            <template v-if="report">
              {{ shortDate(report.summary.created_at) }} · #{{
                shortId(report.report_id)
              }}
            </template>
            <template v-else>No reports yet</template>
          </div>
        </div>
      </button>
    </div>

    <nav class="sidebar-nav" aria-label="Views">
      <button
        type="button"
        class="nav-item"
        :class="{ 'is-active': view === 'inbox' }"
        :aria-current="view === 'inbox' ? 'true' : undefined"
        @click="$emit('navigate', 'inbox')"
      >
        <Icon name="inbox" :size="14" color="var(--ink-2)" />
        <span class="nav-label">Research Inbox</span>
        <span v-if="inboxCount > 0" class="nav-badge">{{ inboxCount }}</span>
      </button>
      <button
        type="button"
        class="nav-item"
        :class="{ 'is-active': view === 'settings' }"
        :aria-current="view === 'settings' ? 'true' : undefined"
        @click="$emit('navigate', 'settings')"
      >
        <Icon name="settings" :size="14" color="var(--ink-2)" />
        <span class="nav-label">Settings</span>
      </button>
    </nav>
  </aside>
</template>

<style scoped>
.sidebar {
  width: 280px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  min-height: 0;
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
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}

/* The report row reuses the global `.row` (accent edge + hover) but is a
   <button>; reset the button chrome `.row` doesn't set so only the leading
   accent edge and bottom hairline read. */
.report-row {
  width: 100%;
  appearance: none;
  background: transparent;
  border-top: 0;
  border-right: 0;
  font: inherit;
  text-align: left;
}

.report-row:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
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

/* Bottom nav — targets at the foot of the panel (design kit Sidebar.jsx).
   Geometry mirrors `.row` (2px edge + 14px content offset) so a nav item's
   label aligns with the report-row title above it. */
.sidebar-nav {
  border-top: var(--border);
  padding-top: var(--s-2);
}

.nav-item {
  display: flex;
  align-items: center;
  gap: var(--s-4);
  width: 100%;
  appearance: none;
  padding: var(--s-3) var(--s-4) var(--s-3) 14px;
  border: 0;
  border-left: 2px solid transparent;
  background: transparent;
  cursor: pointer;
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  font-weight: 500;
  color: var(--ink);
  text-align: left;
  transition: background-color var(--dur-fast) var(--ease);
}

.nav-item:hover {
  background: var(--paper-soft);
}

.nav-item.is-active {
  background: var(--paper-soft);
  border-left-color: var(--accent);
  font-weight: 600;
}

.nav-item:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}

.nav-label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.nav-badge {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-variant-numeric: tabular-nums lining-nums;
  font-size: var(--t-caption);
  color: var(--ink-3);
}
</style>
