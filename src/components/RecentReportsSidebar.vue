<script setup lang="ts">
import Icon from "./Icon.vue";
import { localDateTime } from "../format";
import type { AppView, ReportSummary } from "../types";

defineProps<{
  reports: ReportSummary[];
  selectedReportId: string | null;
  // A failure to list the reports (sidebar-level). Only surfaces when there's no
  // list to fall back on — a refresh failure with an existing list keeps the
  // stale list silently rather than flashing an error.
  reportsError: string | null;
  view: AppView;
  inboxCount: number;
  archiveCount: number;
}>();

defineEmits<{
  (e: "navigate", view: AppView): void;
  (e: "select", reportId: string): void;
}>();

// The row's report date and time in local time, matching the report toolbar's
// dateline — the time is included so two reports generated on the same day are
// distinguishable (the export filename stays date-only; see ../format).
function shortStamp(iso: string): string {
  return localDateTime(iso);
}

function shortId(id: string): string {
  return id.slice(0, 8);
}

// The row's label: the agent's per-issue headline, falling back to the product
// name for reports persisted before titles existed (an empty title).
function rowTitle(r: ReportSummary): string {
  return r.title?.trim() || "Market Signal Report";
}
</script>

<template>
  <aside class="sidebar">
    <div class="sidebar-header">Recent reports · last 30</div>
    <div class="sidebar-list">
      <!-- One row per persisted report, newest first. Selecting a row loads that
           issue into the report pane (App handles the load + view switch). -->
      <template v-if="reports.length > 0">
        <button
          v-for="r in reports"
          :key="r.report_id"
          type="button"
          class="row report-row"
          :class="{
            'is-current': view === 'report' && r.report_id === selectedReportId,
          }"
          :aria-current="
            view === 'report' && r.report_id === selectedReportId
              ? 'true'
              : undefined
          "
          @click="$emit('select', r.report_id)"
        >
          <div class="row-main">
            <div class="row-title">{{ rowTitle(r) }}</div>
            <div class="row-meta">
              {{ shortStamp(r.created_at) }} · #{{ shortId(r.report_id) }}
            </div>
          </div>
        </button>
      </template>
      <!-- Empty state keeps a path back to the (empty) report view from the
           inbox/archive/settings surfaces, since the bottom nav has no report
           target. Clickable and keyboard-operable like a real row. When listing
           failed (and there's no list to show), the meta says so rather than
           misreporting an empty library as "No reports yet". -->
      <button
        v-else
        type="button"
        class="row report-row"
        :class="{ 'is-current': view === 'report' }"
        :aria-current="view === 'report' ? 'true' : undefined"
        :title="reportsError ?? undefined"
        @click="$emit('navigate', 'report')"
      >
        <div class="row-main">
          <div class="row-title">Market Signal Report</div>
          <div
            class="row-meta"
            :class="{ 'is-error': reportsError }"
            aria-live="polite"
          >
            {{ reportsError ? "Couldn't load reports" : "No reports yet" }}
          </div>
        </div>
      </button>
    </div>

    <nav class="sidebar-nav" aria-label="Views">
      <button
        type="button"
        class="nav-item"
        :class="{ 'is-active': view === 'portfolio' }"
        :aria-current="view === 'portfolio' ? 'page' : undefined"
        @click="$emit('navigate', 'portfolio')"
      >
        <Icon name="portfolio" :size="14" color="var(--ink-2)" />
        <span class="nav-label">Portfolio</span>
      </button>
      <button
        type="button"
        class="nav-item"
        :class="{ 'is-active': view === 'inbox' }"
        :aria-current="view === 'inbox' ? 'page' : undefined"
        @click="$emit('navigate', 'inbox')"
      >
        <Icon name="inbox" :size="14" color="var(--ink-2)" />
        <span class="nav-label">Research Inbox</span>
        <span v-if="inboxCount > 0" class="nav-badge">{{ inboxCount }}</span>
      </button>
      <button
        type="button"
        class="nav-item"
        :class="{ 'is-active': view === 'archive' }"
        :aria-current="view === 'archive' ? 'page' : undefined"
        @click="$emit('navigate', 'archive')"
      >
        <Icon name="archive" :size="14" color="var(--ink-2)" />
        <span class="nav-label">Research Archive</span>
        <span v-if="archiveCount > 0" class="nav-badge">{{ archiveCount }}</span>
      </button>
      <button
        type="button"
        class="nav-item"
        :class="{ 'is-active': view === 'settings' }"
        :aria-current="view === 'settings' ? 'page' : undefined"
        @click="$emit('navigate', 'settings')"
      >
        <Icon name="settings" :size="14" color="var(--ink-2)" />
        <span class="nav-label">Settings</span>
      </button>
    </nav>
  </aside>
</template>

<style scoped>
/* Recessed chrome: the sidebar sits one tonal step below the paper reading
   surface so the boundary between navigation and report content is legible
   without leaning on the hairline alone. */
.sidebar {
  width: 280px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  min-height: 0;
  border-right: var(--border);
  background: var(--paper-soft);
}

/* Header row: fixed height + centered so its bottom seam aligns with the
   collapsed warning band's across the column gutter (both are the "header" tier). */
.sidebar-header {
  display: flex;
  align-items: center;
  min-height: 44px;
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  color: var(--ink-3);
  padding: 0 var(--s-5);
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
/* Item row: matched to the report toolbar's height (the "item/title" tier) and
   centered, so its bottom seam lines up with the toolbar's across the gutter. */
.report-row {
  width: 100%;
  min-height: 50px;
  align-items: center;
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

/* The global `.row` hover/current is paper-soft, which now matches the sidebar
   background — step it one deeper so selection still reads. */
.report-row:hover,
.report-row.is-current {
  background: var(--paper-edge);
}

.row-main {
  min-width: 0;
}

.row-title {
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  font-weight: 600;
  color: var(--ink);
  /* The title is now an agent-written, variable-length headline. Clamp it to two
     lines with an ellipsis so a long one can't blow up the row height, and break
     a long unbroken token rather than overflow the column (frontend-craft overflow
     handling; no new tokens). */
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  line-clamp: 2;
  overflow: hidden;
  overflow-wrap: anywhere;
}

.row-meta {
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  /* The stamp + short-id are figures that stack down the column; tabular/lining
     numerals keep digits aligned row-to-row and steady on selection. */
  font-variant-numeric: tabular-nums lining-nums;
  color: var(--ink-3);
  margin-top: var(--s-1);
}

/* List-load failure: accent draws the eye to a problem, matching the report
   pane's error-label treatment rather than reading as quiet caption metadata. */
.row-meta.is-error {
  color: var(--accent-text);
}

/* Bottom nav — targets at the foot of the panel (design kit Sidebar.jsx).
   Geometry mirrors `.row` (2px edge + 14px content offset) so a nav item's
   label aligns with the report-row title above it. */
/* No top padding: the first nav item's selected highlight (and its accent edge)
   meets the divider flush, rather than leaving a sliver of gap above it. */
.sidebar-nav {
  border-top: var(--border);
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
  background: var(--paper-edge);
}

.nav-item.is-active {
  background: var(--paper-edge);
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
