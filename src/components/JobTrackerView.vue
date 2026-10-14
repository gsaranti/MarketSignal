<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import Icon from "./Icon.vue";
import ReasoningPane from "./ReasoningPane.vue";
import type { PortfolioResumeStatus, RunTrace, StepStatus, TrackerRequest } from "../types";

// Live job run tracker, shown in place of the report pane while a run is in
// flight and reopenable as the latest run's log afterward.
//
// DESIGN-SYSTEM EXTENSION (CLAUDE.md §5): a process/step-list surface is not in
// the ui_kits. It is grounded in existing idioms (the inset-well `.prose pre`
// block, hairline list rows, the caption/eyebrow scale, the accent only on
// interactive states and the running marker): no fills beyond the inset-well
// token, no shadows, no shimmer, no celebratory motion.
//
// LIVENESS BREATHE: the running step marker and the in-flight request dot carry
// a slow opacity breathe so the run reads as alive while a long step holds the
// footer's fill steady. An approved departure from the system's no-idle-motion
// stance for these two elements only, gated by prefers-reduced-motion.
//
// REQUEST-ROW SUBJECT: a research row adds a SEARCH / FETCH token and its query
// or page address, and a completed row its short outcome note. The subject is a
// ghost-button disclosure revealing the full text, so no hover is required to
// read clipped text (the desktop kit README's request subject disclosure
// extension).
const props = withDefaults(
  defineProps<{
    trace: RunTrace;
    active: boolean;
    cancelRequested: boolean;
    // Which page owns the run (docs/run-tracking.md); keys the surface labels.
    kind?: "report" | "portfolio" | "portfolio-quick-check";
    // Supplied only for a Portfolio run's terminal state
    // (docs/portfolio-analysis.md §Failure posture); `null` = no checkpoint
    // trail, nothing renders.
    resume?: PortfolioResumeStatus | null;
    // Disables the button so a double press cannot race the single run slot.
    resumeRequested?: boolean;
  }>(),
  { kind: "report", resume: null, resumeRequested: false }
);

const emit = defineEmits<{
  (e: "cancel"): void;
  (e: "close"): void;
  (e: "resume"): void;
}>();

// Keyed by row identity: repeated requests can share a query or URL.
const expandedTargets = ref(new Set<TrackerRequest>());
watch(() => props.trace.runId, () => expandedTargets.value.clear());
function toggleTarget(row: TrackerRequest) {
  if (!expandedTargets.value.delete(row)) expandedTargets.value.add(row);
}
function targetDisclosureLabel(row: TrackerRequest): string {
  const subject = row.target?.kind === "search" ? "search query" : "fetch URL";
  return `${expandedTargets.value.has(row) ? "Hide" : "Show"} full ${subject}: ${row.target?.text ?? ""}`;
}

// docs/run-tracking.md: a Portfolio run's tracker must never announce itself
// as report generation.
const idleHeadline = computed(() => {
  switch (props.kind) {
    case "portfolio":
      return "Analyzing portfolio";
    case "portfolio-quick-check":
      return "Running quick check";
    default:
      return "Generating report";
  }
});
const backLabel = computed(() =>
  props.kind === "report" ? "Back to report" : "Back to portfolio"
);
const progressRegionLabel = computed(() => {
  switch (props.kind) {
    case "portfolio":
      return "Portfolio analysis progress";
    case "portfolio-quick-check":
      return "Quick check progress";
    default:
      return "Report generation progress";
  }
});

const headline = computed(() => {
  if (props.active) {
    const running = props.trace.steps.find((s) => s.status === "running");
    return running ? running.label : idleHeadline.value;
  }
  return "Run log";
});

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
// Pairs the tag tint with its word, so the outcome never rides on color alone.
const terminalIsAlert = computed(
  () => terminal.value?.status === "failed" || terminal.value?.status === "cancelled"
);

// docs/portfolio-analysis.md §Failure posture. The probe returns whichever
// trail exists, not necessarily this run's (a run failing before it opens its
// own trail leaves an earlier one standing), so the note identifies the trail
// by its own as-of, never claiming it belongs to the run shown.
const resumeContext = computed(
  () =>
    !props.active &&
    props.kind === "portfolio" &&
    terminalIsAlert.value &&
    props.resume != null
);
const resumeOffered = computed(
  () => resumeContext.value && props.resume?.available === true
);
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
const resumeNote = computed(() => {
  if (!resumeContext.value || !props.resume) return "";
  if (props.resume.available) {
    const { completed, total, createdAt } = props.resume;
    const asOf = createdAt ? ` from ${formatLocal(createdAt)}` : "";
    return (
      `An interrupted run${asOf} can resume: ${completed} of ${total} holdings completed. ` +
      "Resume keeps them and its pinned holdings pull and run-level context, and analyzes the rest."
    );
  }
  return props.resume.reason ? `Resume unavailable: ${props.resume.reason}.` : "";
});

