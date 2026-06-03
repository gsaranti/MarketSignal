<script setup lang="ts">
import { computed } from "vue";
import type { ValidationReport } from "../types";

// Persistent Warning Area (docs/interface.md). The design system's WarningBar
// is the fidelity reference: an always-visible row of active caveats, no icon
// and no color flag — the words are the alert. Each active category renders as
// one row: a sans uppercase label and the serif body listing what is missing.
// When there are no active warnings the area renders nothing.
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
    aria-label="Active configuration warnings"
    aria-live="polite"
  >
    <ul class="warning-list">
      <li v-if="error" class="warning-row">
        <span class="warning-label">Warning</span>
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
.warning-area {
  background: var(--paper);
  border-bottom: var(--border);
}

.warning-list {
  list-style: none;
  margin: 0;
  padding: 0;
}

/* Padding matches the report toolbar (var(--s-3) var(--s-8)) so the warning
   rows and the toolbar below them share one left edge. */
.warning-row {
  display: flex;
  align-items: baseline;
  gap: var(--s-4);
  padding: var(--s-3) var(--s-8);
}

.warning-row + .warning-row {
  border-top: 1px solid var(--hairline-soft);
}

.warning-label {
  flex-shrink: 0;
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  font-weight: 600;
  color: var(--ink);
  white-space: nowrap;
}

.warning-body {
  min-width: 0;
  font-family: var(--font-serif);
  font-size: var(--t-prose-sm);
  line-height: var(--lh-prose);
  letter-spacing: var(--track-prose);
  font-style: italic;
  color: var(--ink);
  overflow-wrap: anywhere;
}
</style>
