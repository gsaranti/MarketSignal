<script setup lang="ts">
import { ref, computed, watch } from "vue";
import Icon from "./Icon.vue";
import ConnectionTestRow from "./ConnectionTestRow.vue";
import { localDate } from "../format";
import type {
  SettingsView,
  AgentModels,
  ConnectionTestResult,
  CredentialUpdate,
  ModelOption,
  SchwabStatus,
  SchwabCredentialUpdate,
  TruncationStats,
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
  // Appearance (Light/Dark) — applied + persisted by App via ./theme; this just
  // drives the switch state. Independent of the config form's gated Save.
  dark: boolean;
  // Per-credential "Test connection" state, owned by App (the single invoke
  // home): which credential is mid-test, and the last result for each.
  testing: Record<CredKey, boolean>;
  testResults: Record<CredKey, ConnectionTestResult | null>;
  // Read-only truncation telemetry for the diagnostics section. `null` =
  // unavailable / not yet loaded (the section is omitted); a populated all-zero
  // aggregate renders the "none recorded" empty state.
  truncationStats: TruncationStats | null;
  // Charles Schwab connection (docs/schwab-integration.md, docs/interface.md
  // §Connection status). `null` = not yet loaded / unavailable (the section is
  // omitted, like diagnostics). `schwabConnecting` is true while the interactive
  // browser login is in flight; `schwabBusy` is true when any run holds the global
  // slot (so Connect — which takes that slot — is disabled). `schwabError` carries a
  // connect/save failure.
  schwabStatus: SchwabStatus | null;
  schwabConnecting: boolean;
  schwabBusy: boolean;
  schwabError: string | null;
  // Data portability (docs/data-portability.md): whole-corpus export/import.
  // `dataBusy` names which operation is in flight (null = idle); `slotBusy` is
  // true when any run holds the global slot (export/import take it too). Status
  // and error are App-owned channels on the Schwab-section pattern.
  dataBusy: "export" | "import" | null;
  slotBusy: boolean;
  dataError: string | null;
  dataStatus: string | null;
}>();

const emit = defineEmits<{
  (e: "save", payload: { models: AgentModels; credentials: CredentialUpdate }): void;
  (e: "set-dark", value: boolean): void;
  (e: "test", key: CredKey): void;
  (e: "save-schwab", payload: SchwabCredentialUpdate): void;
  (e: "connect-schwab"): void;
  (e: "disconnect-schwab"): void;
  (e: "export-data", passphrase: string): void;
  (e: "import-data", passphrase: string): void;
}>();

type ModelKey = "main" | "bull" | "bear" | "balanced";
type CredKey = "openai" | "anthropic" | "fmp" | "fred" | "tavily";

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
  { key: "fred", label: "FRED" },
  { key: "tavily", label: "Tavily" },
];

