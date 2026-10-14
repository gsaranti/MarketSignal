<script setup lang="ts">
import { computed } from "vue";
import Icon from "./Icon.vue";
import { localDateTime } from "../format";
import type { AppView, PortfolioRunSummary, ReportSummary } from "../types";

// The one shared-history sidebar (design kit Sidebar.jsx): only the history
// content swaps per feature (Portfolio runs on the Portfolio view, report
// issues everywhere else); structure and treatment never do.
const props = defineProps<{
  reports: ReportSummary[];
  selectedReportId: string | null;
  // Surfaces only with no list to fall back on; a refresh failure keeps the
  // stale list silently.
  reportsError: string | null;
  // Portfolio-runs history (docs/interface.md §Main Layout); its error has the
  // reports' posture.
  portfolioRuns: PortfolioRunSummary[];
  selectedRunId: string | null;
  portfolioRunsError: string | null;
  view: AppView;
  inboxCount: number;
  archiveCount: number;
}>();

defineEmits<{
  (e: "navigate", view: AppView): void;
  (e: "select", reportId: string): void;
  (e: "select-run", runId: string): void;
}>();

const feature = computed<"portfolio" | "report">(() =>
  props.view === "portfolio" ? "portfolio" : "report"
);

// Date and time: two reports from the same day must be distinguishable (the
// export filename stays date-only).
function shortStamp(iso: string): string {
  return localDateTime(iso);
}

function shortId(id: string): string {
  return id.slice(0, 8);
}

function rowTitle(r: ReportSummary): string {
  return r.title?.trim() || "Market Signal Report";
}

// An unreadable run's blob no longer decodes; its summary zeros are
// placeholders, not facts, so no count renders.
function runTitle(r: PortfolioRunSummary): string {
  if (!r.readable) return "Full book";
  const n = r.holdings_count;
  return `Full book · ${n} ${n === 1 ? "holding" : "holdings"}`;
}
</script>

<template>
  <aside class="sidebar">
    <template v-if="feature === 'portfolio'">
      <div class="sidebar-header">Portfolio runs · last 30</div>
      <div class="sidebar-list">
        <template v-if="portfolioRuns.length > 0">
          <button
            v-for="r in portfolioRuns"
            :key="r.run_id"
            type="button"
            class="row report-row"
            :class="{ 'is-current': r.run_id === selectedRunId }"
            :aria-current="r.run_id === selectedRunId ? 'true' : undefined"
            :disabled="!r.readable"
            :title="
              r.readable
                ? undefined
                : 'This run\'s stored record no longer decodes and cannot open'
            "
            @click="$emit('select-run', r.run_id)"
          >
            <div class="row-main">
              <div class="row-title">{{ runTitle(r) }}</div>
              <div class="row-meta">
                {{ shortStamp(r.created_at)
                }}<template v-if="r.readable">
                  · graded {{ r.graded_count }}</template
                >
                <span v-if="!r.readable" class="ana-tag row-tag">unreadable</span>
              </div>
            </div>
          </button>
        </template>
        <button
          v-else
          type="button"
          class="row report-row is-current"
          aria-current="true"
          :title="portfolioRunsError ?? undefined"
          @click="$emit('navigate', 'portfolio')"
        >
          <div class="row-main">
            <div class="row-title">Portfolio analysis</div>
            <div
              class="row-meta"
              :class="{ 'is-error': portfolioRunsError }"
              aria-live="polite"
            >
              {{ portfolioRunsError ? "Couldn't load runs" : "No runs yet" }}
            </div>
          </div>
        </button>
      </div>
    </template>
    <template v-else>
      <div class="sidebar-header">Recent reports · last 30</div>
      <div class="sidebar-list">
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
        <!-- The empty row is a path back to the report view at the spot a row
             would occupy; on a listing failure the meta says so rather than
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
    </template>

    <nav class="sidebar-nav" aria-label="Views">
      <!-- The report target leads the nav (docs/interface.md §Main Layout): on
           the Portfolio view the history swaps to runs, so without this entry
           the view is a navigation dead-end. -->
      <button
        type="button"
        class="nav-item"
        :class="{ 'is-active': view === 'report' }"
        :aria-current="view === 'report' ? 'page' : undefined"
        @click="$emit('navigate', 'report')"
      >
        <Icon name="report" :size="14" color="var(--ink-2)" />
        <span class="nav-label">Latest Market Report</span>
      </button>
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
.sidebar {
  width: 280px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  min-height: 0;
  border-right: var(--border);
  background: var(--paper-soft);
}

/* Fixed height so the bottom seam aligns with the collapsed warning band's
   across the column gutter. */
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

/* A <button> reusing the global `.row`: reset the button chrome `.row` doesn't
   set. min-height matches the report toolbar so the seams line up across the
   gutter. */
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

/* The global `.row` hover/current is paper-soft, the sidebar's own background;
   step one deeper so selection reads. */
.report-row:hover,
.report-row.is-current {
  background: var(--paper-edge);
}

/* `.row-title` sets `--ink` explicitly, which would beat the inherited
   disabled color, so the title needs its own override. */
.report-row:disabled {
  cursor: default;
  color: var(--ink-3);
}
.report-row:disabled .row-title {
  color: var(--ink-3);
}
.report-row:disabled:hover {
  background: transparent;
}

.row-main {
  min-width: 0;
}

.row-title {
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  font-weight: 600;
  color: var(--ink);
  /* Clamp the agent-written headline to two lines so a long one can't blow up
     the row height. */
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
  /* Tabular/lining numerals keep the stacked stamps aligned row-to-row. */
  font-variant-numeric: tabular-nums lining-nums;
  color: var(--ink-3);
  margin-top: var(--s-1);
}

.row-meta.is-error {
  color: var(--accent-text);
}

.row-meta .row-tag {
  margin-left: var(--s-1);
  vertical-align: top;
}

/* Nav geometry mirrors `.row` (2px edge + 14px offset) so a label aligns with
   the row titles above; no top padding so the first item's highlight meets the
   divider flush. */
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
