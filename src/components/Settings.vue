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
  InvestorProfileDisplay,
  LocalDaemonStatus,
  LocalModelSettings,
  ModelOption,
  ProviderCredentialUpdate,
  SchwabStatus,
  SchwabCredentialUpdate,
  TruncationStats,
  WebResearchPreflight,
  WebResearchSettings,
} from "../types";

// Configuration surface (docs/configuration.md, docs/interface.md §Settings),
// translated from the design kit's Settings.jsx. Presentational: App owns the
// state and the save commands. Secrets never arrive here: a credential shows
// only as configured or not, and an untouched field is left unchanged on save.
// The submissions are independent (docs/configuration.md §API Tokens): only
// the cloud form is token-gated, so a cloud-keyless machine completes
// local-suite setup.
const props = defineProps<{
  settings: SettingsView | null;
  loading: boolean;
  saving: boolean;
  error: string | null;
  // Applied and persisted by App; independent of the gated Save.
  dark: boolean;
  testing: Record<CredKey, boolean>;
  testResults: Record<CredKey, ConnectionTestResult | null>;
  savingProviders: boolean;
  providersError: string | null;
  // `localDaemon` is the last probe result, null until tested
  // (docs/interface.md §Connection status).
  savingLocal: boolean;
  localError: string | null;
  localTesting: boolean;
  localDaemon: LocalDaemonStatus | null;
  // `searxngStatus` is the last probe result, null until tested; SearXNG is
  // off every gate (docs/configuration.md §Web Research).
  savingWebResearch: boolean;
  webResearchError: string | null;
  searxngTesting: boolean;
  searxngStatus: WebResearchPreflight | null;
  // `null` omits the section; an all-zero aggregate renders the empty state.
  truncationStats: TruncationStats | null;
  // docs/configuration.md §Investor Profile. `null` omits the section; the
  // strings arrive ready-to-render from the backend.
  investorProfile: InvestorProfileDisplay | null;
  // docs/schwab-integration.md. `null` omits the section. `schwabBusy`: any
  // run holds the global slot, which Connect also takes.
  schwabStatus: SchwabStatus | null;
  schwabConnecting: boolean;
  schwabBusy: boolean;
  schwabError: string | null;
  // docs/data-portability.md. `slotBusy`: any run holds the global slot, which
  // export/import also take.
  dataBusy: "export" | "import" | null;
  slotBusy: boolean;
  dataError: string | null;
  dataStatus: string | null;
}>();

