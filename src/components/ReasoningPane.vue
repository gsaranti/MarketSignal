<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from "vue";

// A streamed-reasoning pane: the caption plus a bounded, self-scrolling well.
// One primitive for every reasoning stream the tracker shows, so they never
// drift.
//
// Design-system extension (CLAUDE.md §5): the package defines no bounded well
// or scroll-cue treatment. The bound is derived from the well's own type tokens
// so the outer step list scrolls between items rather than through every
// thought; the clipped last line inside the hairline is the scroll cue (a fade
// would be a gradient, which the system rejects) and the OS scrollbar stays
// unstyled. Wheel chaining is left on. No motion, so reduced-motion needs no
// handling.
const props = defineProps<{
  label: string;
  // Distinct per pane: the caption alone would name a long run's panes identically.
  accessibleLabel: string;
  text: string;
}>();

// Auto-follow: pinned to the newest text while the reader is at the bottom,
// never yanked back once they've scrolled up to read.
const well = ref<HTMLElement | null>(null);
const pinned = ref(true);

function onScroll() {
  const el = well.value;
  if (!el) return;
  pinned.value = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
}

watch(
  () => props.text,
  async () => {
    if (!pinned.value) return;
    await nextTick();
    const el = well.value;
    if (el) el.scrollTop = el.scrollHeight;
  }
);

// The tracker remounts when the user returns mid-run, so a fresh mount with
// text starts at the tail rather than jumping there on the next delta.
onMounted(() => {
  const el = well.value;
  if (el) el.scrollTop = el.scrollHeight;
});
</script>

<template>
  <div class="agent-thinking">
    <span class="agent-thinking-label">{{ label }}</span>
    <!-- A named group keeps the well identifiable on keyboard focus without a
         landmark per holding; tabindex makes it keyboard-scrollable. -->
    <pre
      ref="well"
      class="agent-thinking-body"
      role="group"
      :aria-label="accessibleLabel"
      tabindex="0"
      @scroll="onScroll"
      >{{ text }}</pre
    >
  </div>
</template>

<style scoped>
.agent-thinking {
  margin: var(--s-4) 0 0 0;
}
.agent-thinking-label {
  display: block;
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  color: var(--ink-3);
  margin-bottom: var(--s-2);
}
.agent-thinking-body {
  margin: 0;
  padding: var(--s-4) var(--s-5);
  background: var(--paper-edge);
  border: 1px solid var(--hairline-soft);
  border-radius: var(--radius);
  font-family: var(--font-serif);
  font-style: italic;
  font-size: var(--t-ui-sm);
  line-height: var(--lh-ui);
  color: var(--ink-3);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  /* Fourteen lines of the well's own type plus its padding and border
     (border-box app-wide). */
  max-height: calc(14 * var(--lh-ui) * var(--t-ui-sm) + 2 * var(--s-4) + 2px);
  overflow-y: auto;
}
.agent-thinking-body:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}
</style>
