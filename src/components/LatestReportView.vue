<script setup lang="ts">
import { computed } from "vue";
import MarkdownIt from "markdown-it";
import type { GeneratedReport } from "../types";

const props = defineProps<{
  report: GeneratedReport | null;
  generating: boolean;
  error: string | null;
  blocked?: boolean;
}>();

defineEmits<{ (e: "generate"): void }>();

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
</script>

<template>
  <main class="report-pane">
    <!-- A quiet reading toolbar: generation lives in the empty-state CTA and the
         footer's "Generate now"; export returns here when that slice lands. -->
    <div class="toolbar">
      <div class="toolbar-label">Latest report</div>
    </div>

    <div class="report-scroll">
      <div v-if="error" class="report-error" role="alert">
        <div class="report-error-label">Generation failed</div>
        <p class="report-error-detail">{{ error }}</p>
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
        <div class="report-empty-actions">
          <button
            class="btn btn-primary"
            :disabled="generating || props.blocked"
            @click="$emit('generate')"
          >
            {{ generating ? "Generating…" : "Generate report" }}
          </button>
          <!-- Visible, not hover-only: the gate's reason is in the warning band
               above; this names the blocker at the disabled control itself. -->
          <p v-if="props.blocked" class="report-empty-hint">
            Resolve the configuration warnings above to generate a report.
          </p>
        </div>
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

/* The primary call-to-action on the empty surface — this is the report view's
   home for manual generation now that the toolbar is reading-only. */
.report-empty-actions {
  margin-top: var(--s-7);
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: var(--s-4);
}

/* The blocked reason, shown inline (not as a hover title) so keyboard and touch
   users see why generation is unavailable. ink-2: a 13px hint must clear AA. */
.report-empty-hint {
  margin: 0;
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  line-height: var(--lh-ui);
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