// Local, editable form state. Models pre-select the saved slugs; credential
// inputs always start empty (the secret is never sent to the webview).
const local = ref<AgentModels>({ main: "", bull: "", bear: "", balanced: "" });
const creds = ref<Record<CredKey, string>>({
  openai: "",
  anthropic: "",
  fmp: "",
  fred: "",
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
    creds.value = { openai: "", anthropic: "", fmp: "", fred: "", tavily: "" };
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
// The docs scope this gate to the agent/token submission; ungating the
// provider-credential save is a named prerequisite of the local-suite
// Settings slice (docs/configuration.md §API Tokens).
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

// Appearance toggle — flips and applies instantly (App owns the apply+persist).
function toggleDark() {
  emit("set-dark", !props.dark);
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
    fred: creds.value.fred.trim() ? creds.value.fred : null,
    tavily: creds.value.tavily.trim() ? creds.value.tavily : null,
  };
  emit("save", { models: { ...local.value }, credentials: credUpdate });
}

// Whether a credential field holds a typed (unsaved) value — drives the test
// row's gating (it validates the saved credential, not what's typed).
function credDirty(key: CredKey): boolean {
  return creds.value[key].trim() !== "";
}

// Whether any truncation has ever been recorded — a populated aggregate with a
// zero count renders the "none recorded" empty state instead of the readout.
const hasTruncations = computed(
  () => (props.truncationStats?.total_truncations ?? 0) > 0
);

// Group an integer with thousands separators, locale-independent so the readout
// is deterministic across environments (and tabular-figure aligned per the
// design system's numeric idiom).
function fmtNum(n: number): string {
  return String(n).replace(/\B(?=(\d{3})+(?!\d))/g, ",");
}

// The truncation rate as a "X of Y (Z%)" readout — the numerator over its
// parsed-documents denominator. Falls back to the bare count whenever the
// denominator can't support an honest rate:
//   - missing (0) — would render a nonsensical "of 0";
//   - smaller than the numerator — would render an over-100% rate (the two are
//     written on independent best-effort channels, so a truncation can briefly
//     lead its parse-run);
//   - incomplete — some truncations predate the denominator (`unaligned_truncations`),
//     so a rate over them would mix cohorts. Self-heals as those rows age out.
// Only consulted inside the `hasTruncations` block, so the numerator is > 0 here.
const truncatedDocsValue = computed(() => {
  const s = props.truncationStats;
  if (!s) return "";
  const truncated = fmtNum(s.total_truncations);
  if (
    s.total_docs_parsed <= 0 ||
    s.total_truncations > s.total_docs_parsed ||
    s.unaligned_truncations > 0
  ) {
    return truncated;
  }
  const pct = (s.total_truncations / s.total_docs_parsed) * 100;
  return `${truncated} of ${fmtNum(s.total_docs_parsed)} (${pct.toFixed(1)}%)`;
});

// The chars-dropped ratio as a "X of Y (Z%)" readout — chars cut over total
// original chars across all parsed docs (the share of ingested text lost to
// truncation). Mirrors `truncatedDocsValue`: falls back to the bare count
// whenever the denominator can't support an honest ratio:
//   - missing (0) — would render a nonsensical "of 0";
//   - smaller than the numerator — would render an over-100% ratio (the two are
//     written on independent best-effort channels, so a drop can briefly lead
//     its parse-run);
//   - incomplete — the numerator counts chars the denominator never did. Two
//     distinct cohort gaps both produce this, and both must withhold the ratio:
//       · a truncation whose report has no parse-run row at all
//         (`unaligned_truncations` — the same gap that withholds the doc rate;
//         that report's original chars are absent from `total_original_chars`
//         while its dropped chars are in `total_chars_dropped`), and
//       · a parse-run row that predates the chars column
//         (`parse_runs_missing_original_chars`, NULL → skipped by SUM).
//     Self-heals as those legacy rows age out of the retention window.
// Only consulted inside the `hasTruncations` block, so the numerator is > 0 here.
const charsDroppedValue = computed(() => {
  const s = props.truncationStats;
  if (!s) return "";
  const dropped = fmtNum(s.total_chars_dropped);
  if (
    s.total_original_chars <= 0 ||
    s.total_chars_dropped > s.total_original_chars ||
    s.unaligned_truncations > 0 ||
    s.parse_runs_missing_original_chars > 0
  ) {
    return dropped;
  }
  const pct = (s.total_chars_dropped / s.total_original_chars) * 100;
  return `${dropped} of ${fmtNum(s.total_original_chars)} (${pct.toFixed(1)}%)`;
});

// --- Charles Schwab connection ---------------------------------------------
// The developer-app credentials + the connect/disconnect controls. Local, editable
// form state mirrors the credential fields: the non-secret client_id round-trips its
// saved value, while the secret input starts empty and is write-only (its stored
// presence shows only as a placeholder).
const schwabClientId = ref("");
const schwabSecret = ref("");

// (Re)seed the form whenever a fresh status arrives — on load and after a
// save/connect/disconnect (App re-fetches). Clears the typed secret and resets the
// dirty baseline to what was just persisted.
watch(
  () => props.schwabStatus,
  (s) => {
    if (!s) return;
    schwabClientId.value = s.client_id;
    schwabSecret.value = "";
  },
  { immediate: true }
);

// The secret placeholder mirrors the API-token idiom: a stored secret shows as saved
// (type to replace), an absent one as "Not set".
const schwabSecretPlaceholder = computed(() =>
  props.schwabStatus?.secret_configured
    ? "•••• saved — type a new value to replace"
    : "Not set"
);

// Unsaved edits: a changed client_id, or any typed secret. Connect reads the *saved*
// credentials, so an edit must be saved before connecting (mirrors the credential
// "save before testing" rule).
const schwabDirty = computed(() => {
  const saved = props.schwabStatus?.client_id ?? "";
  return schwabClientId.value.trim() !== saved || schwabSecret.value.trim() !== "";
});

// Saving needs a non-empty client_id and something actually changed — and is blocked
// while a connect or run holds the slot. That guards the mid-login race: `schwab_connect`
// captures the client_id up front but reads the secret from the Keychain later, so a save
// during the login could pair the old id with a new secret and fail the token exchange.
const canSaveSchwab = computed(
  () =>
    schwabClientId.value.trim() !== "" &&
    schwabDirty.value &&
    !props.schwabConnecting &&
    !props.schwabBusy
);

// Both credentials present on the backend — the precondition for a connect attempt
// (the loopback reads them from storage / Keychain).
const schwabCredsConfigured = computed(
  () =>
    !!props.schwabStatus &&
    props.schwabStatus.client_id.trim() !== "" &&
    props.schwabStatus.secret_configured
);

// Connect is offered only when the saved credentials are complete, the form has no
// unsaved edits, and no run holds the global slot (Connect takes it — see App).
const canConnect = computed(
  () =>
    schwabCredsConfigured.value &&
    !schwabDirty.value &&
    !props.schwabConnecting &&
    !props.schwabBusy
);

// Why Connect is unavailable, surfaced as its title (frontend-craft: never a dead
// control with no explanation).
const connectTitle = computed(() => {
  if (props.schwabConnecting) return "Completing the Schwab login…";
  if (!schwabCredsConfigured.value)
    return "Enter and save your Schwab client ID and secret first";
  if (schwabDirty.value) return "Save your credential changes before connecting";
  if (props.schwabBusy) return "Another job is running — connect once it finishes";
  return "Open the Schwab login in your browser to connect";
});

const schwabConnection = computed(
  () => props.schwabStatus?.connection ?? "not-connected"
);

// Disconnect is shown only when a session exists (connected or lapsed); it clears the
// tokens but keeps the saved credentials.
const showDisconnect = computed(() => schwabConnection.value !== "not-connected");
const canDisconnect = computed(
  () => !props.schwabConnecting && !props.schwabBusy
);

// The connection status line — plain, declarative copy (the design system's voice),
// with the weekly-re-login date as a heads-up when connected.
const schwabStatusText = computed(() => {
  switch (schwabConnection.value) {
    case "connected": {
      const at = props.schwabStatus?.refresh_expires_at;
      return at
        ? `Connected. Weekly re-login by ${localDate(at)}.`
        : "Connected.";
    }
    case "expired":
      return "Connection expired — reconnect to continue. The 7-day refresh window has lapsed.";
    default:
      return "Not connected.";
  }
});

// The status line's tone class, reusing ConnectionTestRow's status vocabulary
// (ink-2 + check for a live connection; accent-text for a lapsed one; ink-3 for the
// neutral not-connected state) — no new tokens, no analytical palette.
const schwabStatusClass = computed(() => {
  switch (schwabConnection.value) {
    case "connected":
      return "schwab-status--ok";
    case "expired":
      return "schwab-status--err";
    default:
      return "schwab-status--pending";
  }
});

// The connect control's label: first-time "Connect", else "Reconnect" (the weekly
// re-login, or re-running after a lapse).
const connectLabel = computed(() => {
  if (props.schwabConnecting) return "Connecting…";
  return schwabConnection.value === "not-connected" ? "Connect" : "Reconnect";
});

function onSaveSchwab() {
  if (!canSaveSchwab.value) return;
  emit("save-schwab", {
    client_id: schwabClientId.value.trim(),
    // Null (not "") leaves the stored secret untouched — the write-only field idiom.
    client_secret: schwabSecret.value.trim() ? schwabSecret.value : null,
  });
}

// --- Data portability --------------------------------------------------------
// One optional passphrase field serves both directions: export encrypts with
// it (blank = plaintext archive), import decrypts an encrypted archive with it.
// Never stored, never round-tripped.
const dataPassphrase = ref("");

const canUseData = computed(() => props.dataBusy === null && !props.slotBusy);

// Why the action is unavailable, as its title (the connectTitle idiom: never a
// dead control with no explanation).
const exportDataTitle = computed(() => {
  if (props.dataBusy) return "An export or import is already in progress";
  if (props.slotBusy) return "Another job is running — export once it finishes";
  return "Choose where to save the archive";
});
const importDataTitle = computed(() => {
  if (props.dataBusy) return "An export or import is already in progress";
  if (props.slotBusy) return "Another job is running — import once it finishes";
  return "Choose an archive to restore from";
});

const exportDataLabel = computed(() =>
  props.dataBusy === "export" ? "Exporting…" : "Export archive…"
);
const importDataLabel = computed(() =>
  props.dataBusy === "import" ? "Importing…" : "Import archive…"
);
</script>

<template>
  <main class="settings-pane">
    <div class="toolbar">
      <div class="toolbar-label">Settings</div>
      <!-- Appearance toggle is hosted in the toolbar (a utility/chrome control),
           deliberately apart from the gated config form below — so it reads as
           instant-applying chrome, not a field governed by the form's Save. Always
           rendered with the toolbar, so it works even if settings fail to load. -->
      <div class="toolbar-appearance">
        <span id="appearance-label" class="toolbar-appearance-label">Dark surface</span>
        <button
          type="button"
          class="switch"
          role="switch"
          :aria-checked="dark"
          aria-labelledby="appearance-label"
          @click="toggleDark"
        >
          <span class="switch-knob" :class="{ 'switch-knob--on': dark }"></span>
        </button>
      </div>
    </div>

    <div class="settings-scroll">
      <div class="settings-body">
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
          <!-- First section under the toolbar now that Appearance moved up: take the
               lead treatment (no top rule / top padding) so it sits flush at the seam. -->
          <section
            class="settings-section settings-section--lead"
            aria-labelledby="sec-models"
          >
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
              Financial Modeling Prep, FRED, and Tavily are all required to run a
              job. FRED needs a free API key; BLS, GDELT, and CFTC need no credential.
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
            <button
              type="submit"
              class="btn btn-primary"
              :disabled="!canSave"
              :aria-describedby="needsTokens ? 'save-needs-tokens' : undefined"
            >
              {{ saving ? "Saving…" : "Save" }}
            </button>
            <span v-if="showSaved" class="save-status" role="status">
              <Icon name="check" :size="13" color="var(--ink-2)" />
              Saved
            </span>
            <span
              v-else-if="needsTokens"
              id="save-needs-tokens"
              class="save-status save-status--hint"
              role="status"
            >
              Add both API tokens to save.
            </span>
          </div>
        </form>

        <!-- Charles Schwab connection (docs/schwab-integration.md, docs/interface.md
             §Connection status). Its own section outside the config form — its
             Save/Connect/Disconnect are independent of the gated model+credential
             Save above, and it renders on its own status channel (like diagnostics),
             so it appears even if the cloud settings fail to load. Omitted while the
             status is unavailable. Generic-chrome register: monochrome, no analytical
             palette. -->
        <section
          v-if="schwabStatus"
          class="settings-section"
          aria-labelledby="sec-schwab"
        >
          <h3 id="sec-schwab" class="section-eyebrow">Charles Schwab connection</h3>
          <p class="section-note">
            Portfolio Analysis and Trade Opportunities read your holdings and option
            chains from Schwab. Enter your developer-app client ID and secret, then
            connect — a browser login you repeat weekly, when the 7-day session lapses.
          </p>

          <div class="field">
            <label class="label" for="schwab-client-id">Client ID</label>
            <input
              id="schwab-client-id"
              v-model="schwabClientId"
              class="input mono"
              type="text"
              autocomplete="off"
              spellcheck="false"
              placeholder="Not set"
            />
          </div>
          <div class="field">
            <label class="label" for="schwab-client-secret">Client secret</label>
            <input
              id="schwab-client-secret"
              v-model="schwabSecret"
              class="input mono"
              type="password"
              autocomplete="off"
              spellcheck="false"
              :placeholder="schwabSecretPlaceholder"
            />
          </div>

          <!-- Persistent live region (node stays mounted, text changes) so a screen
               reader announces connection changes reliably. -->
          <p
            class="schwab-status"
            :class="schwabStatusClass"
            role="status"
            aria-live="polite"
          >
            <Icon
              v-if="schwabConnection === 'connected'"
              name="check"
              :size="13"
              color="var(--ink-2)"
            />
            {{ schwabStatusText }}
          </p>

          <p v-if="schwabConnecting" class="schwab-hint" role="status">
            Complete the Schwab login in your browser. After the login, a
            certificate warning for the local callback (127.0.0.1) is expected —
            that page is this app's own listener. In Safari, choose Show Details,
            then "visit this website" to finish connecting.
          </p>

          <div v-if="schwabError" class="settings-error" role="alert">
            <div class="settings-error-label">Couldn't update Schwab</div>
            <p class="settings-error-detail">{{ schwabError }}</p>
          </div>

          <div class="schwab-actions">
            <button
              type="button"
              class="btn btn-secondary"
              :disabled="!canSaveSchwab"
              @click="onSaveSchwab"
            >
              Save credentials
            </button>
            <button
              type="button"
              class="btn btn-primary"
              :disabled="!canConnect"
              :title="connectTitle"
              @click="emit('connect-schwab')"
            >
              {{ connectLabel }}
            </button>
            <button
              v-if="showDisconnect"
              type="button"
              class="btn btn-secondary"
              :disabled="!canDisconnect"
              @click="emit('disconnect-schwab')"
            >
              Disconnect
            </button>
          </div>
        </section>

        <!-- Data portability (docs/data-portability.md): whole-corpus
             backup/restore. Its own section outside the config form on the Schwab
             pattern — own props, own error/status channels, an action row
             independent of the gated Save — and always rendered (it needs no
             loaded settings; the store itself is the subject). The destructive
             import path is confirmed by App's ConfirmDialog, not here. -->
        <section class="settings-section" aria-labelledby="sec-data">
          <h3 id="sec-data" class="section-eyebrow">Data</h3>
          <p class="section-note">
            Export every report, learning, snapshot, and portfolio run as one
            archive, or restore from one — for a new machine or an offline
            backup. API keys and settings never enter the archive.
          </p>

          <div class="field">
            <label class="label" for="data-passphrase">Passphrase (optional)</label>
            <input
              id="data-passphrase"
              v-model="dataPassphrase"
              class="input mono"
              type="password"
              autocomplete="off"
              spellcheck="false"
              placeholder="Leave blank for an unencrypted archive"
            />
          </div>

          <p class="section-note data-caution">
            An unencrypted archive is your full analysis history in the clear —
            keep the file private. An encrypted one is unrecoverable without its
            passphrase; there is no reset.
          </p>

          <p v-if="dataStatus" class="save-status data-status" role="status">
            <Icon name="check" :size="13" color="var(--ink-2)" />
            {{ dataStatus }}
          </p>

          <div v-if="dataError" class="settings-error" role="alert">
            <div class="settings-error-label">Couldn't move data</div>
            <p class="settings-error-detail">{{ dataError }}</p>
          </div>

          <div class="data-actions">
            <button
              type="button"
              class="btn btn-secondary"
              :disabled="!canUseData"
              :title="exportDataTitle"
              @click="emit('export-data', dataPassphrase)"
            >
              {{ exportDataLabel }}
            </button>
            <button
              type="button"
              class="btn btn-secondary"
              :disabled="!canUseData"
              :title="importDataTitle"
              @click="emit('import-data', dataPassphrase)"
            >
              {{ importDataLabel }}
            </button>
          </div>
        </section>

        <!-- Diagnostics: read-only truncation telemetry (docs/agents.md §Data
             Extraction). Renders independent of the config form's load state and
             on its own data channel, so it appears even when settings fail to
             load. Omitted entirely while the aggregate is unavailable; a populated
             all-zero aggregate is the "overflow is rare" empty state. -->
        <section
          v-if="truncationStats"
          class="settings-section"
          aria-labelledby="sec-diagnostics"
        >
          <h3 id="sec-diagnostics" class="section-eyebrow">Document truncations</h3>
          <p class="section-note">
            What share of parsed research documents were oversized enough to be
            head-truncated during parsing. Accumulates across reports.
          </p>

          <template v-if="hasTruncations">
            <dl class="trunc-stats">
              <div class="trunc-row">
                <dt>Documents truncated</dt>
                <dd>{{ truncatedDocsValue }}</dd>
              </div>
              <div class="trunc-row">
                <dt>Reports affected</dt>
                <dd>{{ fmtNum(truncationStats.reports_affected) }}</dd>
              </div>
              <div class="trunc-row">
                <dt>Characters dropped</dt>
                <dd>{{ charsDroppedValue }}</dd>
              </div>
              <div v-if="truncationStats.latest_captured_at" class="trunc-row">
                <dt>Most recent</dt>
                <dd>{{ localDate(truncationStats.latest_captured_at) }}</dd>
              </div>
            </dl>

            <div v-if="truncationStats.by_format.length" class="trunc-formats">
              <span class="trunc-formats-label">By format</span>
              <ul class="trunc-format-list">
                <li
                  v-for="f in truncationStats.by_format"
                  :key="f.format"
                  class="trunc-format-item"
                >
                  <span class="trunc-format-name">{{ f.format }}</span>
                  <span class="trunc-format-count">{{ fmtNum(f.count) }}</span>
                </li>
              </ul>
            </div>
          </template>

          <p v-else class="trunc-empty">
            No truncations recorded yet — research documents have fit within the
            parser's limits.
          </p>
        </section>
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
  /* Yield to the appearance toggle on a narrow pane rather than pushing it off. */
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
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
  color: var(--accent-text);
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
  /* Match the section rhythm below the button so the next section's top rule
     (e.g. Document truncations) doesn't hug the Save button's bottom edge. */
  margin-bottom: var(--s-7);
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

/* Schwab connection status line — reuses ConnectionTestRow's status vocabulary
   (no new tokens, no analytical palette; this is generic chrome): ink-3 for the
   neutral not-connected state, ink-2 (+ check) for a live connection, accent-text
   for a lapsed one. */
.schwab-status {
  display: flex;
  align-items: center;
  gap: var(--s-2);
  margin: var(--s-2) 0 0;
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  line-height: var(--lh-ui);
  overflow-wrap: anywhere;
}

.schwab-status--pending {
  color: var(--ink-3);
}

/* ink-2 (not ink-3) to clear AA at 13px, matching the Save "Saved" confirmation. */
.schwab-status--ok {
  color: var(--ink-2);
}

.schwab-status--err {
  color: var(--accent-text);
}

/* The while-connecting heads-up (browser login + one-time cert warning): a quiet
   informational line, not an error. */
.schwab-hint {
  margin: var(--s-3) 0 0;
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  line-height: var(--lh-ui);
  color: var(--ink-3);
}

/* Section-local action row — save / connect / disconnect. Unlike `.settings-actions`
   it carries no top rule (it sits mid-section, not at the form's foot). */
.schwab-actions,
.data-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--s-4);
  margin-top: var(--s-6);
}

