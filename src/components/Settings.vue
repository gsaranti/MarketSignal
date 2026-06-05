<script setup lang="ts">
import { ref, computed, watch } from "vue";
import Icon from "./Icon.vue";
import ConnectionTestRow from "./ConnectionTestRow.vue";
import type {
  SettingsView,
  AgentModels,
  ConnectionTestResult,
  CredentialUpdate,
  ModelOption,
} from "../types";

// Configuration surface — agent models, API tokens, provider credentials
// (docs/configuration.md, docs/interface.md §Settings). Presentational: state
// lives in App.vue, which fetches via `get_settings` and persists via
// `save_settings`. Visual idiom from the design kit's Settings.jsx (single
// column, label-above-field, open bottom-border inputs); the data is the spec's
// real configuration, not the kit's mock. Secrets never arrive here — a
// credential is shown only as "configured" or not; the user types a new value to
// replace one, and an untouched field is left unchanged on save.
const props = defineProps<{
  settings: SettingsView | null;
  loading: boolean;
  saving: boolean;
  error: string | null;
  // Weekly-schedule control — Settings is the single home for it; the footer
  // only reports run status (docs/interface.md §Settings).
  jobEnabled: boolean | null;
  jobBusy: boolean;
  // Per-credential "Test connection" state, owned by App (the single invoke
  // home): which credential is mid-test, and the last result for each.
  testing: Record<CredKey, boolean>;
  testResults: Record<CredKey, ConnectionTestResult | null>;
}>();

const emit = defineEmits<{
  (e: "save", payload: { models: AgentModels; credentials: CredentialUpdate }): void;
  (e: "set-enabled", value: boolean): void;
  (e: "test", key: CredKey): void;
}>();

type ModelKey = "main" | "bull" | "bear" | "balanced";
type CredKey = "openai" | "anthropic" | "fmp" | "tavily";

const agentFields: { key: ModelKey; label: string }[] = [
  { key: "main", label: "Main Agent" },
  { key: "bull", label: "Bull Analyst" },
  { key: "bear", label: "Bear Analyst" },
  { key: "balanced", label: "Balanced Analyst" },
];

const tokenFields: { key: CredKey; label: string }[] = [
  { key: "openai", label: "OpenAI API token" },
  { key: "anthropic", label: "Anthropic API token" },
];

const credentialFields: { key: CredKey; label: string }[] = [
  { key: "fmp", label: "Financial Modeling Prep" },
  { key: "tavily", label: "Tavily" },
];

// Local, editable form state. Models pre-select the saved slugs; credential
// inputs always start empty (the secret is never sent to the webview).
const local = ref<AgentModels>({ main: "", bull: "", bear: "", balanced: "" });
const creds = ref<Record<CredKey, string>>({
  openai: "",
  anthropic: "",
  fmp: "",
  tavily: "",
});
const justSaved = ref(false);

// (Re)initialise the form whenever a fresh view arrives — on first load and
// after a save (App re-fetches, which clears typed secrets and resets the
// dirty baseline to what was just persisted).
watch(
  () => props.settings,
  (s) => {
    if (!s) return;
    local.value = { ...s.models };
    creds.value = { openai: "", anthropic: "", fmp: "", tavily: "" };
  },
  { immediate: true }
);

// A new save starting clears a stale confirmation; a clean completion sets it.
watch(
  () => props.saving,
  (now, was) => {
    if (now && !was) justSaved.value = false;
    if (was && !now && props.error === null) justSaved.value = true;
  }
);

// Model dropdown options, grouped by provider (order preserved from the backend).
const providerGroups = computed<{ provider: string; options: ModelOption[] }[]>(() => {
  const groups: { provider: string; options: ModelOption[] }[] = [];
  for (const opt of props.settings?.available_models ?? []) {
    let group = groups.find((g) => g.provider === opt.provider);
    if (!group) {
      group = { provider: opt.provider, options: [] };
      groups.push(group);
    }
    group.options.push(opt);
  }
  return groups;
});

const modelsDirty = computed(() => {
  const s = props.settings?.models;
  if (!s) return false;
  const m = local.value;
  return (
    m.main !== s.main ||
    m.bull !== s.bull ||
    m.bear !== s.bear ||
    m.balanced !== s.balanced
  );
});

const credsDirty = computed(() =>
  (Object.keys(creds.value) as CredKey[]).some((k) => creds.value[k].trim() !== "")
);

const dirty = computed(() => modelsDirty.value || credsDirty.value);