function stepStatusText(status: StepStatus): string {
  switch (status) {
    case "running":
      return "Working…";
    case "failed":
      return "Failed";
    case "cancelled":
      return "Stopped";
    // The quick check's outcomes, named in text so neither leans on the marker
    // or its color alone.
    case "flagged":
      return "Flagged";
    case "unknown":
      return "Unknown";
    default:
      return ""; // ok / pending carry their meaning through the marker alone
  }
}

// `benign` is a non-result (no data for this probe), not a failure. The tone
// drives the indicator and the name color; failure and benign also show their
// word, so meaning never rides on color alone.
type ReqTone = "running" | "ok" | "benign" | "fail";
function reqTone(status: string): ReqTone {
  if (status === "running") return "running";
  if (status === "ok") return "ok";
  if (status === "empty" || status === "out-of-scope") return "benign";
  return "fail";
}

// Keyed by the posture the backend tags each chunk with.
const ANALYST_ORDER = ["bull", "bear", "balanced"] as const;
const ANALYST_LABELS: Record<string, string> = {
  bull: "Bull",
  bear: "Bear",
  balanced: "Balanced",
};
// An unrecognized posture still renders, labeled by its raw key.
function analystPanes(thinking: Record<string, string> | undefined): { posture: string; label: string; text: string }[] {
  if (!thinking) return [];
  const known = ANALYST_ORDER.filter((p) => thinking[p]);
  const extra = Object.keys(thinking).filter(
    (p) => thinking[p] && !ANALYST_ORDER.includes(p as (typeof ANALYST_ORDER)[number])
  );
  return [...known, ...extra].map((posture) => ({
    posture,
    label: ANALYST_LABELS[posture] ?? posture,
    text: thinking[posture],
  }));
}

// Auto-follow: pinned to the newest content while the user is at the bottom,
// never yanked back once they've scrolled up to read.
const scroller = ref<HTMLElement | null>(null);
const pinned = ref(true);

function onScroll() {
  const el = scroller.value;
  if (!el) return;
  pinned.value = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
}

