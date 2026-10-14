<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from "vue";
import type { JobStatus } from "../types";

// Job status footer (docs/scheduling.md §Job Status Visibility): run stamps
// scoped to the active section's job, and the in-flight indicator for the one
// shared run slot. The manual "Generate now" trigger lives here only when the
// report view is the main window; Portfolio keeps its own Run analysis trigger
// on the page.
const props = defineProps<{
  status: JobStatus | null;
  // Which job's stamps the footer reports (docs/interface.md §Main Layout);
  // every non-Portfolio view scopes to the report.
  section: "report" | "portfolio";
  // Separate from `section`: the neutral surfaces are section "report" but
  // must not show the button.
  showGenerate: boolean;
  error: string | null;
  blocked: boolean;
  generating: boolean;
  // Event-driven and immediate, independent of the polled job status; covers
  // the interactive Schwab connect too.
  runActive: boolean;
  // The caller's label: the slot is shared, and a Schwab login must never read
  // as "Generating report…".
  runningLabel: string;
  progress: { fraction: number; stepNumber: number; total: number; label: string } | null;
  // Epoch ms.
  runStartedAt: number | null;
  // A run trace exists this session (terminal or not).
  hasRunLog: boolean;
  viewingTracker: boolean;
}>();

defineEmits<{ (e: "generate"): void; (e: "view-tracker"): void }>();

const isRunning = computed(() => props.runActive || props.status?.is_running === true);

// Only the active section's stamps, so a portfolio run never stamps LAST RUN
// under report chrome.
const stamps = computed(() => (props.status ? props.status[props.section] : null));

const noRunYetLabel = computed(() =>
  props.section === "portfolio" ? "No analysis has run yet" : "No report has run yet"
);

// Silent until the first status resolves, so the footer doesn't flash empty on load.
const visible = computed(
  () =>
    props.status !== null ||
    props.error !== null ||
    props.runActive ||
    props.hasRunLog
);

const progressPct = computed(() => {
  const f = props.progress?.fraction ?? 0;
  return `${Math.round(Math.max(0, Math.min(1, f)) * 100)}%`;
});

// A ticking clock proves the run isn't frozen while a long step holds the fill
// steady. It ticks only while a run is in flight; `aria-hidden` keeps it out of
// the polite live region.
const now = ref(Date.now());
let timerId: ReturnType<typeof setInterval> | null = null;
function startTimer() {
  if (timerId !== null) return;
  now.value = Date.now();
  timerId = setInterval(() => {
    now.value = Date.now();
  }, 1000);
}
function stopTimer() {
  if (timerId !== null) {
    clearInterval(timerId);
    timerId = null;
  }
}
watch(
  isRunning,
  (running) => {
    if (running) startTimer();
    else stopTimer();
  },
  { immediate: true }
);
onUnmounted(stopTimer);

