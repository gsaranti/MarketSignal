<script setup lang="ts">
import { computed } from "vue";
import MarkdownIt from "markdown-it";
import Icon from "./Icon.vue";
import { renderChart } from "../renderChart";
import { localDate } from "../format";
import type { GeneratedReport } from "../types";

const props = defineProps<{
  report: GeneratedReport | null;
  error: string | null;
  // A load failure when opening a selected issue (distinct from a generation
  // failure) — e.g. the Markdown file was removed out-of-band.
  loadError: string | null;
  // Whether the shown report is the newest one — drives the "Latest" tag.
  isLatest: boolean;
  // Markdown export is in flight (the parent owns the invoke); drives the
  // "Share as Markdown" button's busy/disabled state.
  exportingMarkdown: boolean;
  // A failed Markdown export, surfaced as a slim inline alert under the toolbar.
  // PDF export is handled locally here and has no parent-tracked error channel.
  exportError: string | null;
}>();

const emit = defineEmits<{
  // Ask the parent to run the Markdown export (Save dialog + write live in the
  // backend command; the parent owns the invoke and the error/busy state).
  (e: "export-markdown"): void;
}>();

// html:false — the Markdown is our own trusted report body, and we never want
// raw HTML from it leaking into the rendered surface.
const md = new MarkdownIt({ html: false, linkify: true, typographer: true });

// Wrap rendered tables in a horizontal-scroll container so a wide or
// many-column report table scrolls locally instead of pushing the whole
// reading column sideways (frontend-craft overflow handling — reports
// routinely contain tables, e.g. the watchlist). The wrapper keeps the
// table's width:100% stretch for narrow tables; only wide ones scroll.
md.renderer.rules.table_open = (tokens, idx, options, _env, self) =>
  `<div class="prose-table-wrap">${self.renderToken(tokens, idx, options)}`;
md.renderer.rules.table_close = (tokens, idx, options, _env, self) =>
  `${self.renderToken(tokens, idx, options)}</div>`;

// Intercept ```chart fences and render their JSON body as a restrained inline-SVG
// line figure (see ../renderChart). On any parse/validation failure renderChart
// returns null and we fall back to the default code-block rendering, so a
// malformed chart degrades to its raw source rather than blanking the report.
const defaultFence =
  md.renderer.rules.fence ??
  ((tokens, idx, options, _env, self) => self.renderToken(tokens, idx, options));
md.renderer.rules.fence = (tokens, idx, options, env, self) => {
  const token = tokens[idx];
  if (token.info.trim() === "chart") {
    const svg = renderChart(token.content);
    if (svg !== null) return svg;
  }
  return defaultFence(tokens, idx, options, env, self);
};

const renderedHtml = computed(() =>
  props.report ? md.render(props.report.markdown) : ""
);

// The toolbar reflects which issue is shown: the selected report's date and
// short id, rather than a static label. Falls back to "Latest report" when no
// report is loaded.
const toolbarLabel = computed(() =>
  props.report
    ? `${localDate(props.report.summary.created_at)} · #${props.report.report_id.slice(0, 8)}`
    : "Latest report"
);

// Export is only meaningful when a report is actually on screen — not while an
// error/load-error block is showing in its place, and not on the empty state.
const canExport = computed(
  () => props.report !== null && props.error === null && props.loadError === null
);

// Export the rendered report as PDF via the webview's native print-to-PDF (the
// macOS print panel's "Save as PDF"; there is no silent print-to-PDF API in
// Tauri/wry on macOS — wry#707). On macOS, Tauri replaces `window.print` with a
// shim that dispatches the `core:webview:allow-print` command (hence it is async
// and requires that capability — granted in capabilities/default.json). The
// `@media print` stylesheet isolates the report article; `document.title` seeds
// the panel's suggested filename, so set it to the spec's basename (docs/export.md
// §Export Naming — no internal id suffix) for the duration of the print, then
// restore. The date is the local-calendar date (see ../format), matching the
// Markdown export name and the toolbar dateline so all three agree.
async function exportPdf() {
  if (!props.report) return;
  const base = `${localDate(props.report.summary.created_at)}-market-signal-report`;
  const previousTitle = document.title;
  document.title = base;
  try {
    await window.print();
  } finally {
    document.title = previousTitle;
  }
}
</script>

