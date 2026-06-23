<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from "vue";
import type { JobStatus } from "../types";

// Job status (docs/scheduling.md §Job Status Visibility). Reports run history
// (last run / last failure / last skipped / last cancelled) and the in-flight
// indicator. It also carries the persistent manual "Generate now" trigger (the
// kit's footer division — generation is a job action, the report toolbar is
// reading-only). Recessed chrome on paper-soft.
const props = defineProps<{
  status: JobStatus | null;
  error: string | null;
  blocked: boolean;
  generating: boolean;
  // A run is in flight right now (event-driven, immediate — independent of the
  // periodically-refreshed job status).
  runActive: boolean;
  // Determinate run progress (how far through the fixed pipeline), driving the
  // status row's 1px fill and "step N of T" caption. Null when no run is in flight.
  progress: { fraction: number; stepNumber: number; total: number; label: string } | null;
  // Wall-clock start of the in-flight run (epoch ms) for the live elapsed timer.
  runStartedAt: number | null;
  // A latest-run trace exists in this session (terminal or not), so the tracker is
  // reopenable.
  hasRunLog: boolean;
  // The tracker is currently the active pane, so the handle would be a no-op.
  viewingTracker: boolean;
}>();

defineEmits<{ (e: "generate"): void; (e: "view-tracker"): void }>();

// Whether a run is in flight: prefer the immediate event-driven flag, falling back
// to the backend guard state.
const isRunning = computed(() => props.runActive || props.status?.is_running === true);

// Stay silent until the first status resolves (mirrors the warning area), so the
// footer doesn't flash empty on load. Surface as soon as there's status, an error,
// a live run, or a reopenable run log.
const visible = computed(
  () =>
    props.status !== null ||
    props.error !== null ||
    props.runActive ||
    props.hasRunLog
);

// The determinate fill width, clamped to [0, 100]%. The width *advancing* is the
// status row's primary motion — a confirmed state change as each step completes.
const progressPct = computed(() => {
  const f = props.progress?.fraction ?? 0;
  return `${Math.round(Math.max(0, Math.min(1, f)) * 100)}%`;
});

// Live elapsed timer — a ticking mono clock that proves the run isn't frozen even
// while a single long step (e.g. the up-to-30-min research window) holds the fill
// steady. It's honest data changing, not a decorative sweep, so it stays on-system.
// A 1s tick runs only while a run is in flight; `aria-hidden` keeps it from spamming
// the polite live region (screen readers get the label + step caption instead).
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
  // `== null` (not `===`) so a missing/undefined value is treated as "no start time"
  // and the timer is hidden, rather than computing against NaN.
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

// The backend persists UTC; render in the viewer's local time. Fall back to the
// raw string if unparseable.
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
          <span class="job-running-label">Generating report…</span>
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
              status?.last_successful_at
                ? formatLocal(status.last_successful_at)
                : "No report has run yet"
            }}
          </dd>
        </div>
        <!-- Timestamp only — the full failure reason is in the warning band
             above, which has room to wrap (the footer always truncated it). -->
        <div v-if="status?.last_failed_at" class="job-fact">
          <dt>Last failure</dt>
          <dd>{{ formatLocal(status.last_failed_at) }}</dd>
        </div>
        <div v-if="status?.last_cancelled_at" class="job-fact">
          <dt>Last cancelled</dt>
          <dd>{{ formatLocal(status.last_cancelled_at) }}</dd>
        </div>
        <div v-if="status?.last_skipped_at" class="job-fact">
          <dt>Last skipped</dt>
          <dd>{{ formatLocal(status.last_skipped_at) }}</dd>
        </div>
      </dl>
    </div>

    <div class="job-actions">
      <!-- The footer is the run's home: this handle opens (or returns to) the
           tracker — "View progress" during a run, "Latest run log" for the lingering
           terminal trace afterward. Hidden when the tracker is already showing. -->
      <!-- Only offered when a trace actually exists to show. If the UI missed this
           run's start (e.g. a dev reload mid-run), `isRunning` may be true with no
           trace; the handle stays hidden rather than opening an empty tracker. -->
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

      <!-- Persistent manual trigger. Hidden while a run is in flight (the bar to
           the left already says so). Disabled when the gate blocks a run; the
           reason lives in the warning band above and the report empty state. -->
      <button
        v-if="!isRunning"
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
/* Footer seam aligns its left edge with the toolbar and warning rows
   (var(--s-3) var(--s-8)); recessed onto paper-soft like the sidebar. */
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

/* Long-running-job indicator — the design kit's long-job status row
   (components-status.html): a label, a 1px track carrying a determinate --ink fill,
   a mono elapsed time, and a "step N of T" caption below. The fill's width tracks
   real step completion, so its advance is a confirmed state change (the kit's motion
   tier), never a decorative sweep — staying on-system while finally moving. */
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
/* The width change is a state-change confirmation (the kit's 120ms tier). Reduced-
   motion users get an instant jump to the new width — the progress still shows,
   just without the tween. */
@media (prefers-reduced-motion: no-preference) {
  .job-running-fill {
    transition: width 120ms ease-out;
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

.job-running-caption {
  margin: 0;
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
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

/* Compact secondary button sized to the footer's tight chrome. Hover steps one
   tonal step deeper than the paper-soft footer (the secondary default hover is
   paper-soft, which would be invisible here) — matching the sidebar's tinted-
   region pattern. */
.btn-generate {
  flex-shrink: 0;
  padding: var(--s-2) var(--s-4);
  font-size: var(--t-ui-sm);
}

.btn-generate:hover:not(:disabled) {
  background: var(--paper-edge);
}

/* The run-handle + generate buttons share the footer's right edge. */
.job-actions {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: var(--s-3);
}

/* Quiet ghost handle that opens the tracker — recedes next to the generate
   button and the facts. Sized to the footer's tight chrome. */
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