const elapsedLabel = computed(() => {
  // `== null` so undefined also reads as no start time, not arithmetic on NaN.
  if (props.runStartedAt == null) return null;
  const secs = Math.max(0, Math.floor((now.value - props.runStartedAt) / 1000));
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  if (m >= 60) {
    const h = Math.floor(m / 60);
    return `${h}:${String(m % 60).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  }
  return `${m}:${String(s).padStart(2, "0")}`;
});

// The backend persists UTC; render in the viewer's local time.
function formatLocal(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}
</script>

<template>
  <footer v-if="visible" class="job-panel">
    <div class="job-status" aria-live="polite">
      <div v-if="isRunning" class="job-running">
        <div class="job-running-row">
          <span class="job-running-label">{{ runningLabel }}</span>
          <span class="job-running-track" aria-hidden="true">
            <span class="job-running-fill" :style="{ width: progressPct }"></span>
          </span>
          <span v-if="elapsedLabel" class="job-running-time" aria-hidden="true">{{
            elapsedLabel
          }}</span>
        </div>
        <p v-if="progress" class="job-running-caption">
          Step {{ progress.stepNumber }} of {{ progress.total }} · {{ progress.label }}
        </p>
      </div>
      <p v-else-if="error" class="job-error">
        Couldn't read job status — {{ error }}
      </p>
      <dl v-else class="job-facts">
        <div class="job-fact">
          <dt>Last run</dt>
          <dd>
            {{
              stamps?.last_successful_at
                ? formatLocal(stamps.last_successful_at)
                : noRunYetLabel
            }}
          </dd>
        </div>
        <!-- Timestamp only: the failure reason lives in the warning band, which
             has room to wrap. -->
        <div v-if="stamps?.last_failed_at" class="job-fact">
          <dt>Last failure</dt>
          <dd>{{ formatLocal(stamps.last_failed_at) }}</dd>
        </div>
        <div v-if="stamps?.last_cancelled_at" class="job-fact">
          <dt>Last cancelled</dt>
          <dd>{{ formatLocal(stamps.last_cancelled_at) }}</dd>
        </div>
        <div v-if="stamps?.last_skipped_at" class="job-fact">
          <dt>Last skipped</dt>
          <dd>{{ formatLocal(stamps.last_skipped_at) }}</dd>
        </div>
      </dl>
    </div>

    <div class="job-actions">
      <!-- If the UI missed this run's start (a dev reload mid-run), `isRunning` can
           be true with no trace; the handle stays hidden rather than opening an
           empty tracker. -->
      <button
        v-if="isRunning && hasRunLog && !viewingTracker"
        type="button"
        class="btn btn-ghost btn-handle"
        @click="$emit('view-tracker')"
      >
        View progress
      </button>
      <button
        v-else-if="!isRunning && hasRunLog && !viewingTracker"
        type="button"
        class="btn btn-ghost btn-handle"
        @click="$emit('view-tracker')"
      >
        Latest run log
      </button>

      <!-- Hidden while a run is in flight (the bar already says so); disabled
           when the gate blocks a run, the reason living in the warning band. -->
      <button
        v-if="showGenerate && !isRunning"
        type="button"
        class="btn btn-secondary btn-generate"
        :disabled="generating || blocked"
        :title="
          blocked
            ? 'Resolve the configuration warnings above to generate a report'
            : undefined
        "
        @click="$emit('generate')"
      >
        {{ generating ? "Generating…" : "Generate now" }}
      </button>
    </div>
  </footer>
</template>

<style scoped>
/* Left edge aligns with the toolbar and warning rows. */
.job-panel {
  display: flex;
  align-items: center;
  gap: var(--s-6);
  padding: var(--s-3) var(--s-8);
  border-top: var(--border);
  background: var(--paper-soft);
}

.job-status {
  min-width: 0;
  flex: 1;
}

/* The kit's long-job status row (components-status.html). The fill tracks real
   step completion, so its advance is a confirmed state change (the kit's motion
   tier), never a decorative sweep. */
.job-running {
  display: flex;
  flex-direction: column;
  gap: var(--s-2);
  margin: 0;
}

.job-running-row {
  display: flex;
  align-items: center;
  gap: var(--s-4);
}

.job-running-label {
  flex-shrink: 0;
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  color: var(--ink-2);
  white-space: nowrap;
}

.job-running-track {
  position: relative;
  flex: 1;
  height: 1px;
  background: var(--hairline-soft);
  overflow: hidden;
}
.job-running-fill {
  position: absolute;
  left: 0;
  top: 0;
  bottom: 0;
  background: var(--ink);
}
/* Reduced-motion users get an instant jump to the new width. */
@media (prefers-reduced-motion: no-preference) {
  .job-running-fill {
    transition: width var(--dur-fast) var(--ease);
  }
}

/* Mono tabular so the ticking digits don't reflow the row each second. */
.job-running-time {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-variant-numeric: tabular-nums;
  font-size: var(--t-caption);
  color: var(--ink-3);
  white-space: nowrap;
}

/* --track-caption is for 11px uppercase captions only; mixed-case running
   status takes --track-ui. */
.job-running-caption {
  margin: 0;
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-ui);
  color: var(--ink-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.job-error {
  margin: 0;
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  color: var(--accent-text);
  overflow-wrap: anywhere;
}

/* Hover steps one tone deeper: the secondary default hover is paper-soft,
   invisible on this footer. */
.btn-generate {
  flex-shrink: 0;
  padding: var(--s-2) var(--s-4);
  font-size: var(--t-ui-sm);
}

.btn-generate:hover:not(:disabled) {
  background: var(--paper-edge);
}

.job-actions {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: var(--s-3);
}

.btn-handle {
  padding: var(--s-2) var(--s-3);
  font-size: var(--t-ui-sm);
}
.btn-handle:hover {
  background: var(--paper-edge);
}

.job-facts {
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: var(--s-1);
}

.job-fact {
  display: flex;
  align-items: baseline;
  gap: var(--s-3);
  min-width: 0;
}

.job-fact dt {
  flex-shrink: 0;
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  color: var(--ink-3);
}

.job-fact dd {
  margin: 0;
  flex: 1;
  min-width: 0;
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  color: var(--ink-2);
  font-variant-numeric: tabular-nums lining-nums;
  /* Clamp to one line so a long localized timestamp can't balloon the footer. */
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
