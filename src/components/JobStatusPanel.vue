<script setup lang="ts">
import { computed } from "vue";
import type { JobStatus } from "../types";

// Weekly-job status (docs/scheduling.md §Job Status Visibility). Reports run
// history (last run / last failure / last skipped) and the in-flight indicator;
// the enable/disable control lives in Settings. It also carries the persistent
// manual "Generate now" trigger (the kit's footer division — generation is a job
// action, the report toolbar is reading-only). Recessed chrome on paper-soft.
const props = defineProps<{
  status: JobStatus | null;
  error: string | null;
  blocked: boolean;
  generating: boolean;
  // A run is in flight right now (event-driven, immediate — independent of the
  // periodically-refreshed job status).
  runActive: boolean;
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
        <span class="job-running-label">Generating this week's report…</span>
        <span class="job-running-bar" aria-hidden="true">
          <span class="job-running-fill"></span>
        </span>
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
           tracker — "View progress" during a run, "View run log" for the lingering
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
        View run log
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

/* Long-running-job indicator — text plus a single static 1px bar, per the
   design kit's status row. Deliberately no spinner, no shimmer: the pipeline
   surfaces no step telemetry, so the fill reads as "in progress", not a
   determinate percentage. */
.job-running {
  display: flex;
  align-items: center;
  gap: var(--s-4);
  margin: 0;
}

.job-running-label {
  flex-shrink: 0;
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  color: var(--ink-2);
  white-space: nowrap;
}

.job-running-bar {
  flex: 1;
  height: 1px;
  background: var(--hairline-soft);
  position: relative;
  overflow: hidden;
}

.job-running-fill {
  position: absolute;
  left: 0;
  top: 0;
  bottom: 0;
  width: 38%;
  background: var(--ink);
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