// Saving is disabled until both API tokens are present (docs/configuration.md
// §API Tokens) — either already stored (configured) or entered now. The backend
// enforces the same gate; this just keeps the Save control honest.
const tokensSatisfied = computed(() => {
  const c = props.settings?.credentials;
  const openai = creds.value.openai.trim() !== "" || !!c?.openai;
  const anthropic = creds.value.anthropic.trim() !== "" || !!c?.anthropic;
  return openai && anthropic;
});
const needsTokens = computed(() => dirty.value && !tokensSatisfied.value);

const canSave = computed(
  () =>
    !!props.settings &&
    !props.saving &&
    !props.loading &&
    dirty.value &&
    tokensSatisfied.value
);

// Scheduled-job control — the footer reports status; this is where it's toggled.
const scheduleEnabled = computed(() => props.jobEnabled ?? false);
function toggleSchedule() {
  if (props.jobBusy || props.jobEnabled === null) return;
  emit("set-enabled", !scheduleEnabled.value);
}

// "Saved" shows only while the form is at rest and unchanged since the save.
const showSaved = computed(
  () => justSaved.value && !dirty.value && !props.saving && props.error === null
);

function tokenPlaceholder(key: CredKey): string {
  return props.settings?.credentials[key]
    ? "•••• saved — type a new value to replace"
    : "Not set";
}

function onSave() {
  if (!canSave.value) return;
  const credUpdate: CredentialUpdate = {
    openai: creds.value.openai.trim() ? creds.value.openai : null,
    anthropic: creds.value.anthropic.trim() ? creds.value.anthropic : null,
    fmp: creds.value.fmp.trim() ? creds.value.fmp : null,
    tavily: creds.value.tavily.trim() ? creds.value.tavily : null,
  };
  emit("save", { models: { ...local.value }, credentials: credUpdate });
}

// Whether a credential field holds a typed (unsaved) value — drives the test
// row's gating (it validates the saved credential, not what's typed).
function credDirty(key: CredKey): boolean {
  return creds.value[key].trim() !== "";
}
</script>

