<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from "vue";

// A streamed-reasoning pane: the caption plus the bounded, self-scrolling well.
// One primitive for every reasoning stream the tracker shows — the main agent's,
// each analyst's, and a local job's per-item step — so the three never drift.
//
// DESIGN-SYSTEM EXTENSION (per CLAUDE.md §5; same family as JobTrackerView's
// tracker note): the well is the system's quiet serif-italic "aside" voice on the
// paper-edge inset surface, in --ink-3 with a softer hairline so it reads as
// secondary to the deliverable, and it is BOUNDED — a max height derived from its
// own type tokens (fourteen ui-sm lines plus the well's padding and border) with
// its own vertical scroller, so a long run's earlier steps stay compact and the
// outer step list scrolls between items rather than through every thought
// (ruled 2026-09-15: all reasoning panes, the report-text console left
// unbounded). The package defines no scroll-cue treatment; the clipped last line
// inside the hairline well is the cue — a fade would be a gradient, which the
// system rejects, and the OS scrollbar is left unstyled. Wheel chaining is left
// on: a gesture that starts on the pane scrolls it and continues into the outer
// list at its end. Static, like the rest of the tracker (no motion), so
// reduced-motion needs no special handling.
const props = defineProps<{
  // The visible caption above the well ("Reasoning", "Bull · Reasoning").
  label: string;
  // The well's accessible name ("Bull reasoning", "Analyze TSLA
  // reasoning") — the caption alone would name a long run's panes identically.
  accessibleLabel: string;
  // The streamed thinking so far.
  text: string;
}>();

// Auto-follow: keep the well pinned to the newest text while the reader is at
// the bottom, but never yank them back once they've scrolled up to read. The
// same rule the tracker's outer region applies, here per pane.
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

// A fresh mount with text already present starts at the tail too: the tracker
// remounts when the user returns to it mid-run, and the pane must not open on
// its first lines only to jump on the next delta.
onMounted(() => {
  const el = well.value;
  if (el) el.scrollTop = el.scrollHeight;
});
</script>

<template>
  <div class="agent-thinking">
    <span class="agent-thinking-label">{{ label }}</span>
    <!-- A named group keeps the well identifiable on keyboard focus without
         adding a landmark for every holding. tabindex makes it scrollable by keyboard. -->
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
  /* The bound: fourteen lines of this well's own type, plus its padding and
     border (box-sizing is border-box app-wide). Derived, not a magic pixel. */
  max-height: calc(14 * var(--lh-ui) * var(--t-ui-sm) + 2 * var(--s-4) + 2px);
  overflow-y: auto;
}
/* Keyboard focus on the well itself (it's tabbable so it can be scrolled by
   keyboard) — the same quiet inset ring as the tracker's outer region. */
.agent-thinking-body:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}
</style>