const emit = defineEmits<{
  (e: "save", payload: { models: AgentModels; credentials: CredentialUpdate }): void;
  (e: "save-providers", payload: ProviderCredentialUpdate): void;
  (e: "save-local", payload: LocalModelSettings): void;
  (e: "test-local"): void;
  (e: "save-web-research", payload: WebResearchSettings): void;
  (e: "test-searxng"): void;
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

const local = ref<AgentModels>({ main: "", bull: "", bear: "", balanced: "" });
const creds = ref<Record<CredKey, string>>({
  openai: "",
  anthropic: "",
  fmp: "",
  fred: "",
  tavily: "",
});
const localModels = ref<LocalModelSettings>({
  daemon_endpoint: "",
  reasoner_model: "",
  fast_model: "",
});
const webResearch = ref<WebResearchSettings>({ searxng_endpoint: "" });
const justSaved = ref(false);
const providersJustSaved = ref(false);
const localJustSaved = ref(false);
const webResearchJustSaved = ref(false);

// Seed the form only when the view first arrives. App re-fetches after every
// save, and a reload must not reset form state: saving one section must never
// discard another section's unsaved edits. Each save clears its own secret
// fields on its completion edge below.
watch(
  () => props.settings,
  (s, prev) => {
    if (!s || prev) return;
    local.value = { ...s.models };
    creds.value = { openai: "", anthropic: "", fmp: "", fred: "", tavily: "" };
    localModels.value = { ...s.local_models };
    webResearch.value = { ...s.web_research };
  },
  { immediate: true }
);

// A clean completion clears that submission's own typed fields: a persisted
// secret must not linger in its input.
watch(
  () => props.saving,
  (now, was) => {
    if (now && !was) justSaved.value = false;
    if (was && !now && props.error === null) {
      justSaved.value = true;
      creds.value = { ...creds.value, openai: "", anthropic: "" };
    }
  }
);
watch(
  () => props.savingProviders,
  (now, was) => {
    if (now && !was) providersJustSaved.value = false;
    if (was && !now && props.providersError === null) {
      providersJustSaved.value = true;
      creds.value = { ...creds.value, fmp: "", fred: "", tavily: "" };
    }
  }
);
watch(
  () => props.savingLocal,
  (now, was) => {
    if (now && !was) localJustSaved.value = false;
    if (was && !now && props.localError === null) {
      localJustSaved.value = true;
      // The backend persists trimmed values; adopt them so the form reads clean.
      localModels.value = {
        daemon_endpoint: localModels.value.daemon_endpoint.trim(),
        reasoner_model: localModels.value.reasoner_model.trim(),
        fast_model: localModels.value.fast_model.trim(),
      };
    }
  }
);
watch(
  () => props.savingWebResearch,
  (now, was) => {
    if (now && !was) webResearchJustSaved.value = false;
    if (was && !now && props.webResearchError === null) {
      webResearchJustSaved.value = true;
      webResearch.value = {
        searxng_endpoint: webResearch.value.searxng_endpoint.trim(),
      };
    }
  }
);

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

const tokensTyped = computed(
  () => creds.value.openai.trim() !== "" || creds.value.anthropic.trim() !== ""
);
const providersDirty = computed(
  () =>
    creds.value.fmp.trim() !== "" ||
    creds.value.fred.trim() !== "" ||
    creds.value.tavily.trim() !== ""
);

const dirty = computed(() => modelsDirty.value || tokensTyped.value);

// Both API tokens, stored or typed (docs/configuration.md §API Tokens); the
// backend enforces the same gate.
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

const canSaveProviders = computed(
  () => !!props.settings && !props.savingProviders && providersDirty.value
);

function toggleDark() {
  emit("set-dark", !props.dark);
}

const showSaved = computed(
  () => justSaved.value && !dirty.value && !props.saving && props.error === null
);
const showProvidersSaved = computed(
  () =>
    providersJustSaved.value &&
    !providersDirty.value &&
    !props.savingProviders &&
    props.providersError === null
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
  };
  emit("save", { models: { ...local.value }, credentials: credUpdate });
}

function onSaveProviders() {
  if (!canSaveProviders.value) return;
  emit("save-providers", {
    fmp: creds.value.fmp.trim() ? creds.value.fmp : null,
    fred: creds.value.fred.trim() ? creds.value.fred : null,
    tavily: creds.value.tavily.trim() ? creds.value.tavily : null,
  });
}

// --- Local analysis models ---------------------------------------------------
// docs/configuration.md §Local Models. Presence of endpoint + reasoner is what
// clears the "local models not configured" warning.

const localDirty = computed(() => {
  const s = props.settings?.local_models;
  if (!s) return false;
  const m = localModels.value;
  return (
    m.daemon_endpoint !== s.daemon_endpoint ||
    m.reasoner_model !== s.reasoner_model ||
    m.fast_model !== s.fast_model
  );
});

const canSaveLocal = computed(
  () => !!props.settings && !props.savingLocal && localDirty.value
);

const showLocalSaved = computed(
  () =>
    localJustSaved.value &&
    !localDirty.value &&
    !props.savingLocal &&
    props.localError === null
);

function onSaveLocal() {
  if (!canSaveLocal.value) return;
  emit("save-local", { ...localModels.value });
}

// Test Connection probes the *saved* endpoint + roster, so unsaved edits block it.
const localEndpointConfigured = computed(
  () => (props.settings?.local_models.daemon_endpoint.trim() ?? "") !== ""
);
const canTestLocal = computed(
  () => localEndpointConfigured.value && !localDirty.value && !props.localTesting
);
const localTestTitle = computed(() => {
  if (props.localTesting) return "Testing…";
  if (!localEndpointConfigured.value)
    return "Save a daemon endpoint before testing";
  if (localDirty.value) return "Save your changes before testing";
  return "Check the daemon is reachable and the rostered models are pulled";
});