<template>
  <main class="settings-pane">
    <div class="toolbar">
      <div class="toolbar-label">Settings</div>
    </div>

    <div class="settings-scroll">
      <div class="settings-body">
        <!-- Weekly-schedule control: the single home for enabling/disabling the
             Sunday job (docs/interface.md §Settings). The footer only reports
             status. Leads the surface (not below Save) and renders regardless of
             the config form's load state. -->
        <section
          class="settings-section settings-section--lead"
          aria-labelledby="sec-schedule"
        >
          <h3 id="sec-schedule" class="section-eyebrow">Scheduled job</h3>
          <div class="control-row">
            <div class="control-text">
              <div class="control-label">Weekly report</div>
              <div class="control-hint">
                {{
                  scheduleEnabled
                    ? "Runs automatically every Sunday at 9:00 AM."
                    : "Scheduled runs are paused."
                }}
              </div>
            </div>
            <button
              type="button"
              class="switch"
              role="switch"
              :aria-checked="scheduleEnabled"
              :aria-label="
                scheduleEnabled
                  ? 'Disable weekly report job'
                  : 'Enable weekly report job'
              "
              :disabled="jobBusy || jobEnabled === null"
              @click="toggleSchedule"
            >
              <span
                class="switch-knob"
                :class="{ 'switch-knob--on': scheduleEnabled }"
              ></span>
            </button>
          </div>
        </section>

        <p
          v-if="loading && !settings"
          class="settings-status"
          aria-live="polite"
        >
          Loading…
        </p>

        <div v-else-if="!settings && error" class="settings-error" role="alert">
          <div class="settings-error-label">Couldn't load settings</div>
          <p class="settings-error-detail">{{ error }}</p>
        </div>

        <form v-else-if="settings" class="settings-form" @submit.prevent="onSave">
          <section class="settings-section" aria-labelledby="sec-models">
            <h3 id="sec-models" class="section-eyebrow">Agent models</h3>
            <p class="section-note">All four must be set before a report can run.</p>
            <div v-for="field in agentFields" :key="field.key" class="field">
              <label class="label" :for="`model-${field.key}`">{{ field.label }}</label>
              <div class="select-wrap">
                <select
                  :id="`model-${field.key}`"
                  v-model="local[field.key]"
                  class="field-select"
                >
                  <option value="">— Select a model —</option>
                  <optgroup
                    v-for="group in providerGroups"
                    :key="group.provider"
                    :label="group.provider"
                  >
                    <option
                      v-for="opt in group.options"
                      :key="opt.slug"
                      :value="opt.slug"
                    >
                      {{ opt.label }}
                    </option>
                  </optgroup>
                </select>
                <Icon
                  name="chevron_d"
                  :size="14"
                  color="var(--ink-3)"
                  class="select-chevron"
                />
              </div>
            </div>
          </section>

          <section class="settings-section" aria-labelledby="sec-tokens">
            <h3 id="sec-tokens" class="section-eyebrow">API tokens</h3>
            <p class="section-note">
              Both are always required — the fixed pipeline stages use OpenAI and
              Anthropic regardless of the models you pick.
            </p>
            <div v-for="field in tokenFields" :key="field.key" class="field">
              <label class="label" :for="`cred-${field.key}`">{{ field.label }}</label>
              <input
                :id="`cred-${field.key}`"
                v-model="creds[field.key]"
                class="input mono"
                type="password"
                autocomplete="off"
                spellcheck="false"
                :placeholder="tokenPlaceholder(field.key)"
              />
              <ConnectionTestRow
                :configured="!!settings.credentials[field.key]"
                :dirty="credDirty(field.key)"
                :testing="testing[field.key]"
                :result="testResults[field.key]"
                @test="emit('test', field.key)"
              />
            </div>
          </section>

          <section class="settings-section" aria-labelledby="sec-creds">
            <h3 id="sec-creds" class="section-eyebrow">Data provider credentials</h3>
            <p class="section-note">
              Both are required to run a job. FRED, BLS, and GDELT are public and
              need no credential.
            </p>
            <div v-for="field in credentialFields" :key="field.key" class="field">
              <label class="label" :for="`cred-${field.key}`">{{ field.label }}</label>
              <input
                :id="`cred-${field.key}`"
                v-model="creds[field.key]"
                class="input mono"
                type="password"
                autocomplete="off"
                spellcheck="false"
                :placeholder="tokenPlaceholder(field.key)"
              />
              <ConnectionTestRow
                :configured="!!settings.credentials[field.key]"
                :dirty="credDirty(field.key)"
                :testing="testing[field.key]"
                :result="testResults[field.key]"
                @test="emit('test', field.key)"
              />
            </div>
          </section>

          <div v-if="error" class="settings-error" role="alert">
            <div class="settings-error-label">Couldn't save</div>
            <p class="settings-error-detail">{{ error }}</p>
          </div>

          <div class="settings-actions">
            <button type="submit" class="btn btn-primary" :disabled="!canSave">
              {{ saving ? "Saving…" : "Save" }}
            </button>
            <span v-if="showSaved" class="save-status" role="status">
              <Icon name="check" :size="13" color="var(--ink-2)" />
              Saved
            </span>
            <span v-else-if="needsTokens" class="save-status save-status--hint">
              Add both API tokens to save.
            </span>
          </div>
        </form>
      </div>
    </div>
  </main>
</template>

<style scoped>
.settings-pane {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
  background: var(--paper);
}

/* Toolbar geometry matches the report/inbox panes so the views share a seam.
   min-height keeps that seam uniform whether or not a toolbar carries a button
   (the inbox's "Add files…" sets the reference height), so a button-less title
   gets the same top/bottom breathing room. */
.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 50px;
  padding: var(--s-3) var(--s-8);
  border-bottom: var(--border);
}

/* Surface title: stronger than the section eyebrows below it — 13px ink semibold
   (a deliberate step up from the 11px caption used for sub-headings). */
.toolbar-label {
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  font-weight: 600;
  color: var(--ink);
}

.settings-scroll {
  flex: 1;
  overflow-y: auto;
}

.settings-body {
  max-width: 640px;
  padding: var(--s-7) var(--s-8) var(--s-12);
}

/* The lead section sits directly under the toolbar seam, so it drops the section
   rule + top padding that separate the stacked sections below it. Compound
   selector so it beats `.settings-section` regardless of source order. */
.settings-section.settings-section--lead {
  border-top: 0;
  padding-top: 0;
}

.settings-status {
  margin: 0;
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  color: var(--ink-3);
}

.settings-section {
  padding-top: var(--s-7);
  margin-bottom: var(--s-7);
  border-top: var(--border);
}

.section-eyebrow {
  margin: 0 0 var(--s-3);
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  font-weight: 600;
  color: var(--ink);
}

