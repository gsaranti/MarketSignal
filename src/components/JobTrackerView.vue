<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import Icon from "./Icon.vue";
import type { RunTrace, StepStatus } from "../types";

// Live job run tracker — shown in place of the report pane while a run is in
// flight (and reopenable as a terminal "run log" afterward, latest run only).
//
// DESIGN-SYSTEM EXTENSION: a process/step-list surface is not in the ui_kits.
// It is grounded in existing system idioms rather than invented: the inset-well
// `.prose pre` block (the live agent stream), hairline list rows (the per-series
// requests), the caption/eyebrow scale, sans chrome, and the oxblood accent only
// on interactive states + the running marker. No fills beyond the inset-well
// token, no shadows, no shimmer, no celebratory motion. Flagged per CLAUDE.md §5.
const props = defineProps<{
  trace: RunTrace;
  // Whether the run is still in flight (drives Cancel vs Dismiss + the live region).
  active: boolean;
  // A cancel has been requested but the run hasn't ended yet — disables Cancel and
  // shows the cooperative-stop note.
  cancelRequested: boolean;
}>();

const emit = defineEmits<{
  (e: "cancel"): void;
  (e: "close"): void;
}>();

// The toolbar headline, announced politely to screen readers as it changes.
const headline = computed(() => {
  if (props.active) {
    const running = props.trace.steps.find((s) => s.status === "running");
    return running ? running.label : "Generating report";
  }
  return "Run log";
});

// The terminal tag shown once the run ends (the run-finished status).
const terminal = computed(() => props.trace.terminal);
const terminalLabel = computed(() => {
  switch (terminal.value?.status) {
    case "successful":
      return "Completed";
    case "cancelled":
      return "Cancelled";
    case "failed":
      return "Failed";
    default:
      return terminal.value?.status ?? "";
  }
});
// A non-color signal pairs with the tag tint: completed reads neutral, the two
// stopped outcomes read in accent-text and carry their own word.
const terminalIsAlert = computed(
  () => terminal.value?.status === "failed" || terminal.value?.status === "cancelled"
);

function stepStatusText(status: StepStatus): string {
  switch (status) {
    case "running":
      return "Working…";
    case "failed":
      return "Failed";
    case "cancelled":
      return "Stopped";
    default:
      return ""; // ok / pending carry their meaning through the marker alone
  }
}

// A request row's tone: in-flight, success, a benign non-result (no data for this
// probe), or a failure. Drives both the right-hand indicator and the name color, so
// meaning never rides on color alone (the failure/benign cases also show their word).
type ReqTone = "running" | "ok" | "benign" | "fail";
function reqTone(status: string): ReqTone {
  if (status === "running") return "running";
  if (status === "ok") return "ok";
  if (status === "empty" || status === "out-of-scope") return "benign";
  return "fail";
}

// Auto-follow the stream: keep the view pinned to the newest content while the
// user is at the bottom, but never yank them back if they've scrolled up to read.
const scroller = ref<HTMLElement | null>(null);
const pinned = ref(true);

function onScroll() {
  const el = scroller.value;
  if (!el) return;
  pinned.value = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
}

// A cheap signal that grows as content arrives (request rows + streamed chars +
// step count), so the watcher fires on any new content without deep-watching.
const contentSignature = computed(() => {
  let n = props.trace.steps.length;
  for (const s of props.trace.steps) n += s.requests.length + s.agentText.length;
  if (props.trace.terminal) n += 1;
  return n;
});

watch(contentSignature, async () => {
  if (!pinned.value) return;
  await nextTick();
  const el = scroller.value;
  if (el) el.scrollTop = el.scrollHeight;
});
</script>

