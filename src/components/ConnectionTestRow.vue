<script setup lang="ts">
import { computed } from "vue";
import Icon from "./Icon.vue";
import type { ConnectionTestResult } from "../types";

// The "Test connection" affordance under one credential field: a quiet text
// button plus an inline status. Extracted from Settings so the markup isn't
// duplicated across the API-token and data-provider sections. Validates the
// *saved* credential, so it's offered only when one is configured and the field
// isn't mid-edit (a typed value isn't saved yet, and isn't what the backend
// tests).
const props = defineProps<{
  // Whether a value is already saved for this credential.
  configured: boolean;
  // Whether the field currently holds a typed (unsaved) value.
  dirty: boolean;
  // This credential is mid-test.
  testing: boolean;
  // The last test result, or null if none yet.
  result: ConnectionTestResult | null;
}>();

const emit = defineEmits<{ (e: "test"): void }>();

const canTest = computed(
  () => props.configured && !props.dirty && !props.testing
);

// Why the button is (un)available, surfaced as its title.
const title = computed(() => {
  if (props.testing) return "Testing…";
  if (!props.configured) return "Save a value before testing";
  if (props.dirty) return "Save the new value before testing";
  return "Send a test request to verify the saved credential";
});

// Show a result only when the field is at rest — a typed-but-unsaved value would
// make a chip about the saved key misleading.
const showResult = computed(() => !props.dirty && props.result !== null);

const statusClass = computed(() => {
  if (props.testing || !props.result) return "cred-status--pending";
  return props.result.ok ? "cred-status--ok" : "cred-status--err";
});
</script>

<template>
  <div class="cred-test">
    <button
      type="button"
      class="cred-test-btn"
      :disabled="!canTest"
      :title="title"
      @click="emit('test')"
    >
      {{ testing ? "Testing…" : "Test connection" }}
    </button>
    <!-- Persistent live region: the node stays mounted and its text changes, so
         screen readers reliably announce results (a node inserted with text
         already present is not announced consistently). -->
    <span class="cred-status" :class="statusClass" role="status" aria-live="polite">
      <template v-if="testing">Testing…</template>
      <template v-else-if="showResult">
        <Icon v-if="result?.ok" name="check" :size="13" color="var(--ink-2)" />
        {{ result?.detail }}
      </template>
    </span>
  </div>
</template>

<style scoped>
/* A system extension — the design package defines no per-field test affordance;
   the success chip reuses the Save "check + ink-2" treatment and the failure copy
   uses the accent the settings error already uses. */
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
  color: var(--accent);
}

@media (prefers-reduced-motion: reduce) {
  .cred-test-btn {
    transition: none;
  }
}
</style>
