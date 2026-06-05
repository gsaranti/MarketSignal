<script setup lang="ts">
import { computed, ref, watch } from "vue";
import Icon from "./Icon.vue";
import type { ValidationReport } from "../types";

// Persistent Warning Area (docs/interface.md). Deliberately rendered in the
// chrome/status register — sans type on an inset-well (paper-edge) band with an
// accent header — NOT the serif reading register, so it can never be mistaken
// for report content. This extends the design kit's WarningBar: the kit's "plain
// serif prose, no header" treatment was indistinguishable from the report body
// (the reading surface uses the same serif), so a coding-agent review promoted it
// to a labelled status band. Each active category is one line inside one block.
const props = defineProps<{
  report: ValidationReport | null;
  error: string | null;
}>();

// Only the backend's non-empty categories are ever present in the report.
const categories = computed(() => props.report?.categories ?? []);

// Show the area when there is something to say. While the first check is still
// loading and nothing is known yet, stay silent rather than flash a row.
const visible = computed(
  () => props.error !== null || categories.value.length > 0
);

// Collapse state (session-scoped — the component stays mounted across view
// switches, so this persists until the app restarts).
const collapsed = ref(false);

// One row per active category, plus the config-check error row when present.
const issueCount = computed(() => (props.error ? 1 : 0) + categories.value.length);

// A signature of which warnings are present, so we can re-expand when a NEW one
// appears even if the user had collapsed the band.
const signature = computed(
  () => (props.error ? "err," : "") + categories.value.map((c) => c.kind).join(",")
);
watch(signature, (now, before) => {
  const had = new Set((before ?? "").split(",").filter(Boolean));
  const appeared = now.split(",").filter(Boolean).some((k) => !had.has(k));
  if (appeared) collapsed.value = false;
});

function formatItems(items: string[]): string {
  return items.join("; ");
}
</script>

<template>
  <section
    v-if="visible"
    class="warning-area"
    aria-label="Needs attention"
    aria-live="polite"
  >
    <button
      type="button"
      class="warning-toggle"
      :aria-expanded="!collapsed"
      aria-controls="warning-list"
      @click="collapsed = !collapsed"
    >
      <span class="warning-head">Needs attention</span>
      <span v-if="collapsed" class="warning-count">
        · {{ issueCount }} {{ issueCount === 1 ? "issue" : "issues" }}
      </span>
      <Icon
        name="chevron_d"
        :size="14"
        color="var(--accent)"
        class="warning-chevron"
        :class="{ 'is-open': !collapsed }"
      />
    </button>
    <ul v-show="!collapsed" id="warning-list" class="warning-list">
      <li v-if="error" class="warning-row">
        <span class="warning-label">Configuration</span>
        <span class="warning-body">
          Couldn't check configuration — {{ error }}
        </span>
      </li>
      <li v-for="cat in categories" :key="cat.kind" class="warning-row">
        <span class="warning-label">{{ cat.title }}</span>
        <span class="warning-body">{{ formatItems(cat.items) }}</span>
      </li>
    </ul>
  </section>
</template>

<style scoped>
/* An inset-well band (one tonal step below the paper reading surface) so the
   warning area reads as system status, not report content. Left padding matches
   the toolbar/footer (var(--s-8)) so the left edge still aligns across regions. */
.warning-area {
  background: var(--paper-edge);
  border-bottom: var(--border);
  padding: var(--s-4) var(--s-8);
  /* Query container so rows can adapt to the content column's width (viewport
     minus the fixed sidebar), not the viewport. */
  container-type: inline-size;
}

/* The header doubles as the collapse control: full-width row, head on the left,
   disclosure chevron pushed to the right. A <button> so it is keyboard-operable. */
.warning-toggle {
  display: flex;
  align-items: center;
  gap: var(--s-2);
  width: 100%;
  padding: 0;
  border: 0;
  background: transparent;
  cursor: pointer;
  text-align: left;
}

.warning-toggle:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}

/* Accent header is the alert signal — no saturated red, just the system's
   oxblood used the way it already marks error labels elsewhere. Sized to the
   strengthened surface-title scale (13px) so it reads as a region heading. */
.warning-head {
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  font-weight: 600;
  color: var(--accent);
}

/* Count summary shown only when collapsed. */
.warning-count {
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  color: var(--ink-2);
}

/* Chevron points down when collapsed, flips up when open. */
.warning-chevron {
  margin-left: auto;
  transition: transform var(--dur-fast) var(--ease);
}

.warning-chevron.is-open {
  transform: rotate(180deg);
}

/* One block: the categories are a single list grouped by spacing, with no
   inter-row hairlines (those made one warning area look like three sections). */
.warning-list {
  list-style: none;
  margin: var(--s-3) 0 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--s-2);
}

/* Default to a stacked label-over-body row: this never crushes the body, and it
   is the graceful-degradation layout if container queries are unsupported. */
.warning-row {
  display: flex;
  flex-direction: column;
  gap: var(--s-1);
}

/* Wide enough: lay the whole list out as a two-column grid so every body shares
   one left edge regardless of label length (column 1 auto-sizes to the widest
   label). display:contents lets each row's label + body join the list grid. */
@container (min-width: 32rem) {
  .warning-list {
    display: grid;
    grid-template-columns: max-content 1fr;
    align-items: baseline;
    column-gap: var(--s-5);
    row-gap: var(--s-2);
  }
  .warning-row {
    display: contents;
  }
}

.warning-label {
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  font-weight: 600;
  color: var(--ink-2);
}

/* Sans, not serif-italic: keeps the status text out of the reading register. */
.warning-body {
  min-width: 0;
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  line-height: var(--lh-ui);
  color: var(--ink);
  overflow-wrap: anywhere;
}

@media (prefers-reduced-motion: reduce) {
  .warning-chevron {
    transition: none;
  }
}
</style>