// Grows as content arrives, so the watcher fires on new content without
// deep-watching.
const contentSignature = computed(() => {
  let n = props.trace.steps.length + props.trace.unattributed.length;
  for (const s of props.trace.steps) {
    n += s.requests.length + s.agentText.length + s.agentThinking.length;
    for (const k in s.analystThinking) n += s.analystThinking[k].length;
  }
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
    <!-- Seam matches the report toolbar so the view swap is invisible at the
         boundary. -->
    <div class="toolbar">
      <!-- aria-live on the heading, not just the label, so the terminal tag
           inserted on completion is announced with the headline change. -->
      <div class="toolbar-heading" aria-live="polite">
        <span class="toolbar-label">{{ headline }}</span>
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
        <template v-else>
          <button
            v-if="resumeOffered"
            type="button"
            class="btn btn-secondary"
            :disabled="resumeRequested"
            @click="emit('resume')"
          >
            {{ resumeRequested ? "Resuming…" : "Resume run" }}
          </button>
          <button type="button" class="btn btn-secondary" @click="emit('close')">
            {{ backLabel }}
          </button>
        </template>
      </div>
    </div>
    <p v-if="resumeNote" class="resume-note" aria-live="polite">
      {{ resumeNote }}
    </p>

    <!-- role + label + tabindex make the scroll region reachable and operable by
         keyboard. -->
    <div
      ref="scroller"
      class="tracker-scroll"
      role="region"
      :aria-label="progressRegionLabel"
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
                v-else-if="
                  step.status === 'failed' ||
                  step.status === 'cancelled' ||
                  step.status === 'flagged'
                "
                name="warning"
                :size="13"
              />
              <!-- running / pending / unknown use a CSS box; the status text
                   carries unknown's meaning, never the marker alone -->
            </span>
            <span class="step-label">{{ step.label }}</span>
            <span v-if="stepStatusText(step.status)" class="step-status">{{
              stepStatusText(step.status)
            }}</span>
            <!-- ok / pending have only the aria-hidden marker; give a screen
                 reader the word. -->
            <span v-else class="sr-only">{{
              step.status === "ok" ? "Completed" : "Not started"
            }}</span>
          </div>

          <p
            v-if="step.detail && (step.status === 'failed' || step.status === 'cancelled')"
            class="step-detail"
          >
            {{ step.detail }}
          </p>

          <ul v-if="step.requests.length" class="req-list">
            <li
              v-for="(r, i) in step.requests"
              :key="`${r.group}-${r.seriesId}-${i}`"
              class="req"
              :data-tone="reqTone(r.status)"
            >
              <span class="req-provider">{{ r.provider }}</span>
              <span class="req-name" :title="r.name">{{ r.name }}</span>
              <button
                v-if="r.target"
                type="button"
                class="btn btn-ghost req-target"
                :title="r.target.text"
                :aria-label="targetDisclosureLabel(r)"
                :aria-expanded="expandedTargets.has(r)"
                @click="toggleTarget(r)"
              >
                <span class="req-target-chevron" aria-hidden="true">{{ expandedTargets.has(r) ? "▾" : "▸" }}</span>
                <span class="req-target-kind">{{ r.target.kind }}</span>
                <span class="req-target-text">{{ r.target.text }}</span>
              </button>
              <span
                v-if="reqTone(r.status) === 'ok' && r.detail"
                class="req-note"
                :title="r.detail"
              >
                {{ r.detail }}
              </span>
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
              <span v-if="r.target && expandedTargets.has(r)" class="req-target-full">{{ r.target.text }}</span>
              <span
                v-if="reqTone(r.status) === 'fail' && r.detail"
                class="req-detail"
                :title="r.detail"
              >
                {{ r.detail }}
              </span>
            </li>
          </ul>

          <!-- Thoughts only: the review body never streams. Absent for models that
               don't surface thinking. -->
          <ReasoningPane
            v-for="pane in analystPanes(step.analystThinking)"
            :key="pane.posture"
            :label="`${pane.label} · Reasoning`"
            :accessible-label="`${pane.label} reasoning`"
            :text="pane.text"
          />

          <!-- The accessible name carries the step label so a long run's panes
               don't all read "Reasoning". -->
          <ReasoningPane
            v-if="step.agentThinking"
            label="Reasoning"
            :accessible-label="`${step.label} reasoning`"
            :text="step.agentThinking"
          />

          <pre v-if="step.agentText" class="agent-stream">{{ step.agentText }}</pre>
        </li>
      </ol>

      <!-- Requests with no owning step, never synthesized into a step so an
           unowned row can't read as a failed stage. Absent in every
           well-bracketed run. -->
      <section
        v-if="trace.unattributed.length"
        class="unattributed"
        aria-label="Requests outside any step"
      >
        <span class="unattributed-label">Requests outside any step</span>
        <ul class="req-list">
          <li
            v-for="(r, i) in trace.unattributed"
            :key="`${r.group}-${r.seriesId}-${i}`"
            class="req"
            :data-tone="reqTone(r.status)"
          >
            <span class="req-provider">{{ r.provider }}</span>
            <span class="req-name" :title="r.name">{{ r.name }}</span>
            <button
              v-if="r.target"
              type="button"
              class="btn btn-ghost req-target"
              :title="r.target.text"
              :aria-label="targetDisclosureLabel(r)"
              :aria-expanded="expandedTargets.has(r)"
              @click="toggleTarget(r)"
            >
              <span class="req-target-chevron" aria-hidden="true">{{ expandedTargets.has(r) ? "▾" : "▸" }}</span>
              <span class="req-target-kind">{{ r.target.kind }}</span>
              <span class="req-target-text">{{ r.target.text }}</span>
            </button>
            <span
              v-if="reqTone(r.status) === 'ok' && r.detail"
              class="req-note"
              :title="r.detail"
            >
              {{ r.detail }}
            </span>
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
            <span v-if="r.target && expandedTargets.has(r)" class="req-target-full">{{ r.target.text }}</span>
            <span
              v-if="reqTone(r.status) === 'fail' && r.detail"
              class="req-detail"
              :title="r.detail"
            >
              {{ r.detail }}
            </span>
          </li>
        </ul>
      </section>

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

/* Mirrors LatestReportView's toolbar seam so swapping the pane leaves the
   boundary unchanged. */
.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 50px;
  padding: var(--s-3) var(--s-8);
  border-bottom: var(--border);
  gap: var(--s-4);
}

.resume-note {
  margin: 0;
  padding: var(--s-3) var(--s-8);
  border-bottom: var(--border);
  font-size: var(--t-ui-sm);
  color: var(--ink-2);
}

.toolbar-heading {
  display: flex;
  align-items: baseline;
  gap: var(--s-3);
  min-width: 0;
}

/* Dynamic content, so sentence case at neutral tracking; caps + --track-caption
   are reserved for static surface titles (colors_and_type.css). */
.toolbar-label {
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  font-weight: 600;
  color: var(--ink);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  margin: -1px;
  overflow: hidden;
  clip: rect(0 0 0 0);
  white-space: nowrap;
  border: 0;
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
  /* Positioning context for the `.sr-only` spans: absolutely positioned with
     <html> as their containing block, each would extend the document
     scrollHeight and produce a second, page-level scrollbar. */
  position: relative;
}
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
/* Filled vs hollow: shape carries the meaning, not color alone. */
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
/* Flagged reuses the grade-D amber text token (the Portfolio attention-flag
   treatment); unknown is a hollow ink box, its meaning in the status text. */
.step-marker[data-status="flagged"] {
  color: var(--grade-d-tx);
}
.step-marker[data-status="unknown"]::before {
  content: "";
  width: 8px;
  height: 8px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--ink-3);
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
.step[data-status="flagged"] .step-status {
  color: var(--grade-d-tx);
}

.step-detail {
  margin: var(--s-3) 0 0 0;
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  line-height: var(--lh-ui);
  color: var(--ink-2);
  overflow-wrap: anywhere;
}

.req-list {
  list-style: none;
  margin: var(--s-4) 0 0 0;
  padding: 0;
  border-top: 1px solid var(--hairline-soft);
}
.req {
  display: flex;
  flex-wrap: wrap;
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
.req-status[data-tone="ok"],
.req-status[data-tone="benign"] {
  color: var(--ink-3);
}
.req-status[data-tone="fail"] {
  color: var(--accent-text);
}

/* A research row is an explicit five-column grid (provider, topic, subject,
   note, status) rather than the wrapping flex row, so at any width the status
   marker holds the right edge and nothing wraps to the left edge. The provider
   track is `auto` so the span's own 9ch sizes it, keeping the topic flush with
   the plain rows' names. Under pressure the subject keeps a 12ch floor, the
   topic (20ch fits every agenda key) and the note share what remains, and the
   subject takes everything past that, so a note clips before a subject falls
   below its floor. */
.req:has(.req-target) {
  display: grid;
  grid-template-columns: auto minmax(0, 20ch) minmax(12ch, 1fr) minmax(0, max-content) auto;
}
.req:has(.req-target) .req-name {
  grid-column: 2;
}
.req:has(.req-target) .req-target {
  grid-column: 3;
}
/* A `.btn-ghost` stripped to the row's metrics: the kit button's transparent
   1px border would add 2px to the row and break the hairline list's rhythm. */
.req-target {
  min-width: 0;
  display: inline-flex;
  align-items: baseline;
  gap: var(--s-2);
  overflow: hidden;
  justify-content: flex-start;
  padding: 0;
  border: 0;
  font: inherit;
  text-align: left;
}
.req-target-chevron {
  flex-shrink: 0;
}
/* Starts at the topic column, never under the provider column. */
.req-target-full {
  grid-column: 2 / -1;
  min-width: 0;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  color: var(--ink-2);
}
.req-target-kind {
  flex-shrink: 0;
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  color: var(--ink-3);
}
.req-target-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--ink-2);
}
.req:has(.req-target) .req-status {
  grid-column: 5;
  justify-self: end;
}
.req:has(.req-target) .req-detail {
  grid-column: 1 / -1;
}

