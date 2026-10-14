<script setup lang="ts">
import { computed } from "vue";
import MarkdownIt from "markdown-it";
import Icon from "./Icon.vue";
import { renderChart } from "../renderChart";
import { localDate, localDateTime } from "../format";
import type { GeneratedReport } from "../types";

const props = defineProps<{
  report: GeneratedReport | null;
  error: string | null;
  // Opening a selected issue failed, as distinct from `error` (generation failed).
  loadError: string | null;
  isLatest: boolean;
  exportingMarkdown: boolean;
  // Markdown export only; PDF export is handled here and has no error channel.
  exportError: string | null;
}>();

const emit = defineEmits<{
  (e: "export-markdown"): void;
}>();

// html:false: raw HTML from the report body never reaches the rendered surface.
const md = new MarkdownIt({ html: false, linkify: true, typographer: true });

// Wrap tables so a wide one scrolls locally instead of pushing the reading
// column sideways; a narrow table keeps its width:100% stretch.
md.renderer.rules.table_open = (tokens, idx, options, _env, self) =>
  `<div class="prose-table-wrap">${self.renderToken(tokens, idx, options)}`;
md.renderer.rules.table_close = (tokens, idx, options, _env, self) =>
  `${self.renderToken(tokens, idx, options)}</div>`;

// ```chart fences render through renderChart; on a parse failure it returns null
// and the fence falls back to a code block, so a malformed chart degrades to its
// source rather than blanking the report.
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

// The per-issue headline, so the open report reads the same as its sidebar row.
const toolbarLabel = computed(() =>
  props.report
    ? props.report.summary.title?.trim() || "Market Signal Report"
    : "Latest report"
);
const toolbarMeta = computed(() =>
  props.report
    ? `${localDateTime(props.report.summary.created_at)} · #${props.report.report_id.slice(0, 8)}`
    : ""
);

const canExport = computed(
  () => props.report !== null && props.error === null && props.loadError === null
);

// PDF export goes through the print panel: Tauri/wry on macOS has no silent
// print-to-PDF (wry#707), and Tauri shims `window.print` to dispatch
// `core:webview:allow-print` (async; the capability is granted in
// capabilities/default.json). `document.title` seeds the panel's suggested
// filename, so it holds the spec's basename (docs/export.md §Export Naming) for
// the print's duration. The date is the local-calendar date, matching the
// Markdown export name.
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
    <!-- Kit ReportToolbar: export lives here; generation lives in the empty-state
         CTA and the footer's "Generate now". -->
    <div class="toolbar">
      <div class="toolbar-heading">
        <span class="toolbar-label">{{ toolbarLabel }}</span>
        <span v-if="report" class="toolbar-meta">{{ toolbarMeta }}</span>
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
          :aria-busy="exportingMarkdown"
          :title="exportingMarkdown ? 'Saving…' : 'Save this report as a Markdown file'"
          @click="emit('export-markdown')"
        >
          <Icon name="file" :size="13" />
          {{ exportingMarkdown ? "Saving…" : "Share as Markdown" }}
        </button>
      </div>
    </div>

    <!-- Export is an action, not a load: the alert leaves the report on screen. -->
    <p v-if="exportError" class="export-error" role="alert">
      Couldn't export: {{ exportError }}
    </p>

    <div class="report-scroll" role="region" aria-label="Report" tabindex="0">
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

.toolbar-heading {
  display: flex;
  align-items: baseline;
  gap: var(--s-3);
  min-width: 0;
}

/* Matches the sidebar row-title; deliberately not the caption treatment of the
   meta and eyebrows. */
.toolbar-label {
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  font-weight: 600;
  color: var(--ink);
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.toolbar-meta {
  flex-shrink: 0;
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  color: var(--ink-3);
}

.toolbar-tag {
  flex-shrink: 0;
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  color: var(--ink-3);
}

/* flex-shrink:0 keeps the buttons whole when the label is long. */
.toolbar-actions {
  flex-shrink: 0;
  display: flex;
  gap: var(--s-3);
}

/* A one-line strip so the report underneath stays visible. */
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

/* Tabbable so arrow keys scroll a report with no focusable content; inset ring
   so the scroll edge doesn't clip it. */
.report-scroll:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
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