// --- Web research ------------------------------------------------------------
// docs/configuration.md §Web Research. Off every gate: unreachable degrades a
// run behind a pre-run notice, never a lock or a warning category.

const webResearchDirty = computed(() => {
  const s = props.settings?.web_research;
  if (!s) return false;
  return webResearch.value.searxng_endpoint !== s.searxng_endpoint;
});

const canSaveWebResearch = computed(
  () => !!props.settings && !props.savingWebResearch && webResearchDirty.value
);

const showWebResearchSaved = computed(
  () =>
    webResearchJustSaved.value &&
    !webResearchDirty.value &&
    !props.savingWebResearch &&
    props.webResearchError === null
);

function onSaveWebResearch() {
  if (!canSaveWebResearch.value) return;
  emit("save-web-research", { ...webResearch.value });
}

// Test connection probes the *saved* endpoint, so unsaved edits block it.
const searxngConfigured = computed(
  () => (props.settings?.web_research.searxng_endpoint.trim() ?? "") !== ""
);
const canTestSearxng = computed(
  () => searxngConfigured.value && !webResearchDirty.value && !props.searxngTesting
);
const searxngTestTitle = computed(() => {
  if (props.searxngTesting) return "Testing…";
  if (!searxngConfigured.value) return "Save a SearXNG endpoint before testing";
  if (webResearchDirty.value) return "Save your changes before testing";
  return "Check the SearXNG instance is serving JSON search results";
});

const searxngStatusLine = computed<{ tone: LocalStatusTone; text: string }>(() => {
  if (props.searxngTesting) return { tone: "pending", text: "Testing…" };
  const p = props.searxngStatus;
  if (webResearchDirty.value || p === null) {
    return {
      tone: "pending",
      text: "Untested — checked here or when a web-research run starts.",
    };
  }
  if (p.status === "ok") {
    return { tone: "ok", text: "SearXNG is serving search results." };
  }
  if (p.status === "not_configured") {
    return {
      tone: "pending",
      text: "Not configured — a web-research run would research blind (the local suite is SearXNG-only).",
    };
  }
  return {
    tone: "err",
    text: `${p.detail ?? "SearXNG unreachable"} — start it with "docker compose up -d" from the app's searxng/ folder (OrbStack recommended on Apple Silicon), then re-test. The local suite is SearXNG-only, so research is skipped until it serves.`,
  };
});

// A reachable daemon with an empty roster must not read as fully set up: the
// probe only checks the ids that are configured.
const savedRosterComplete = computed(() => {
  const lm = props.settings?.local_models;
  return !!lm && lm.reasoner_model.trim() !== "";
});

// The indicator's states (docs/interface.md §Connection status). A result
// describes the saved config, so unsaved edits read untested.
type LocalStatusTone = "pending" | "ok" | "err";
const localStatus = computed<{ tone: LocalStatusTone; text: string }>(() => {
  if (props.localTesting) return { tone: "pending", text: "Testing…" };
  const d = props.localDaemon;
  if (localDirty.value || d === null) {
    return {
      tone: "pending",
      text: "Untested — connectivity is checked here or when a run starts.",
    };
  }
  if (!d.reachable) {
    const detail = d.detail ?? "Daemon unreachable";
    return {
      tone: "err",
      text: `${detail} — install Ollama from ollama.com/download (or brew install ollama), start it, then re-test.`,
    };
  }
  if (d.missing_models.length > 0) {
    return {
      tone: "err",
      text: `Daemon reachable, but these models aren't pulled: ${d.missing_models.join(
        ", "
      )}. Pull each with "ollama pull", then re-test.`,
    };
  }
  if (!savedRosterComplete.value) {
    return {
      tone: "pending",
      text: "Daemon reachable — add the reasoner model id to complete setup.",
    };
  }
  return { tone: "ok", text: "Daemon reachable — all rostered models available." };
});

function credDirty(key: CredKey): boolean {
  return creds.value[key].trim() !== "";
}

const hasTruncations = computed(
  () => (props.truncationStats?.total_truncations ?? 0) > 0
);

