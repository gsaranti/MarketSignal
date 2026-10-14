<script setup lang="ts">
import { ref, nextTick } from "vue";
import Icon, { type IconName } from "./Icon.vue";
import type { ResearchDocument } from "../types";

// The research-document list (design kit ResearchInbox.jsx), shared by the
// inbox and the archive: the differences are copy and the toolbar's reveal
// affordance, passed as props. Delete is allowed from either folder
// (docs/research-documents.md §User Permissions); only inbox rows carry
// `parse_error` (§Parse Failures).
withDefaults(
  defineProps<{
    documents: ResearchDocument[];
    loading: boolean;
    error: string | null;
    title: string;
    lede: string;
    emptyTitle: string;
    emptyBody: string;
    errorLabel: string;
    revealLabel: string;
    revealTitle: string;
    // The archive's reveal is a read-only convenience, so it takes the quieter
    // secondary variant and no icon.
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

// The row mid-confirmation: an inline two-step rather than a modal, keeping the
// destructive action off a single click.
const confirmingName = ref<string | null>(null);

// Opening the confirm moves focus to Cancel so it isn't dropped to <body>;
// cancelling returns it to that row's Delete. Both are recreated across the
// v-if swap, so they're tracked by function refs.
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
      <!-- The empty state carries its own instruction; a lede too would repeat it. -->
      <div v-if="documents.length > 0" class="docs-intro">
        <p class="docs-lede">{{ lede }}</p>
      </div>

      <div v-if="error" class="docs-error" role="alert">
        <div class="docs-error-label">{{ errorLabel }}</div>
        <p class="docs-error-detail">{{ error }}</p>
      </div>

      <!-- First load only: a refresh with rows on screen shouldn't blank them out. -->
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
            <!-- The name clips with ellipsis. The tooltip is unconditional: measuring
                 per-row truncation would need a ResizeObserver. -->
            <div class="docs-row-name" :title="doc.name">{{ doc.name }}</div>
            <div class="docs-row-meta">
              {{ formatFormat(doc.format) }} · {{ formatSize(doc.size_bytes) }}
              <span v-if="!doc.supported" class="docs-tag">unsupported</span>
              <span v-if="doc.parse_error" class="docs-tag docs-tag--error">parse failed</span>
            </div>
            <!-- docs/research-documents.md §Parse Failures. The id keys off the index,
                 not doc.name: a file name isn't a safe id token. Delete's describedby
                 uses the same index in this row scope. -->
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
            <!-- Ties the reason to the control that acts on it, so reaching Delete
                 by keyboard/SR surfaces why the file is flagged. -->
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

/* Geometry matches the report pane's toolbar so the views share a seam. */
.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 50px;
  padding: var(--s-3) var(--s-8);
  border-bottom: var(--border);
}

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

/* ink-2 (not ink-3) clears WCAG AA at this size. */
.docs-empty-body {
  margin: 0;
  max-width: var(--measure);
  font-family: var(--font-serif);
  font-size: var(--t-ui-sm);
  line-height: var(--lh-prose);
  letter-spacing: var(--track-prose);
  color: var(--ink-2);
}

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

/* No color signal: the accent is reserved for warnings and errors. */
.docs-tag {
  padding: 1px var(--s-2);
  background: var(--paper-edge);
  border: var(--border-soft);
  border-radius: var(--radius-sm);
  color: var(--ink-2);
}

/* This one IS an error: --accent-text, the AA-clearing accent for text. */
.docs-tag--error {
  color: var(--accent-text);
}

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

/* Hidden by opacity, not display:none, so the actions stay in the tab order. */
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
