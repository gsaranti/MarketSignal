<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import Icon from "./Icon.vue";
import type { ValidationReport, WarningCategory } from "../types";

// Persistent Warning Area (docs/interface.md). Rendered in the chrome/status
// register (sans type on an inset band with an accent header), never the serif
// reading register, so it can't be mistaken for report content. An extension of
// the kit's WarningBar, whose plain-serif treatment was indistinguishable from
// the report body.
const props = defineProps<{
  report: ValidationReport | null;
  error: string | null;
}>();

// Dismiss is offered only for the non-blocking failed-job kind; blocking config
// gaps are gate state, not notices (mirrors `WarningKind::is_blocking`). The
// dismiss carries the row's rendered `dismiss_id` so the backend suppresses
// *this* warning, not a newer one it would re-derive.
const emit = defineEmits<{ (e: "dismiss", kind: string, dismissId: string): void }>();

const DISMISSIBLE_KINDS = new Set(["failed-job"]);
function isDismissible(kind: string): boolean {
  return DISMISSIBLE_KINDS.has(kind);
}

// The backend always pairs a dismissible kind with an id; this stays defensive.
function canDismiss(cat: WarningCategory): boolean {
  return isDismissible(cat.kind) && cat.dismiss_id != null;
}
function dismiss(cat: WarningCategory): void {
  if (cat.dismiss_id != null) emit("dismiss", cat.kind, cat.dismiss_id);
}

// Only the backend's non-empty categories are ever present in the report.
const categories = computed(() => props.report?.categories ?? []);

// Silent while the first check is still loading, rather than flashing a row.
const visible = computed(
  () => props.error !== null || categories.value.length > 0
);

// Session-scoped: the component stays mounted across view switches.
const collapsed = ref(false);

const issueCount = computed(() => (props.error ? 1 : 0) + categories.value.length);

// Re-expand when a NEW warning appears, even if the user had collapsed the band.
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

// The list is height-bounded and takes a tab stop only while it overflows (so it
// scrolls by keyboard); a short list adds no stop to the global chrome. A resize
// re-measures through the observer, a content change through the watch.
const list = ref<HTMLUListElement | null>(null);
const overflowing = ref(false);
function measure(): void {
  const el = list.value;
  overflowing.value = el !== null && el.scrollHeight > el.clientHeight + 1;
}
const observer =
  typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
watch(list, (el, prev) => {
  if (prev) observer?.unobserve(prev);
  if (el) observer?.observe(el);
  measure();
});
watch(
  () => [props.report, props.error, collapsed.value],
  () => nextTick(measure),
  { deep: true }
);
onBeforeUnmount(() => observer?.disconnect());
</script>

<template>
  <!-- Always mounted: a live region must pre-exist in the DOM for inserted
       content to be announced. -->
  <div class="warning-live" aria-live="polite">
  <section
    v-if="visible"
    class="warning-area"
    aria-label="Needs attention"
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
        color="var(--accent-text)"
        class="warning-chevron"
        :class="{ 'is-open': !collapsed }"
      />
    </button>
    <ul
      v-show="!collapsed"
      id="warning-list"
      ref="list"
      class="warning-list"
      :tabindex="overflowing ? 0 : undefined"
    >
      <li v-if="error" class="warning-row">
        <span class="warning-label">Configuration</span>
        <span class="warning-body">
          Couldn't check configuration — {{ error }}
        </span>
      </li>
      <!-- Keyed on kind AND position: uniqueness by kind alone depends on App.vue's
           merge of two validation reports holding. -->
      <li
        v-for="(cat, i) in categories"
        :key="`${cat.kind}-${i}`"
        class="warning-row"
      >
        <span class="warning-label">{{ cat.title }}</span>
        <span class="warning-body">
          <span class="warning-text">{{ formatItems(cat.items) }}</span>
          <button
            v-if="canDismiss(cat)"
            type="button"
            class="warning-dismiss"
            :aria-label="`Dismiss: ${cat.title}`"
            @click="dismiss(cat)"
          >
            <Icon name="close" :size="12" color="currentColor" />
          </button>
        </span>
      </li>
    </ul>
  </section>
  </div>
</template>

<style scoped>
/* Inset-well band so the area reads as system status, not report content. Left
   padding matches the toolbar/footer so the left edge aligns across regions. */
.warning-area {
  background: var(--paper-edge);
  border-bottom: var(--border);
  padding: var(--s-4) var(--s-8);
  /* Collapsed, matches the sidebar header's height so the bottom seams line up
     across the column gutter. */
  min-height: 44px;
  display: flex;
  flex-direction: column;
  justify-content: center;
  /* Rows adapt to the content column's width, not the viewport's. */
  container-type: inline-size;
}

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
  outline-offset: 1px;
}

/* Hover uses --accent-press as text: no darker accent-text token exists, and it
   clears AA on the band in both themes. */
.warning-toggle:hover .warning-head {
  color: var(--accent-press);
}

.warning-head {
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  font-weight: 600;
  color: var(--accent-text);
}

.warning-count {
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  color: var(--ink-2);
}

.warning-chevron {
  margin-left: auto;
  transition: transform var(--dur-fast) var(--ease);
}

.warning-chevron.is-open {
  transform: rotate(180deg);
}

/* No inter-row hairlines: those made one area look like three sections. The
   bound is about eight lines of the body type plus the list's padding
   (border-box app-wide), so a long error scrolls inside the band instead of
   growing it over the view below. The --s-3 padding gutter keeps the dismiss
   control's negative-margin hit area and focus outline inside the scroll box;
   the matching negative margins hold the text in place. */
.warning-list {
  list-style: none;
  margin: 0 calc(-1 * var(--s-3)) calc(-1 * var(--s-3));
  padding: var(--s-3);
  display: flex;
  flex-direction: column;
  gap: var(--s-2);
  max-height: calc(8 * var(--lh-ui) * var(--t-ui-sm) + 2 * var(--s-3));
  overflow-y: auto;
}

.warning-list:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}

/* Stacked by default: the fallback where container queries are unsupported. */
.warning-row {
  display: flex;
  flex-direction: column;
  gap: var(--s-1);
}

/* display:contents lets each row's label + body join the list grid, so every
   body shares one left edge regardless of label length. */
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

/* Sans keeps the status text out of the reading register. A flex row so the
   dismiss control sits at the trailing edge without breaking the two-column
   grid: each row keeps exactly two grid children. */
.warning-body {
  min-width: 0;
  display: flex;
  align-items: baseline;
  gap: var(--s-3);
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  line-height: var(--lh-ui);
  color: var(--ink);
  overflow-wrap: anywhere;
}

.warning-text {
  min-width: 0;
  flex: 1 1 auto;
  overflow-wrap: anywhere;
}

/* Negative margins let the padded hit area reach the row edges without adding
   height to the band. */
.warning-dismiss {
  flex: none;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: var(--s-1);
  margin: calc(-1 * var(--s-1)) calc(-1 * var(--s-1)) calc(-1 * var(--s-1)) 0;
  border: 0;
  border-radius: var(--radius);
  background: transparent;
  color: var(--ink-2);
  cursor: pointer;
  transition: color var(--dur-fast) var(--ease), background var(--dur-fast) var(--ease);
}

/* Accent glyph on a faint paper tone, not a fill, keeps the close icon legible. */
.warning-dismiss:hover {
  color: var(--accent-text);
  background: var(--paper-soft);
}

.warning-dismiss:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 1px;
}

@media (prefers-reduced-motion: reduce) {
  .warning-chevron,
  .warning-dismiss {
    transition: none;
  }
}
</style>
