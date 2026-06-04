<script setup lang="ts">
import { computed } from "vue";
import type { JobStatus } from "../types";

// Weekly-job status + controls (docs/scheduling.md §Job Status Visibility,
// §Job Controls). A minimal functional surface for this slice — the warning-area
// redesign and richer status come in the UI/design pass. Fidelity references:
// the boxy Toggle in the design kit's Settings.jsx, the status row in
// components-status.html, and the settings control-row (sans label + serif hint).
const props = defineProps<{
  status: JobStatus | null;
  error: string | null;
  busy?: boolean;
}>();

const emit = defineEmits<{ (e: "set-enabled", value: boolean): void }>();

const enabled = computed(() => props.status?.enabled ?? false);

// Stay silent until the first status resolves (mirrors the warning area), so the
// footer doesn't flash empty on load. Surface as soon as there's status or error.
const visible = computed(() => props.status !== null || props.error !== null);

// The backend persists UTC; render in the viewer's local time (the "show local"
// half of the time-zone decision). Fall back to the raw string if unparseable.
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

function toggle() {
  if (props.busy || !props.status) return;
  emit("set-enabled", !enabled.value);
}
</script>

<template>
  <footer v-if="visible" class="job-panel">
    <div class="job-status" aria-live="polite">
      <p v-if="status?.is_running" class="job-running">
        Generating this week's report…
      </p>
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
        <div v-if="status?.last_failed_at" class="job-fact">
          <dt>Last failure</dt>
          <dd :title="status.last_failure_detail || undefined">
            {{ formatLocal(status.last_failed_at)
            }}<template v-if="status.last_failure_detail">
              — {{ status.last_failure_detail }}</template>
          </dd>
        </div>
        <div v-if="status?.last_skipped_at" class="job-fact">
          <dt>Last skipped</dt>
          <dd>{{ formatLocal(status.last_skipped_at) }}</dd>
        </div>
      </dl>
    </div>

    <div v-if="status" class="job-control">
      <div class="job-control-text">
        <div class="job-control-label">Weekly report</div>
        <div class="job-control-hint">
          {{
            enabled
              ? "Runs automatically every Sunday at 9:00 AM."
              : "Scheduled runs are paused."
          }}
        </div>
      </div>
      <button
        type="button"
        class="switch"
        role="switch"
        :aria-checked="enabled"
        :aria-label="
          enabled ? 'Disable weekly report job' : 'Enable weekly report job'
        "
        :disabled="busy || !status"
        @click="toggle"
      >
        <span class="switch-knob" :class="{ 'switch-knob--on': enabled }"></span>
      </button>
    </div>
  </footer>
</template>

<style scoped>
/* Footer seam aligns its left edge with the toolbar and warning rows
   (var(--s-3) var(--s-8)). */
.job-panel {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--s-6);
  padding: var(--s-3) var(--s-8);
  border-top: var(--border);
  background: var(--paper);
}

.job-status {
  min-width: 0;
  flex: 1;
}

.job-running {
  margin: 0;
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  color: var(--ink-2);
}

.job-error {
  margin: 0;
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  color: var(--accent);
  overflow-wrap: anywhere;
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
  /* A status footer is a compact bar — clamp each value to one line so an
     arbitrarily long detail (e.g. a raw provider error) can never balloon the
     footer's height. The full text is available via the dd's title on hover. */
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.job-control {
  display: flex;
  align-items: center;
  gap: var(--s-5);
  flex-shrink: 0;
}

.job-control-text {
  text-align: right;
}

.job-control-label {
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  font-weight: 500;
  color: var(--ink);
}

.job-control-hint {
  font-family: var(--font-serif);
  font-style: italic;
  font-size: var(--t-caption);
  color: var(--ink-3);
  line-height: var(--lh-ui);
}

/* Boxy switch — mirrors Settings.jsx Toggle: 44×22, 1px ink edge, 2px radius,
   a sliding 18×16 ink block. No pill, no rounded slider. Rendered as a button
   so it is keyboard-operable (Enter/Space) and shows a focus ring. */
.switch {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  width: 44px;
  height: 22px;
  padding: 2px;
  border: 1px solid var(--ink);
  border-radius: var(--radius);
  background: transparent;
  cursor: pointer;
}

.switch-knob {
  width: 18px;
  height: 16px;
  border-radius: var(--radius-sm);
  background: transparent;
  margin-left: 0;
  transition: margin-left var(--dur-fast) var(--ease),
    background-color var(--dur-fast) var(--ease);
}

.switch-knob--on {
  background: var(--ink);
  margin-left: 20px;
}

.switch:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 1px;
}

/* Disabled (no status yet, or a toggle request in flight): inert, muted edge. */
.switch:disabled {
  cursor: not-allowed;
  border-color: var(--hairline);
}

.switch:disabled .switch-knob--on {
  background: var(--ink-3);
}

@media (prefers-reduced-motion: reduce) {
  .switch-knob {
    transition: none;
  }
}
</style>