<template>
  <main class="report-pane">
    <!-- A quiet reading toolbar: generation lives in the empty-state CTA and the
         footer's "Generate now"; export lives here (kit ReportToolbar). The label
         reflects the selected issue, with a tag when it's the newest. -->
    <div class="toolbar">
      <div class="toolbar-heading">
        <span class="toolbar-label">{{ toolbarLabel }}</span>
        <span v-if="report && isLatest" class="toolbar-tag">Latest</span>
      </div>
      <div class="toolbar-actions">
        <button
          type="button"
          class="btn btn-secondary"
          :disabled="!canExport"
          title="Export this report as a PDF"
          @click="exportPdf"
        >
          <Icon name="export_" :size="13" />
          Export PDF
        </button>
        <button
          type="button"
          class="btn btn-secondary"
          :disabled="!canExport || exportingMarkdown"
          :title="exportingMarkdown ? 'Saving…' : 'Save this report as a Markdown file'"
          @click="emit('export-markdown')"
        >
          <Icon name="file" :size="13" />
          {{ exportingMarkdown ? "Saving…" : "Share as Markdown" }}
        </button>
      </div>
    </div>

    <!-- A failed Markdown export: a slim, non-destructive alert under the toolbar
         that leaves the report on screen (export is an action, not a load). -->
    <p v-if="exportError" class="export-error" role="alert">
      Couldn't export: {{ exportError }}
    </p>

    <div class="report-scroll">
      <div v-if="error" class="report-error" role="alert">
        <div class="report-error-label">Generation failed</div>
        <p class="report-error-detail">{{ error }}</p>
      </div>
      <div v-else-if="loadError" class="report-error" role="alert">
        <div class="report-error-label">Couldn't open this report</div>
        <p class="report-error-detail">{{ loadError }}</p>
      </div>
      <!-- eslint-disable-next-line vue/no-v-html -->
      <article
        v-else-if="report"
        class="prose report-article"
        v-html="renderedHtml"
      ></article>
      <div v-else class="report-empty">
        <div class="report-empty-eyebrow">Market Signal report</div>
        <p class="report-empty-body">
          No issue has been generated yet. When you generate one, it will appear
          here.
        </p>
      </div>
    </div>
  </main>
</template>

<style scoped>
.report-pane {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
  background: var(--paper);
}

/* min-height keeps the toolbar seam uniform with the inbox/settings panes even
   though this reading toolbar carries no button. */
.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 50px;
  padding: var(--s-3) var(--s-8);
  border-bottom: var(--border);
}

/* The label and its "Latest" tag share a baseline-aligned row. */
.toolbar-heading {
  display: flex;
  align-items: baseline;
  gap: var(--s-3);
  min-width: 0;
}

/* Surface title: stronger than the section eyebrows it sits above — 13px ink
   semibold (a deliberate step up from the 11px caption used for sub-headings). */
.toolbar-label {
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  font-weight: 600;
  color: var(--ink);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* "Latest" tag: a quiet caption next to the dateline, marking the newest issue
   without competing with the title. */
.toolbar-tag {
  flex-shrink: 0;
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  color: var(--ink-3);
}

/* The export pair (Export PDF · Share as Markdown), matching the kit's
   ReportToolbar action group and the inbox/archive toolbars' button row.
   flex-shrink:0 keeps the buttons whole when the dateline label is long. */
.toolbar-actions {
  flex-shrink: 0;
  display: flex;
  gap: var(--s-3);
}

/* Slim export-failure alert under the toolbar seam — uses the same caption-scale
   sans as the report's load-error label, but as a one-line strip so the report
   underneath stays visible (export is an action, not a load failure). */
.export-error {
  margin: 0;
  padding: var(--s-2) var(--s-8);
  border-bottom: var(--border);
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  line-height: var(--lh-ui);
  color: var(--ink-2);
  overflow-wrap: anywhere;
}

.report-scroll {
  flex: 1;
  overflow-y: auto;
}

.report-article {
  max-width: var(--measure-wide);
  margin: 0 auto;
  padding: var(--s-10) var(--s-8) var(--s-12);
}

.report-empty {
  max-width: var(--measure);
  margin: 0 auto;
  padding: var(--s-10) var(--s-8);
}

.report-empty-eyebrow {
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  color: var(--ink-3);
  margin-bottom: var(--s-4);
}

.report-empty-body {
  margin: 0;
  font-family: var(--font-serif);
  font-size: var(--t-body);
  line-height: var(--lh-prose);
  letter-spacing: var(--track-prose);
  /* ink-2, not ink-3: 17px reading prose must clear WCAG AA (4.5:1); ink-3 on
     paper is ~4.3:1. */
  color: var(--ink-2);
}

.report-error {
  max-width: var(--measure);
  margin: 0 auto;
  padding: var(--s-10) var(--s-8);
}

.report-error-label {
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  font-weight: 600;
  color: var(--accent-text);
  margin-bottom: var(--s-3);
}

.report-error-detail {
  margin: 0;
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  line-height: var(--lh-ui);
  color: var(--ink-2);
  overflow-wrap: anywhere;
}
</style>