// Locale-independent so the readout is deterministic across environments.
function fmtNum(n: number): string {
  return String(n).replace(/\B(?=(\d{3})+(?!\d))/g, ",");
}

// "X of Y (Z%)", falling back to the bare count whenever the denominator can't
// support an honest rate: missing (0); smaller than the numerator (the two are
// written on independent best-effort channels, so a truncation can briefly
// lead its parse-run); or incomplete (`unaligned_truncations` predate the
// denominator, so a rate would mix cohorts; self-heals as those rows age out).
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

// Chars cut over total original chars, with `truncatedDocsValue`'s fallbacks
// plus one more cohort gap: a parse-run row with no original-chars value
// (`parse_runs_missing_original_chars`, NULL → skipped by SUM) leaves the
// denominator short of the numerator's cohort, so the ratio is withheld.
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
// The non-secret client_id round-trips its saved value; the secret input starts
// empty and is write-only.
const schwabClientId = ref("");
const schwabSecret = ref("");

// Reseed on every fresh status (App re-fetches after save/connect/disconnect):
// clears the typed secret and resets the dirty baseline.
watch(
  () => props.schwabStatus,
  (s) => {
    if (!s) return;
    schwabClientId.value = s.client_id;
    schwabSecret.value = "";
  },
  { immediate: true }
);

const schwabSecretPlaceholder = computed(() =>
  props.schwabStatus?.secret_configured
    ? "•••• saved — type a new value to replace"
    : "Not set"
);

// Connect reads the *saved* credentials, so an edit must be saved first.
const schwabDirty = computed(() => {
  const saved = props.schwabStatus?.client_id ?? "";
  return schwabClientId.value.trim() !== saved || schwabSecret.value.trim() !== "";
});

// Blocked while a connect holds the slot: `schwab_connect` captures the
// client_id up front but reads the secret from the Keychain later, so a
// mid-login save could pair the old id with a new secret and fail the token
// exchange.
const canSaveSchwab = computed(
  () =>
    schwabClientId.value.trim() !== "" &&
    schwabDirty.value &&
    !props.schwabConnecting &&
    !props.schwabBusy
);

const schwabCredsConfigured = computed(
  () =>
    !!props.schwabStatus &&
    props.schwabStatus.client_id.trim() !== "" &&
    props.schwabStatus.secret_configured
);

const canConnect = computed(
  () =>
    schwabCredsConfigured.value &&
    !schwabDirty.value &&
    !props.schwabConnecting &&
    !props.schwabBusy
);

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

const showDisconnect = computed(() => schwabConnection.value !== "not-connected");
const canDisconnect = computed(
  () => !props.schwabConnecting && !props.schwabBusy
);

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

const connectLabel = computed(() => {
  if (props.schwabConnecting) return "Connecting…";
  return schwabConnection.value === "not-connected" ? "Connect" : "Reconnect";
});

function onSaveSchwab() {
  if (!canSaveSchwab.value) return;
  emit("save-schwab", {
    client_id: schwabClientId.value.trim(),
    // Null leaves the stored secret untouched.
    client_secret: schwabSecret.value.trim() ? schwabSecret.value : null,
  });
}

// --- Data portability --------------------------------------------------------
// One passphrase serves both directions: export encrypts with it (blank =
// plaintext archive), import decrypts with it. Never stored.
const dataPassphrase = ref("");

