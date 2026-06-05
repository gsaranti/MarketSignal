<script setup lang="ts">
import { computed } from "vue";
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
    <div class="warning-head">Needs attention</div>
    <ul class="warning-list">
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
}

/* Accent header is the alert signal — no icon, no saturated red, just the
   system's oxblood used the way it already marks error labels elsewhere. */
.warning-head {
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  font-weight: 600;
  color: var(--accent);
  margin-bottom: var(--s-3);
}

/* One block: the categories are a single list grouped by spacing, with no
   inter-row hairlines (those made one warning area look like three sections). */
.warning-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--s-2);
}

.warning-row {
  display: flex;
  align-items: baseline;
  gap: var(--s-4);
}

.warning-label {
  flex-shrink: 0;
  min-width: 11rem;
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
</style>
