<script setup lang="ts">
import { ref } from "vue";
import Icon from "./Icon.vue";
import type { ResearchDocument } from "../types";

// User-supplied research documents — a dense, single-column list, translated
// from the design kit's ResearchInbox.jsx. The inbox is a plain folder; the user
// drops files in (via "Add files…", which reveals the folder) and the pipeline
// parses them at the next run. Parse-failure error states are a later slice
// (job-start processing isn't built yet), so this surfaces only what's on disk.
defineProps<{
  documents: ResearchDocument[];
  loading: boolean;
  error: string | null;
}>();

const emit = defineEmits<{
  (e: "delete", name: string): void;
  (e: "reveal"): void;
}>();

// Per-row delete confirmation: the name of the row currently asking to confirm,
// or null. An inline two-step (Delete → Confirm/Cancel) rather than a modal —
// reversible, keyboard-operable, and it keeps the destructive action off a
// single click.
const confirmingName = ref<string | null>(null);

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
  <main class="inbox-pane">
    <div class="toolbar">
      <div class="toolbar-label">Research inbox</div>
      <div class="toolbar-actions">
        <button
          class="btn btn-primary"
          title="Opens the inbox folder so you can drop documents in"
          @click="emit('reveal')"
        >
          <!-- size ≤14 keeps Icon on its fine 1.1px stroke branch -->
          <Icon name="plus" :size="13" />
          Add files…
        </button>
      </div>
    </div>

    <div class="inbox-scroll">
      <div class="inbox-intro">
        <p class="inbox-lede">
          Use “Add files…” to open the inbox folder, then drop your PDFs,
          transcripts, or notes inside. The pipeline reads them at the start of
          next week's run; nothing leaves your machine until you generate.
        </p>
      </div>

      <div v-if="error" class="inbox-error" role="alert">
        <div class="inbox-error-label">Couldn't read the inbox</div>
        <p class="inbox-error-detail">{{ error }}</p>
      </div>

      <!-- Only show the loading line on the first load (no rows yet); a refresh
           with rows already on screen shouldn't blank them out. -->
      <p
        v-else-if="loading && documents.length === 0"
        class="inbox-status"
        aria-live="polite"
      >
        Loading…
      </p>

      <div v-else-if="documents.length === 0" class="inbox-empty">
        <div class="inbox-empty-eyebrow">No documents</div>
        <p class="inbox-empty-body">
          No documents yet. Use “Add files…” above to open the folder and add
          some; they're parsed at the start of the next report run.
        </p>
      </div>

      <ul v-else class="inbox-list">
        <li
          v-for="doc in documents"
          :key="doc.name"
          class="inbox-row"
          :class="{ 'is-confirming': confirmingName === doc.name }"
        >
          <Icon name="file" :size="14" color="var(--ink-2)" />
          <div class="inbox-row-main">
            <div class="inbox-row-name">{{ doc.name }}</div>
            <div class="inbox-row-meta">
              {{ formatFormat(doc.format) }} · {{ formatSize(doc.size_bytes) }}
              <span v-if="!doc.supported" class="inbox-tag">unsupported</span>
            </div>
          </div>
          <div class="inbox-row-date">{{ formatDate(doc.modified) }}</div>
          <div class="inbox-row-actions">
            <template v-if="confirmingName === doc.name">
              <button
                type="button"
                class="row-action"
                @click="confirmingName = null"
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
            <button
              v-else
              type="button"
              class="row-action"
              :aria-label="`Delete ${doc.name}`"
              @click="confirmingName = doc.name"
            >
              Delete
            </button>
          </div>
        </li>
      </ul>

      <div v-if="documents.length > 0" class="inbox-footer">
        {{ documents.length }}
        {{ documents.length === 1 ? "item" : "items" }} · all local
      </div>
    </div>
  </main>
</template>

<style scoped>
.inbox-pane {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
  background: var(--paper);
}

/* Toolbar geometry matches the report pane's so the two views share a seam. */
.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--s-3) var(--s-8);
  border-bottom: var(--border);
}