/* The passphrase caution reads in the section-note voice but sits *after* its
   field, so it drops the note's bottom rhythm in favor of a tight top one. */
.data-caution {
  margin: var(--s-2) 0 var(--s-5);
}

/* The export success line carries the archive's absolute path — a long
   unbroken string that must wrap inside the column, not overflow it. */
.data-status {
  align-items: baseline;
  overflow-wrap: anywhere;
  min-width: 0;
}

/* Appearance toggle hosted in the toolbar — a compact label + the kit switch,
   right-aligned opposite the surface title. flex-shrink:0 so it never collapses;
   the title truncates first on a narrow pane (see .toolbar-label). */
.toolbar-appearance {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: var(--s-3);
}

.toolbar-appearance-label {
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  color: var(--ink-2);
  white-space: nowrap;
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

/* Diagnostics readout: a label/value list in mono tabular figures (the system's
   numeric idiom), flat and hairline-free — it's a quiet telemetry block, not a
   card. */
.trunc-stats {
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: var(--s-3);
}

.trunc-row {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: var(--s-5);
}

/* Label idiom matches the sibling .trunc-formats-label so the diagnostics block
   reads as one system (was 13px sentence-case, an in-block inconsistency). */
.trunc-row dt {
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  font-weight: 600;
  color: var(--ink-3);
}

.trunc-row dd {
  margin: 0;
  font-family: var(--font-mono);
  font-variant-numeric: tabular-nums lining-nums;
  font-size: var(--t-ui-sm);
  color: var(--ink);
}

.trunc-formats {
  display: flex;
  align-items: baseline;
  gap: var(--s-4);
  margin-top: var(--s-5);
}

.trunc-formats-label {
  flex-shrink: 0;
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  font-weight: 600;
  color: var(--ink-3);
}

.trunc-format-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-wrap: wrap;
  gap: var(--s-2) var(--s-5);
}

.trunc-format-item {
  display: inline-flex;
  align-items: baseline;
  gap: var(--s-2);
  font-family: var(--font-mono);
  font-variant-numeric: tabular-nums lining-nums;
  font-size: var(--t-ui-sm);
}

.trunc-format-name {
  color: var(--ink-2);
}

.trunc-format-count {
  color: var(--ink);
}

/* Empty state mirrors the section note's serif-italic voice (ink-2 for AA at
   13px). */
.trunc-empty {
  margin: 0;
  font-family: var(--font-serif);
  font-style: italic;
  font-size: var(--t-ui-sm);
  line-height: var(--lh-prose);
  color: var(--ink-2);
}

@media (prefers-reduced-motion: reduce) {
  .field-select,
  .switch-knob {
    transition: none;
  }
}
</style>
