<script setup lang="ts">
import { ref, nextTick } from "vue";
import Icon, { type IconName } from "./Icon.vue";
import type { ResearchDocument } from "../types";

// A dense, single-column list of research documents, translated from the design
// kit's ResearchInbox.jsx. Both research surfaces — the inbox (drop files in) and
// the archive (read-only; the pipeline files processed documents here) — render
// through this one component; the differences are all copy and the toolbar's
// reveal affordance, passed as props. The list itself is identical: name, format
// and size, modified date, and a per-row delete (allowed from either folder,
// docs/research-documents.md §User Permissions). An inbox row whose last job
// pass could not parse it carries `parse_error` (§Parse Failures) and renders in
// an error state — a tag plus the reason — so the user can fix or delete it;
// archive rows never carry one.
withDefaults(
  defineProps<{
    documents: ResearchDocument[];
    loading: boolean;
    error: string | null;
    // Toolbar title (uppercase surface label).
    title: string;
    // Intro line, shown only when there are documents.
    lede: string;
    // Empty-state eyebrow + body.
    emptyTitle: string;
    emptyBody: string;
    // Heading on the "couldn't read this folder" error block.
    errorLabel: string;
    // The single toolbar action — reveals the folder in the OS file manager.
    revealLabel: string;
    revealTitle: string;
    // Optional leading icon (inbox uses "plus" for "Add files…"; the archive's
    // "Show in Finder" carries none). Variant lets the archive use the quieter
    // secondary button — it's a read-only convenience, not a primary action.
    revealIcon?: IconName | null;
    revealVariant?: "btn-primary" | "btn-secondary";
  }>(),
  {
    revealIcon: null,
    revealVariant: "btn-primary",
  }
);

const emit = defineEmits<{
  (e: "delete", name: string): void;
  (e: "reveal"): void;
}>();

// Per-row delete confirmation: the name of the row currently asking to confirm,
// or null. An inline two-step (Delete → Confirm/Cancel) rather than a modal —
// reversible, keyboard-operable, and it keeps the destructive action off a
// single click.
const confirmingName = ref<string | null>(null);

// Focus management for the inline two-step. Opening the confirm moves focus onto
// the Cancel button so keyboard/SR focus isn't dropped to <body> and the new
// controls are announced; cancelling returns focus to that row's Delete trigger.
// Both controls are recreated across the v-if swap, so they're tracked by function
// refs (the Delete triggers keyed by document name).
const cancelButton = ref<HTMLButtonElement | null>(null);
function setCancelButton(el: unknown) {
  cancelButton.value = el ? (el as HTMLButtonElement) : null;
}
const deleteTriggers = new Map<string, HTMLButtonElement>();
function setDeleteTrigger(name: string, el: unknown) {
  if (el) deleteTriggers.set(name, el as HTMLButtonElement);
  else deleteTriggers.delete(name);
}

async function startConfirm(name: string) {
  confirmingName.value = name;
  await nextTick();
  cancelButton.value?.focus();
}

async function cancelConfirm(name: string) {
  confirmingName.value = null;
  await nextTick();
  deleteTriggers.get(name)?.focus();
}

function confirmDelete(name: string) {
  confirmingName.value = null;
  emit("delete", name);
}