const canUseData = computed(() => props.dataBusy === null && !props.slotBusy);

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
      <!-- Hosted in the toolbar, apart from the gated form, so it reads as
           instant-applying chrome and works even when settings fail to load. -->
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
              <!-- Disabled while this section's save is in flight: the clean
                   completion clears these fields, so text typed mid-save would
                   be lost. -->
              <input
                :id="`cred-${field.key}`"
                v-model="creds[field.key]"
                class="input mono"
                type="password"
                autocomplete="off"
                spellcheck="false"
                :disabled="saving"
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

        <!-- An independent submission outside the token-gated form
             (docs/configuration.md §External Data Provider Credentials), so a
             cloud-keyless machine persists FMP / FRED for the local suite. -->
        <form
          v-if="settings"
          class="settings-form"
          @submit.prevent="onSaveProviders"
        >
          <section class="settings-section" aria-labelledby="sec-creds">
            <h3 id="sec-creds" class="section-eyebrow">Data provider credentials</h3>
            <p class="section-note">
              Financial Modeling Prep, FRED, and Tavily are all required to run a
              report; the local analysis suite needs Financial Modeling Prep and
              FRED. These save on their own — no cloud API token needed. FRED
              needs a free API key; BLS, GDELT, and CFTC need no credential.
            </p>
            <div v-for="field in credentialFields" :key="field.key" class="field">
              <label class="label" :for="`cred-${field.key}`">{{ field.label }}</label>
              <!-- Locked mid-save for the same reason as the token fields. -->
              <input
                :id="`cred-${field.key}`"
                v-model="creds[field.key]"
                class="input mono"
                type="password"
                autocomplete="off"
                spellcheck="false"
                :disabled="savingProviders"
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

            <div v-if="providersError" class="settings-error" role="alert">
              <div class="settings-error-label">Couldn't save</div>
              <p class="settings-error-detail">{{ providersError }}</p>
            </div>

            <div class="section-actions">
              <button
                type="submit"
                class="btn btn-secondary"
                :disabled="!canSaveProviders"
              >
                {{ savingProviders ? "Saving…" : "Save credentials" }}
              </button>
              <span v-if="showProvidersSaved" class="save-status" role="status">
                <Icon name="check" :size="13" color="var(--ink-2)" />
                Saved
              </span>
            </div>
          </section>
        </form>

        <!-- docs/configuration.md §Local Models, docs/interface.md §Connection
             status. Connectivity is never probed at startup, so the indicator
             reads untested until tested or a run starts. -->
        <form v-if="settings" class="settings-form" @submit.prevent="onSaveLocal">
          <section class="settings-section" aria-labelledby="sec-local-models">
            <h3 id="sec-local-models" class="section-eyebrow">Local analysis models</h3>
            <p class="section-note">
              Portfolio Analysis and Trade Opportunities run on your own Ollama
              daemon. The endpoint and reasoner are required before
              either can run; the fast tier is optional. The app supervises a
              daemon you install — it bundles neither Ollama nor the models.
            </p>

            <div class="field">
              <label class="label" for="local-endpoint">Daemon endpoint</label>
              <input
                id="local-endpoint"
                v-model="localModels.daemon_endpoint"
                class="input mono"
                type="text"
                autocomplete="off"
                spellcheck="false"
                :disabled="savingLocal"
                placeholder="http://127.0.0.1:11434"
              />
            </div>
            <div class="field">
              <label class="label" for="local-reasoner">Reasoner model</label>
              <input
                id="local-reasoner"
                v-model="localModels.reasoner_model"
                class="input mono"
                type="text"
                autocomplete="off"
                spellcheck="false"
                :disabled="savingLocal"
                placeholder="Not set"
              />
            </div>
            <div class="field">
              <label class="label" for="local-fast">Fast model (optional)</label>
              <input
                id="local-fast"
                v-model="localModels.fast_model"
                class="input mono"
                type="text"
                autocomplete="off"
                spellcheck="false"
                :disabled="savingLocal"
                placeholder="Not set — the reasoner covers this role"
              />
            </div>

            <div class="cred-test local-test">
              <button
                type="button"
                class="cred-test-btn"
                :disabled="!canTestLocal"
                :title="localTestTitle"
                @click="emit('test-local')"
              >
                {{ localTesting ? "Testing…" : "Test connection" }}
              </button>
              <span
                class="cred-status"
                :class="`cred-status--${localStatus.tone}`"
                role="status"
                aria-live="polite"
              >
                <Icon
                  v-if="localStatus.tone === 'ok'"
                  name="check"
                  :size="13"
                  color="var(--ink-2)"
                />
                {{ localStatus.text }}
              </span>
            </div>

            <div v-if="localError" class="settings-error" role="alert">
              <div class="settings-error-label">Couldn't save</div>
              <p class="settings-error-detail">{{ localError }}</p>
            </div>

            <div class="section-actions">
              <button
                type="submit"
                class="btn btn-secondary"
                :disabled="!canSaveLocal"
              >
                {{ savingLocal ? "Saving…" : "Save local models" }}
              </button>
              <span v-if="showLocalSaved" class="save-status" role="status">
                <Icon name="check" :size="13" color="var(--ink-2)" />
                Saved
              </span>
            </div>
          </section>
        </form>

        <!-- docs/configuration.md §Web Research, docs/web-research.md §Search
             backend. The local suite is SearXNG-only; there is no Tavily
             fallback. -->
        <form v-if="settings" class="settings-form" @submit.prevent="onSaveWebResearch">
          <section class="settings-section" aria-labelledby="sec-web-research">
            <h3 id="sec-web-research" class="section-eyebrow">Web research</h3>
            <p class="section-note">
              The local jobs research the open web through a self-hosted SearXNG
              instance — keyless and cost-free. Start it once with
              <span class="mono">docker compose up -d</span> from the app's
              <span class="mono">searxng/</span> folder (OrbStack is the
              recommended runtime on Apple Silicon). When it can't serve, a run
              proceeds without web research — evidence is thinner and conviction
              lower.
            </p>

            <div class="field">
              <label class="label" for="searxng-endpoint">SearXNG endpoint</label>
              <input
                id="searxng-endpoint"
                v-model="webResearch.searxng_endpoint"
                class="input mono"
                type="text"
                autocomplete="off"
                spellcheck="false"
                :disabled="savingWebResearch"
                placeholder="http://127.0.0.1:8888"
              />
            </div>

            <div class="cred-test local-test">
              <button
                type="button"
                class="cred-test-btn"
                :disabled="!canTestSearxng"
                :title="searxngTestTitle"
                @click="emit('test-searxng')"
              >
                {{ searxngTesting ? "Testing…" : "Test connection" }}
              </button>
              <span
                class="cred-status"
                :class="`cred-status--${searxngStatusLine.tone}`"
                role="status"
                aria-live="polite"
              >
                <Icon
                  v-if="searxngStatusLine.tone === 'ok'"
                  name="check"
                  :size="13"
                  color="var(--ink-2)"
                />
                {{ searxngStatusLine.text }}
              </span>
            </div>

            <div v-if="webResearchError" class="settings-error" role="alert">
              <div class="settings-error-label">Couldn't save</div>
              <p class="settings-error-detail">{{ webResearchError }}</p>
            </div>

            <div class="section-actions">
              <button
                type="submit"
                class="btn btn-secondary"
                :disabled="!canSaveWebResearch"
              >
                {{ savingWebResearch ? "Saving…" : "Save web research" }}
              </button>
              <span v-if="showWebResearchSaved" class="save-status" role="status">
                <Icon name="check" :size="13" color="var(--ink-2)" />
                Saved
              </span>
            </div>
          </section>
        </form>

        <!-- docs/configuration.md §Investor Profile. Read-only, outside every
             form, on its own data channel like diagnostics. -->
        <section
          v-if="investorProfile"
          class="settings-section"
          aria-labelledby="sec-profile"
        >
          <h3 id="sec-profile" class="section-eyebrow">Investor profile</h3>
          <p class="section-note">
            The fixed posture the local jobs frame their prescriptions against. It
            shapes actions and framing, never which holdings grade well. Read-only
            until a configurable profile ships.
          </p>
          <dl class="profile-rows">
            <div class="profile-row">
              <dt>Objective</dt>
              <dd>{{ investorProfile.objective }}</dd>
            </div>
            <div class="profile-row">
              <dt>Risk tolerance</dt>
              <dd>{{ investorProfile.risk_tolerance }}</dd>
            </div>
            <div class="profile-row">
              <dt>Horizon</dt>
              <dd>{{ investorProfile.horizon }}</dd>
            </div>
            <div class="profile-row">
              <dt>Tax posture</dt>
              <dd>{{ investorProfile.tax }}</dd>
            </div>
            <div class="profile-row">
              <dt>Cash posture</dt>
              <dd>{{ investorProfile.cash }}</dd>
            </div>
          </dl>
        </section>

        <!-- docs/schwab-integration.md, docs/interface.md §Connection status.
             Outside the config form on its own status channel, so it appears
             even if the cloud settings fail to load. -->
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

          <!-- Persistent live region, so connection changes are announced reliably. -->
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

        <!-- docs/data-portability.md. Always rendered: it needs no loaded
             settings. The destructive import is confirmed by App's
             ConfirmDialog, not here. -->
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

        <!-- Truncation telemetry (docs/agents.md §Data Extraction), on its own
             data channel so it appears even when settings fail to load. -->
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

