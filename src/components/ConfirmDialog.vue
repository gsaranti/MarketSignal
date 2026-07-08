<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, watch } from "vue";

// Blocking confirmation for destructive / irreversible actions — the design
// package's confirmation dialog (market-signal-design-system
// preview/confirmation-dialog.html, `.dialog-*` in colors_and_type.css)
// translated to Vue. Generic chrome: flat hairline panel, no shadow — all
// separation comes from the `--scrim` veil. Presentational: the parent owns
// `open` and `busy` (set busy on confirm, clear it when the action settles);
// confirm/cancel are only ever emitted. Escape and a scrim click are Cancel,
// both inert while busy.
const props = defineProps<{
  open: boolean;
  title: string;
  body: string;
  // Optional second body paragraph — the concrete specifics of what the
  // action targets (e.g. the picked archive's date and counts), kept apart
  // from `body` so the destructive scope reads first.
  detail?: string;
  confirmLabel: string;
  busy: boolean;
  // The undecorated status line at the left of the actions row while busy.
  busyStatus: string;
}>();

const emit = defineEmits<{
  (e: "confirm"): void;
  (e: "cancel"): void;
}>();

const panel = ref<HTMLDivElement | null>(null);
const cancelBtn = ref<HTMLButtonElement | null>(null);

// The element focused before the dialog opened, restored on close (the
// dialogs-restore-focus contract).
let opener: HTMLElement | null = null;

function onCancel() {
  if (props.busy) return;
  emit("cancel");
}

function onConfirm() {
  if (props.busy) return;
  emit("confirm");
}

function onScrimClick(e: MouseEvent) {
  if (e.target === e.currentTarget) onCancel();
}

// Escape = Cancel; Tab is trapped inside the panel while open. Registered on
// the document (capture) only while open, so a closed dialog costs nothing.
function onKeydown(e: KeyboardEvent) {
  if (e.key === "Escape") {
    e.preventDefault();
    onCancel();
    return;
  }
  if (e.key !== "Tab") return;
  const focusables = Array.from(
    panel.value?.querySelectorAll<HTMLElement>("button:not(:disabled)") ?? []
  );
  if (focusables.length === 0) {
    // Both actions disabled (busy): focus parks on the panel itself.
    e.preventDefault();
    panel.value?.focus();
    return;
  }
  const first = focusables[0];
  const last = focusables[focusables.length - 1];
  const active = document.activeElement;
  const inside = panel.value?.contains(active) ?? false;
  if (e.shiftKey && (active === first || !inside)) {
    e.preventDefault();
    last.focus();
  } else if (!e.shiftKey && (active === last || !inside)) {
    e.preventDefault();
    first.focus();
  }
}

watch(
  () => props.open,
  async (open) => {
    if (open) {
      opener =
        document.activeElement instanceof HTMLElement
          ? document.activeElement
          : null;
      document.addEventListener("keydown", onKeydown, true);
      await nextTick();
      // Initial focus lands on Cancel, the safe action.
      cancelBtn.value?.focus();
    } else {
      document.removeEventListener("keydown", onKeydown, true);
      opener?.focus();
      opener = null;
    }
  }
);

// While busy both actions disable and focus would fall to <body>; park it on
// the panel (tabindex="-1") and return it to Cancel when busy clears.
watch(
  () => props.busy,
  async (busy) => {
    if (!props.open) return;
    await nextTick();
    if (busy) panel.value?.focus();
    else cancelBtn.value?.focus();
  }
);

onBeforeUnmount(() => {
  document.removeEventListener("keydown", onKeydown, true);
});
</script>

<template>
  <div v-if="open" class="dialog-scrim" @click="onScrimClick">
    <div
      ref="panel"
      class="dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="confirm-dialog-title"
      aria-describedby="confirm-dialog-body"
      :aria-busy="busy || undefined"
      tabindex="-1"
    >
      <h2 id="confirm-dialog-title" class="dialog-title">{{ title }}</h2>
      <div id="confirm-dialog-body" class="dialog-body">
        <p>{{ body }}</p>
        <p v-if="detail">{{ detail }}</p>
      </div>
      <div class="dialog-actions">
        <span v-if="busy" class="dialog-status" role="status">
          {{ busyStatus }}
        </span>
        <button
          ref="cancelBtn"
          type="button"
          class="btn btn-secondary"
          :disabled="busy"
          @click="onCancel"
        >
          Cancel
        </button>
        <button
          type="button"
          class="btn btn-primary"
          :disabled="busy"
          @click="onConfirm"
        >
          {{ confirmLabel }}
        </button>
      </div>
    </div>
  </div>
</template>

<!-- No scoped styles: every class comes from the design package's
     colors_and_type.css (imported globally in main.ts), so the component can
     never drift from the package's dialog spec. -->