.toolbar-label {
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  color: var(--ink-3);
}

.toolbar-actions {
  display: flex;
  gap: var(--s-3);
}

.inbox-scroll {
  flex: 1;
  overflow-y: auto;
}

.inbox-intro {
  max-width: var(--measure);
  padding: var(--s-10) var(--s-8) var(--s-5);
}

.inbox-lede {
  margin: 0;
  font-family: var(--font-serif);
  font-size: var(--t-prose-sm);
  line-height: var(--lh-prose);
  letter-spacing: var(--track-prose);
  color: var(--ink-2);
}

.inbox-error {
  max-width: var(--measure);
  padding: 0 var(--s-8) var(--s-6);
}

.inbox-error-label {
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  font-weight: 600;
  color: var(--accent);
  margin-bottom: var(--s-3);
}

.inbox-error-detail {
  margin: 0;
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  line-height: var(--lh-ui);
  color: var(--ink-2);
  overflow-wrap: anywhere;
}

.inbox-status {
  margin: 0;
  padding: 0 var(--s-8) var(--s-6);
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  color: var(--ink-3);
}

.inbox-empty {
  max-width: var(--measure);
  padding: 0 var(--s-8) var(--s-10);
}

.inbox-empty-eyebrow {
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  color: var(--ink-3);
  margin-bottom: var(--s-4);
}

.inbox-empty-body {
  margin: 0;
  font-family: var(--font-serif);
  font-size: var(--t-body);
  line-height: var(--lh-prose);
  letter-spacing: var(--track-prose);
  /* ink-2, not ink-3: 17px reading prose must clear WCAG AA (4.5:1). */
  color: var(--ink-2);
}

/* The list is hairline-ruled top and bottom, dense rows separated by soft
   hairlines — the kit's "filed research" idiom. */
.inbox-list {
  list-style: none;
  margin: 0;
  padding: 0;
  border-top: var(--border);
  border-bottom: var(--border);
}

.inbox-row {
  display: grid;
  grid-template-columns: 20px minmax(0, 1fr) max-content max-content;
  gap: var(--s-5);
  align-items: baseline;
  padding: var(--s-4) var(--s-8);
  border-bottom: var(--border-soft);
  transition: background-color var(--dur-fast) var(--ease);
}

.inbox-row:last-child {
  border-bottom: 0;
}

.inbox-row:hover,
.inbox-row:focus-within,
.inbox-row.is-confirming {
  background: var(--paper-soft);
}

.inbox-row-main {
  min-width: 0;
}

.inbox-row-name {
  font-family: var(--font-sans);
  font-size: var(--t-ui);
  font-weight: 500;
  color: var(--ink);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.inbox-row-meta {
  display: flex;
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
.inbox-tag {
  padding: 1px var(--s-2);
  background: var(--paper-edge);
  border: var(--border-soft);
  border-radius: var(--radius-sm);
  color: var(--ink-2);
}

.inbox-row-date {
  font-family: var(--font-mono);
  font-variant-numeric: tabular-nums lining-nums;
  font-size: var(--t-ui-sm);
  color: var(--ink-3);
  white-space: nowrap;
}

/* Actions reveal on row hover/focus to keep the resting list clean, but stay in
   the tab order (opacity, not display:none) so keyboard users reach them; a row
   mid-confirmation pins them visible. */
.inbox-row-actions {
  display: flex;
  gap: var(--s-3);
  opacity: 0;
  transition: opacity var(--dur-fast) var(--ease);
}

.inbox-row:hover .inbox-row-actions,
.inbox-row:focus-within .inbox-row-actions,
.inbox-row.is-confirming .inbox-row-actions {
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
  color: var(--accent);
}

.row-action:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}

.inbox-footer {
  padding: var(--s-5) var(--s-8);
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  color: var(--ink-3);
}

@media (prefers-reduced-motion: reduce) {
  .inbox-row,
  .inbox-row-actions,
  .row-action {
    transition: none;
  }
}
</style>