.section-note {
  margin: 0 0 var(--s-6);
  font-family: var(--font-serif);
  font-style: italic;
  font-size: var(--t-ui-sm);
  line-height: var(--lh-prose);
  /* ink-2, not ink-3: at 13px this secondary prose must clear WCAG AA (4.5:1);
     ink-3 on paper is ~4.3:1. */
  color: var(--ink-2);
}

.field {
  margin-bottom: var(--s-6);
}

.field:last-child {
  margin-bottom: 0;
}

/* The <label> uses the global `.label` token; only spacing is nudged here so
   the field reads as label-over-control. */
.field :deep(.label) {
  margin-bottom: var(--s-2);
}

/* Native <select> isn't styled by the design system; this extends the open
   bottom-border `.input` idiom to it (transparent field, hairline underline,
   accent on focus) with the kit's chevron glyph overlaid. Noted as a system
   extension in the scope report. */
.select-wrap {
  position: relative;
}

.field-select {
  display: block;
  width: 100%;
  appearance: none;
  -webkit-appearance: none;
  padding: var(--s-3) var(--s-7) var(--s-3) 0;
  background: transparent;
  border: 0;
  border-bottom: 1px solid var(--ink);
  border-radius: 0;
  font-family: var(--font-sans);
  font-size: var(--t-ui);
  color: var(--ink);
  cursor: pointer;
  transition: border-color var(--dur-fast) var(--ease);
}

.field-select:focus {
  outline: none;
  border-bottom-color: var(--accent);
  box-shadow: 0 1px 0 0 var(--accent);
}

.select-chevron {
  position: absolute;
  right: 0;
  top: 50%;
  transform: translateY(-50%);
  pointer-events: none;
}

/* Mono, tabular figures for the credential fields (long opaque keys). */
.mono {
  font-family: var(--font-mono);
  font-variant-numeric: tabular-nums lining-nums;
}

.settings-error {
  margin: var(--s-2) 0 var(--s-6);
}

.settings-error-label {
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  font-weight: 600;
  color: var(--accent);
  margin-bottom: var(--s-3);
}

.settings-error-detail {
  margin: 0;
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  line-height: var(--lh-ui);
  color: var(--ink-2);
  overflow-wrap: anywhere;
}

.settings-actions {
  display: flex;
  align-items: center;
  gap: var(--s-5);
  padding-top: var(--s-3);
  border-top: var(--border);
}

.save-status {
  display: inline-flex;
  align-items: center;
  gap: var(--s-2);
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  color: var(--ink-2);
}

/* Why Save is disabled — a muted hint, not an error (it's a gating affordance). */
.save-status--hint {
  color: var(--ink-3);
}

/* Schedule control row — label/hint on the left, switch on the right. */
.control-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--s-5);
}

.control-text {
  min-width: 0;
}

.control-label {
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  font-weight: 500;
  color: var(--ink);
}

.control-hint {
  font-family: var(--font-serif);
  font-style: italic;
  font-size: var(--t-caption);
  color: var(--ink-2);
  line-height: var(--lh-ui);
}

/* Boxy switch (design kit Settings.jsx Toggle): 44×22, 1px ink edge, 2px radius,
   a sliding ink block — no pill, no rounded slider. A <button> so it is
   keyboard-operable (Enter/Space) with a focus ring. This is the toggle's only
   instance now that the footer no longer carries one. */
.switch {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  width: 44px;
  height: 22px;
  padding: 2px;
  border: 1px solid var(--ink);
  border-radius: var(--radius);
  background: transparent;
  cursor: pointer;
}

/* The knob is always filled — muted ink-3 when off, solid ink when on — so the
   control reads as a switch in both states (a transparent off-knob looked like
   an empty box). State = fill weight + position. */
.switch-knob {
  width: 18px;
  height: 16px;
  border-radius: var(--radius-sm);
  background: var(--ink-3);
  margin-left: 0;
  transition: margin-left var(--dur-fast) var(--ease),
    background-color var(--dur-fast) var(--ease);
}

.switch-knob--on {
  background: var(--ink);
  margin-left: 20px;
}

.switch:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 1px;
}

.switch:disabled {
  cursor: not-allowed;
  border-color: var(--hairline);
}

.switch:disabled .switch-knob {
  background: var(--hairline);
}

@media (prefers-reduced-motion: reduce) {
  .field-select,
  .switch-knob {
    transition: none;
  }
}
</style>