<template>
  <main class="tracker-pane">
    <!-- Seam matches the report toolbar (height/padding/divider) so the view
         swap is invisible at the boundary. -->
    <div class="toolbar">
      <div class="toolbar-heading">
        <span class="toolbar-label" aria-live="polite">{{ headline }}</span>
        <span
          v-if="terminal"
          class="toolbar-tag"
          :class="{ 'is-alert': terminalIsAlert }"
          >{{ terminalLabel }}</span
        >
      </div>
      <div class="toolbar-actions">
        <button
          v-if="active"
          type="button"
          class="btn btn-secondary btn-cancel"
          :disabled="cancelRequested"
          @click="emit('cancel')"
        >
          {{ cancelRequested ? "Cancelling…" : "Cancel run" }}
        </button>
        <button
          v-else
          type="button"
          class="btn btn-secondary"
          @click="emit('close')"
        >
          Back to report
        </button>
      </div>
    </div>

    <!-- Keyboard-scrollable region; the live agent text and request rows stream
         in here. role+label+tabindex make it reachable and operable by keyboard. -->
    <div
      ref="scroller"
      class="tracker-scroll"
      role="region"
      aria-label="Report generation progress"
      :aria-busy="active"
      tabindex="0"
      @scroll="onScroll"
    >
      <ol class="step-list">
        <li
          v-for="step in trace.steps"
          :key="step.key"
          class="step"
          :data-status="step.status"
        >
          <div class="step-head">
            <span class="step-marker" :data-status="step.status" aria-hidden="true">
              <Icon v-if="step.status === 'ok'" name="check" :size="13" />
              <Icon
                v-else-if="step.status === 'failed' || step.status === 'cancelled'"
                name="warning"
                :size="13"
              />
              <!-- running / pending use a CSS box (filled vs hollow) -->
            </span>
            <span class="step-label">{{ step.label }}</span>
            <span v-if="stepStatusText(step.status)" class="step-status">{{
              stepStatusText(step.status)
            }}</span>
          </div>

          <!-- Failure / stop reason for a step that didn't complete cleanly. -->
          <p
            v-if="step.detail && (step.status === 'failed' || step.status === 'cancelled')"
            class="step-detail"
          >
            {{ step.detail }}
          </p>

          <!-- One row per actual HTTP request, streamed in-flight then resolved. -->
          <ul v-if="step.requests.length" class="req-list">
            <li
              v-for="(r, i) in step.requests"
              :key="`${r.group}-${r.seriesId}-${i}`"
              class="req"
              :data-tone="reqTone(r.status)"
            >
              <span class="req-provider">{{ r.provider }}</span>
              <span class="req-name" :title="r.name">{{ r.name }}</span>
              <span class="req-status" :data-tone="reqTone(r.status)">
                <Icon
                  v-if="reqTone(r.status) === 'ok'"
                  name="check"
                  :size="12"
                  aria-label="ok"
                />
                <span
                  v-else-if="reqTone(r.status) === 'running'"
                  class="req-dot"
                  role="img"
                  aria-label="in progress"
                ></span>
                <template v-else>{{ r.status }}</template>
              </span>
            </li>
          </ul>

          <!-- The main agent's report text, streamed live (decoded Markdown). -->
          <pre v-if="step.agentText" class="agent-stream">{{ step.agentText }}</pre>
        </li>
      </ol>

      <!-- Edge state: the run has started but no step events have landed yet. -->
      <p v-if="!trace.steps.length" class="tracker-starting">Starting…</p>
    </div>
  </main>
</template>

<style scoped>
.tracker-pane {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
  background: var(--paper);
}

/* Mirrors LatestReportView's toolbar seam exactly so swapping the pane for the
   tracker leaves the boundary unchanged (composition coherence). */
.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 50px;
  padding: var(--s-3) var(--s-8);
  border-bottom: var(--border);
  gap: var(--s-4);
}

.toolbar-heading {
  display: flex;
  align-items: baseline;
  gap: var(--s-3);
  min-width: 0;
}

.toolbar-label {
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  font-weight: 600;
  color: var(--ink);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.toolbar-tag {
  flex-shrink: 0;
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  color: var(--ink-3);
}
.toolbar-tag.is-alert {
  color: var(--accent-text);
}

.toolbar-actions {
  flex-shrink: 0;
  display: flex;
  gap: var(--s-3);
}

.btn-cancel {
  padding: var(--s-2) var(--s-4);
  font-size: var(--t-ui-sm);
}

.tracker-scroll {
  flex: 1;
  overflow-y: auto;
  padding: var(--s-7) var(--s-8) var(--s-10);
}
/* Keyboard focus on the scroll region itself (it's tabbable so it can be
   scrolled by keyboard) — a quiet inset ring, not the button outline. */
.tracker-scroll:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}

.step-list {
  list-style: none;
  margin: 0 auto;
  padding: 0;
  max-width: var(--measure-wide);
}

