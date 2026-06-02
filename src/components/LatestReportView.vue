<script setup lang="ts">
import { computed } from "vue";
import MarkdownIt from "markdown-it";
import type { GeneratedReport } from "../types";

const props = defineProps<{
  report: GeneratedReport | null;
  generating: boolean;
  error: string | null;
}>();

defineEmits<{ (e: "generate"): void }>();

// html:false — the Markdown is our own trusted report body, and we never want
// raw HTML from it leaking into the rendered surface.
const md = new MarkdownIt({ html: false, linkify: true, typographer: true });

const renderedHtml = computed(() =>
  props.report ? md.render(props.report.markdown) : ""
);
</script>

<template>
  <main class="report-pane">
    <div class="toolbar">
      <div class="toolbar-label">Latest report</div>
      <div class="toolbar-actions">
        <button
          class="btn btn-primary"
          :disabled="generating"
          @click="$emit('generate')"
        >
          {{ generating ? "Generating…" : "Generate report" }}
        </button>
        <button class="btn btn-secondary" disabled>Export</button>
      </div>
    </div>

    <div class="report-scroll">
      <p v-if="error" class="report-error">{{ error }}</p>
      <!-- eslint-disable-next-line vue/no-v-html -->
      <article
        v-else-if="report"
        class="prose report-article"
        v-html="renderedHtml"
      ></article>
      <div v-else class="report-empty">
        <p>No report yet. Generate one to see it here.</p>
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

.toolbar-actions {
  display: flex;
  gap: var(--s-3);
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
  font-family: var(--font-serif);
  font-size: var(--t-body);
  color: var(--ink-3);
}

.report-error {
  max-width: var(--measure);
  margin: 0 auto;
  padding: var(--s-8);
  font-family: var(--font-sans);
  color: var(--accent);
}
</style>