/* Geometry matches the report/inbox panes so the views share a seam; min-height
   keeps it uniform whether or not a toolbar carries a button. */
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

/* Flush at the toolbar seam. Compound selector so it beats `.settings-section`
   regardless of source order. */
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

.field :deep(.label) {
  margin-bottom: var(--s-2);
}

/* System extension: the package doesn't style a native <select>; this extends
   the open bottom-border `.input` idiom to it with the kit's chevron overlaid. */
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
  /* Section rhythm below the button, so the next section's top rule doesn't
     hug it. */
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

/* A gating hint, not an error. */
.save-status--hint {
  color: var(--ink-3);
}

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

.schwab-hint {
  margin: var(--s-3) 0 0;
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  line-height: var(--lh-ui);
  color: var(--ink-3);
}

/* No top rule, unlike `.settings-actions`: these sit mid-section, not at the
   form's foot. */
.schwab-actions,
.data-actions,
.section-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--s-4);
  margin-top: var(--s-6);
}

/* ConnectionTestRow's styles are scoped, so its classes are re-declared here
   for the richer daemon and SearXNG rows. */
.cred-test {
  display: flex;
  align-items: baseline;
  gap: var(--s-5);
  margin-top: var(--s-3);
  min-height: 18px;
}

.cred-test-btn {
  flex-shrink: 0;
  padding: 0;
  background: transparent;
  border: 0;
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  color: var(--ink-2);
  cursor: pointer;
  text-decoration: underline;
  text-underline-offset: 2px;
  transition: color var(--dur-fast) var(--ease);
}

