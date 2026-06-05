<script setup lang="ts">
import { computed } from "vue";
import MarkdownIt from "markdown-it";
import type { GeneratedReport } from "../types";

const props = defineProps<{
  report: GeneratedReport | null;
  error: string | null;
  // A load failure when opening a selected issue (distinct from a generation
  // failure) — e.g. the Markdown file was removed out-of-band.
  loadError: string | null;
  // Whether the shown report is the newest one — drives the "Latest" tag.
  isLatest: boolean;
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

const renderedHtml = computed(() =>
  props.report ? md.render(props.report.markdown) : ""
);

// The toolbar reflects which issue is shown: the selected report's date and
// short id, rather than a static label. Falls back to "Latest report" when no
// report is loaded.
const toolbarLabel = computed(() =>
  props.report
    ? `${props.report.summary.created_at.slice(0, 10)} · #${props.report.report_id.slice(0, 8)}`
    : "Latest report"
);
</script>

<template>
  <main class="report-pane">
    <!-- A quiet reading toolbar: generation lives in the empty-state CTA and the
         footer's "Generate now"; export returns here when that slice lands. The
         label reflects the selected issue, with a tag when it's the newest. -->
    <div class="toolbar">
      <div class="toolbar-heading">
        <span class="toolbar-label">{{ toolbarLabel }}</span>
        <span v-if="report && isLatest" class="toolbar-tag">Latest</span>
      </div>
    </div>

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
        <div class="report-empty-eyebrow">Weekly market report</div>
        <p class="report-empty-body">
          No issue has been generated yet. When you generate one — or the
          Sunday job runs — it will appear here.
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
  color: var(--accent);
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