.step {
  padding: 0 0 var(--s-6) 0;
  /* A vertical spine connects the steps; the marker sits on it. */
  border-left: 1px solid var(--hairline-soft);
  padding-left: var(--s-5);
  margin-left: 6px;
}
.step:last-child {
  padding-bottom: 0;
}

.step-head {
  display: flex;
  align-items: baseline;
  gap: var(--s-3);
  /* Pull the marker back onto the spine. */
  margin-left: calc(-1 * var(--s-5) - 7px);
}

.step-marker {
  flex-shrink: 0;
  width: 14px;
  height: 14px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: var(--paper);
  color: var(--ink);
  position: relative;
  top: 2px;
}
/* running / pending markers are CSS boxes: a filled accent box vs a hollow
   hairline box. Shape + position carry meaning, not color alone. */
.step-marker[data-status="running"]::before,
.step-marker[data-status="pending"]::before {
  content: "";
  width: 8px;
  height: 8px;
  border-radius: var(--radius-sm);
}
.step-marker[data-status="running"]::before {
  background: var(--accent);
}
.step-marker[data-status="pending"]::before {
  border: 1px solid var(--hairline);
}
.step-marker[data-status="failed"],
.step-marker[data-status="cancelled"] {
  color: var(--accent-text);
}

.step-label {
  font-family: var(--font-sans);
  font-size: var(--t-ui);
  font-weight: 600;
  color: var(--ink);
  min-width: 0;
  overflow-wrap: anywhere;
}
.step[data-status="pending"] .step-label {
  color: var(--ink-3);
  font-weight: 500;
}

.step-status {
  margin-left: auto;
  flex-shrink: 0;
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  color: var(--ink-3);
}
.step[data-status="failed"] .step-status,
.step[data-status="cancelled"] .step-status {
  color: var(--accent-text);
}

.step-detail {
  margin: var(--s-3) 0 0 0;
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  line-height: var(--lh-ui);
  color: var(--ink-2);
  overflow-wrap: anywhere;
}

/* Per-series request rows under the baseline step. */
.req-list {
  list-style: none;
  margin: var(--s-4) 0 0 0;
  padding: 0;
  border-top: 1px solid var(--hairline-soft);
}
.req {
  display: flex;
  align-items: baseline;
  gap: var(--s-4);
  padding: var(--s-2) 0;
  border-bottom: 1px solid var(--hairline-soft);
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
}
.req:last-child {
  border-bottom: 0;
}
.req-provider {
  flex-shrink: 0;
  width: 9ch;
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  color: var(--ink-3);
  font-variant-numeric: tabular-nums;
}
.req-name {
  flex: 1;
  min-width: 0;
  color: var(--ink);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
/* The name dims for anything that isn't a clean success. */
.req:not([data-tone="ok"]) .req-name {
  color: var(--ink-2);
}

.req-status {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
}
/* Success and benign non-results (no data for a probe) read muted, not alarming;
   only true failures take the accent text alongside their reason word. */
.req-status[data-tone="ok"],
.req-status[data-tone="benign"] {
  color: var(--ink-3);
}
.req-status[data-tone="fail"] {
  color: var(--accent-text);
}

/* In-flight indicator: a small accent square, matching the step running marker and
   the system's "accent marks the active item" convention. Static — the rows
   streaming in are the activity signal (no shimmer, per the design system). */
.req-dot {
  width: 6px;
  height: 6px;
  border-radius: var(--radius-sm);
  background: var(--accent);
}

/* The main agent's streamed report text — the system's inset-well code block
   idiom (mono on paper-edge, hairline border), here growing as the model writes
   and softly wrapping its Markdown source. */
.agent-stream {
  margin: var(--s-4) 0 0 0;
  padding: var(--s-4) var(--s-5);
  background: var(--paper-edge);
  border: 1px solid var(--hairline);
  border-radius: var(--radius);
  font-family: var(--font-mono);
  font-size: var(--t-ui-sm);
  line-height: var(--lh-ui);
  color: var(--ink-2);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  /* No inner scroll: the block grows with the streamed text and the outer
     tracker scroll auto-follows to the newest tokens, so the latest writing is
     always in view (the user can scroll up to re-read). */
}

.tracker-starting {
  margin: 0;
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  color: var(--ink-3);
}
</style>