/* The max-width bounds the note track's max-content so a long note can never
   push the marker off the row. The column placement is scoped to the
   research-row grid: a plain flex row that ever resolves ok with a detail must
   not carry a stray grid-column. */
.req:has(.req-target) .req-note {
  grid-column: 4;
}
.req-note {
  min-width: 0;
  max-width: 32ch;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--t-caption);
  color: var(--ink-3);
  font-variant-numeric: tabular-nums;
}

/* Clamped to two lines: a long HTML body head must not swallow the list. */
.req-detail {
  flex-basis: 100%;
  min-width: 0;
  padding-left: calc(9ch + var(--s-4));
  font-size: var(--t-caption);
  color: var(--ink-3);
  overflow: hidden;
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  line-clamp: 2;
}

/* Carries the liveness breathe (see the header note). */
.req-dot {
  width: 6px;
  height: 6px;
  border-radius: var(--radius-sm);
  background: var(--accent);
}

/* Gated so reduced-motion users see a steady, full-opacity marker. */
@media (prefers-reduced-motion: no-preference) {
  @keyframes ms-breathe {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.45;
    }
  }
  .step-marker[data-status="running"]::before,
  .req-dot {
    animation: ms-breathe 1.6s ease-in-out infinite;
  }
}

/* Deliberately unbounded, unlike the reasoning wells: it is the deliverable and
   there is one per run; the outer tracker scroll auto-follows the newest tokens. */
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
}

.unattributed {
  margin: var(--s-6) auto 0;
  max-width: var(--measure-wide);
  display: block;
}
.unattributed-label {
  display: block;
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  color: var(--ink-3);
  margin-bottom: var(--s-2);
}

.tracker-starting {
  margin: 0;
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  color: var(--ink-3);
}
</style>