function formatFormat(format: string): string {
  return format ? format.toUpperCase() : "—";
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${kb < 10 ? kb.toFixed(1) : Math.round(kb)} KB`;
  const mb = kb / 1024;
  return `${mb < 10 ? mb.toFixed(1) : Math.round(mb)} MB`;
}

function formatDate(iso: string | null): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "—";
  return d.toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}
</script>

<template>
  <main class="docs-pane">
    <div class="toolbar">
      <div class="toolbar-label">{{ title }}</div>
      <div class="toolbar-actions">
        <button class="btn" :class="revealVariant" :title="revealTitle" @click="emit('reveal')">
          <!-- size ≤14 keeps Icon on its fine 1.1px stroke branch -->
          <Icon v-if="revealIcon" :name="revealIcon" :size="13" />
          {{ revealLabel }}
        </button>
      </div>
    </div>

    <div class="docs-scroll">
      <!-- Lede only when there are documents; the empty state below carries its
           own single instruction, so showing both would just repeat it. -->
      <div v-if="documents.length > 0" class="docs-intro">
        <p class="docs-lede">{{ lede }}</p>
      </div>

      <div v-if="error" class="docs-error" role="alert">
        <div class="docs-error-label">{{ errorLabel }}</div>
        <p class="docs-error-detail">{{ error }}</p>
      </div>

      <!-- Only show the loading line on the first load (no rows yet); a refresh
           with rows already on screen shouldn't blank them out. -->
      <p
        v-else-if="loading && documents.length === 0"
        class="docs-status"
        aria-live="polite"
      >
        Loading…
      </p>

      <div v-else-if="documents.length === 0" class="docs-empty">
        <div class="docs-empty-eyebrow">{{ emptyTitle }}</div>
        <p class="docs-empty-body">{{ emptyBody }}</p>
      </div>

      <ul v-else class="docs-list">
        <li
          v-for="(doc, index) in documents"
          :key="doc.name"
          class="docs-row"
          :class="{ 'is-confirming': confirmingName === doc.name }"
        >
          <Icon name="file" :size="14" color="var(--ink-2)" />
          <div class="docs-row-main">
            <!-- Full name on hover: the name clips with ellipsis (nowrap), so a
                 long file name is otherwise unreadable on the failed row the user
                 must act on. Same affordance as the chart labels' truncation
                 <title>, but applied unconditionally — measuring per-row truncation
                 would need a ResizeObserver; a redundant tooltip on a name that
                 happens to fit is the accepted tradeoff. -->
            <div class="docs-row-name" :title="doc.name">{{ doc.name }}</div>
            <div class="docs-row-meta">
              {{ formatFormat(doc.format) }} · {{ formatSize(doc.size_bytes) }}
              <span v-if="!doc.supported" class="docs-tag">unsupported</span>
              <span v-if="doc.parse_error" class="docs-tag docs-tag--error">parse failed</span>
            </div>
            <!-- The last job pass's failure reason (docs/research-documents.md
                 §Parse Failures): the file stays in the inbox and is retried next
                 run unless fixed or deleted. -->
            <!-- id keys off the row index, not doc.name (the list :key), since a
                 file name isn't a safe/unique id token. The describedby on Delete
                 below is computed from the same index in this row scope, so the
                 id/reference pair never drifts even though list identity is by name. -->
            <p
              v-if="doc.parse_error"
              :id="`parse-error-${index}`"
              class="docs-row-error"
            >{{ doc.parse_error }}</p>
          </div>
          <div class="docs-row-date">{{ formatDate(doc.modified) }}</div>
          <div class="docs-row-actions">
            <template v-if="confirmingName === doc.name">
              <button
                :ref="setCancelButton"
                type="button"
                class="row-action"
                @click="cancelConfirm(doc.name)"
              >
                Cancel
              </button>
              <button
                type="button"
                class="row-action row-action--danger"
                @click="confirmDelete(doc.name)"
              >
                Delete
              </button>
            </template>
            <!-- On a failed row, tie the reason to the control that acts on it,
                 so reaching Delete by keyboard/SR also surfaces why the file is
                 flagged (the reason text is otherwise only in row reading order). -->
            <button
              v-else
              :ref="(el) => setDeleteTrigger(doc.name, el)"
              type="button"
              class="row-action"
              :aria-label="`Delete ${doc.name}`"
              :aria-describedby="doc.parse_error ? `parse-error-${index}` : undefined"
              @click="startConfirm(doc.name)"
            >
              Delete
            </button>
          </div>
        </li>
      </ul>

      <div v-if="documents.length > 0" class="docs-footer">
        {{ documents.length }}
        {{ documents.length === 1 ? "item" : "items" }} · all local
      </div>
    </div>
  </main>
</template>

<style scoped>
.docs-pane {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
  background: var(--paper);
}

/* Toolbar geometry matches the report pane's so the two views share a seam.
   min-height matches the button-less panes; the reveal button already sets this
   height here, so this just pins the shared reference. */
.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 50px;
  padding: var(--s-3) var(--s-8);
  border-bottom: var(--border);
}

/* Surface title: stronger than the section eyebrows — 13px ink semibold (a
   deliberate step up from the 11px caption used for sub-headings). */
.toolbar-label {
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  font-weight: 600;
  color: var(--ink);
}

.toolbar-actions {
  display: flex;
  gap: var(--s-3);
}

.docs-scroll {
  flex: 1;
  overflow-y: auto;
}

.docs-intro {
  max-width: var(--measure);
  padding: var(--s-10) var(--s-8) var(--s-5);
}

/* Chrome-scale serif: annotates the surface without reading at report size. */
.docs-lede {
  margin: 0;
  font-family: var(--font-serif);
  font-size: var(--t-ui-sm);
  line-height: var(--lh-prose);
  letter-spacing: var(--track-prose);
  color: var(--ink-2);
}

.docs-error {
  max-width: var(--measure);
  padding: 0 var(--s-8) var(--s-6);
}

.docs-error-label {
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  font-weight: 600;
  color: var(--accent-text);
  margin-bottom: var(--s-3);
}

.docs-error-detail {
  margin: 0;
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  line-height: var(--lh-ui);
  color: var(--ink-2);
  overflow-wrap: anywhere;
}

.docs-status {
  margin: 0;
  padding: 0 var(--s-8) var(--s-6);
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  color: var(--ink-3);
}

/* Top padding sets the eyebrow off the toolbar seam (matches the report empty
   state's rhythm) so the eyebrow doesn't hug the divider. */
.docs-empty {
  max-width: var(--measure);
  padding: var(--s-10) var(--s-8);
}

.docs-empty-eyebrow {
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  color: var(--ink-3);
  margin-bottom: var(--s-4);
}

/* Chrome-scale serif, not 17px report prose: an empty research folder is product
   chrome, not a reading surface. ink-2 (not ink-3) clears WCAG AA at this size. */
.docs-empty-body {
  margin: 0;
  max-width: var(--measure);
  font-family: var(--font-serif);
  font-size: var(--t-ui-sm);
  line-height: var(--lh-prose);
  letter-spacing: var(--track-prose);
  color: var(--ink-2);
}

/* The list is hairline-ruled top and bottom, dense rows separated by soft
   hairlines — the kit's "filed research" idiom. */
.docs-list {
  list-style: none;
  margin: 0;
  padding: 0;
  border-top: var(--border);
  border-bottom: var(--border);
}

.docs-row {
  display: grid;
  grid-template-columns: 20px minmax(0, 1fr) max-content max-content;
  gap: var(--s-5);
  align-items: baseline;
  padding: var(--s-4) var(--s-8);
  border-bottom: var(--border-soft);
  transition: background-color var(--dur-fast) var(--ease);
}

.docs-row:last-child {
  border-bottom: 0;
}

.docs-row:hover,
.docs-row:focus-within,
.docs-row.is-confirming {
  background: var(--paper-soft);
}

.docs-row-main {
  min-width: 0;
}

.docs-row-name {
  font-family: var(--font-sans);
  font-size: var(--t-ui);
  font-weight: 500;
  color: var(--ink);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.docs-row-meta {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--s-3);
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  color: var(--ink-3);
  margin-top: var(--s-1);
}

/* Unsupported-format tag — an inset-well chip, no color signal (the system
   reserves the accent for warnings/errors, not catalog metadata). */
.docs-tag {
  padding: 1px var(--s-2);
  background: var(--paper-edge);
  border: var(--border-soft);
  border-radius: var(--radius-sm);
  color: var(--ink-2);
}

/* Parse-failure tag — same chip geometry, but this IS a warning/error, so it
   carries the accent voice (the error-label idiom this surface already uses;
   --accent-text is the AA-clearing accent for text). */
.docs-tag--error {
  color: var(--accent-text);
}

/* The failure reason, mirroring the folder-level error block's label/detail
   split: the tag above is the accent label, the reason reads as quiet detail. */
.docs-row-error {
  margin: var(--s-1) 0 0;
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  line-height: var(--lh-ui);
  color: var(--ink-2);
  overflow-wrap: anywhere;
}

.docs-row-date {
  font-family: var(--font-mono);
  font-variant-numeric: tabular-nums lining-nums;
  font-size: var(--t-ui-sm);
  color: var(--ink-3);
  white-space: nowrap;
}

/* Actions reveal on row hover/focus to keep the resting list clean, but stay in
   the tab order (opacity, not display:none) so keyboard users reach them; a row
   mid-confirmation pins them visible. */
.docs-row-actions {
  display: flex;
  gap: var(--s-3);
  opacity: 0;
  transition: opacity var(--dur-fast) var(--ease);
}

.docs-row:hover .docs-row-actions,
.docs-row:focus-within .docs-row-actions,
.docs-row.is-confirming .docs-row-actions {
  opacity: 1;
}

.row-action {
  border: 0;
  background: transparent;
  padding: 0;
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  color: var(--ink-3);
  cursor: pointer;
  transition: color var(--dur-fast) var(--ease);
}

.row-action:hover {
  color: var(--ink);
}

.row-action--danger:hover {
  color: var(--accent-text);
}

.row-action:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}

.docs-footer {
  padding: var(--s-5) var(--s-8);
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  color: var(--ink-3);
}

@media (prefers-reduced-motion: reduce) {
  .docs-row,
  .docs-row-actions,
  .row-action {
    transition: none;
  }
}
</style>