.cred-test-btn:hover:not(:disabled) {
  color: var(--ink);
}

.cred-test-btn:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
  border-radius: var(--radius-sm);
}

.cred-test-btn:disabled {
  color: var(--ink-3);
  cursor: not-allowed;
  text-decoration: none;
}

.cred-status {
  display: inline-flex;
  align-items: center;
  gap: var(--s-2);
  min-width: 0;
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  line-height: var(--lh-ui);
  overflow-wrap: anywhere;
}

.cred-status--pending {
  color: var(--ink-3);
}

/* ink-2 (not ink-3) to clear AA at 13px, matching the Save "Saved" confirmation. */
.cred-status--ok {
  color: var(--ink-2);
}

.cred-status--err {
  color: var(--accent-text);
}

/* Sits after its field, so a tight top rhythm instead of the note's bottom one. */
.data-caution {
  margin: var(--s-2) 0 var(--s-5);
}

/* Carries the archive's absolute path, which must wrap inside the column. */
.data-status {
  align-items: baseline;
  overflow-wrap: anywhere;
  min-width: 0;
}

/* flex-shrink:0 so it never collapses; the title truncates first. */
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

/* The kit's boxy switch (design kit Settings.jsx Toggle): no pill, no rounded
   slider. */
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

/* The knob is always filled: a transparent off-knob read as an empty box. */
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

.profile-rows {
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: var(--s-4);
}

.profile-row dt {
  font-family: var(--font-sans);
  font-size: var(--t-caption);
  letter-spacing: var(--track-caption);
  text-transform: uppercase;
  font-weight: 600;
  color: var(--ink-3);
}

.profile-row dd {
  margin: var(--s-1) 0 0;
  font-family: var(--font-sans);
  font-size: var(--t-ui-sm);
  line-height: 1.45;
  color: var(--ink);
}

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

/* ink-2 for AA at 13px. */
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
  .switch-knob,
  .cred-test-btn {
    transition: none;
  }
}
</style>
