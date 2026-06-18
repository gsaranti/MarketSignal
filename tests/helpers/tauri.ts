// Reusable test doubles for the four `@tauri-apps/api` modules `App.vue` imports
// (core / event / window / app). App.vue is the only SFC that touches Tauri, so
// this is the single home for the mock surface.
//
// Deliberately pure — no `vi.*` here. `vi.mock` factories are hoisted above a
// spec's imports, so the mock *functions* must be declared in the spec (via
// `vi.hoisted`); this module only supplies their *implementations* — the invoke
// command-router and the default response shapes — applied in the spec's
// `beforeEach`. Keeping `vi` out means the helper is import-order-agnostic and
// reusable from any spec without fighting hoist ordering.

import type {
  ValidationReport,
  JobStatus,
  ReportSummary,
  ResearchDocument,
  SettingsView,
  TruncationStats,
} from "../../src/types";

// Minimal valid shapes for the commands App's `onMounted` cascade calls, so a
// full App mount completes without a hand-rolled fixture per spec. A clean,
// unblocked config with one enabled-but-idle job and nothing yet generated.
export const defaultValidation: ValidationReport = {
  categories: [],
  is_blocked: false,
};

export const defaultJobStatus: JobStatus = {
  is_running: false,
  last_successful_at: null,
  last_failed_at: null,
  last_failure_detail: null,
  last_skipped_at: null,
  last_cancelled_at: null,
};

export const defaultSettings: SettingsView = {
  models: { main: "", bull: "", bear: "", balanced: "" },
  credentials: {
    openai: false,
    anthropic: false,
    fmp: false,
    fred: false,
    tavily: false,
  },
  available_models: [],
};

// Loaded alongside settings when the Settings view opens; a clean install has
// recorded no truncations.
export const defaultTruncationStats: TruncationStats = {
  total_truncations: 0,
  total_docs_parsed: 0,
  unaligned_truncations: 0,
  total_original_chars: 0,
  parse_runs_missing_original_chars: 0,
  reports_affected: 0,
  total_chars_dropped: 0,
  by_format: [],
  latest_captured_at: null,
};

export type InvokeHandler = (args?: Record<string, unknown>) => unknown;

// The command → response map. Any command absent here throws when invoked, so a
// new `onMounted` call — or a typo — surfaces loudly instead of resolving
// `undefined` and silently passing a half-wired mount.
export function defaultInvokeHandlers(): Record<string, InvokeHandler> {
  return {
    // onMounted bootstrap reads.
    check_configuration: () => defaultValidation,
    job_status: () => defaultJobStatus,
    list_reports: () => [] as ReportSummary[],
    list_research_inbox: () => [] as ResearchDocument[],
    list_research_archive: () => [] as ResearchDocument[],
    get_settings: () => defaultSettings,
    // Read on Settings-view entry, alongside get_settings.
    truncation_stats: () => defaultTruncationStats,
    // Action commands a spec may drive through a user interaction.
    save_settings: () => null,
  };
}

// Build an `invoke` implementation. `overrides` replace or add per-command
// handlers — to assert a specific payload, return tailored data, or simulate a
// failure (a handler that throws).
export function makeInvokeRouter(
  overrides: Record<string, InvokeHandler> = {}
): (cmd: string, args?: Record<string, unknown>) => Promise<unknown> {
  const handlers = { ...defaultInvokeHandlers(), ...overrides };
  return async (cmd: string, args?: Record<string, unknown>) => {
    const handler = handlers[cmd];
    if (!handler) {
      throw new Error(`tauri test mock: unhandled invoke("${cmd}")`);
    }
    return handler(args);
  };
}

// A no-op `UnlistenFn` — the resolved value of both `listen` and `onFocusChanged`.
export const unlisten = (): void => {};

// Capture the callback a spec's mocked `listen` registered for an event, so the
// spec can drive App's run tracker by feeding it `ProgressMessage`s the way the
// backend would over the "job-progress" channel. App registers its listeners in
// `onMounted`, so call this only after the mount's promises have flushed. Stays
// `vi`-free (reads the mock's `.calls` structurally) so the helper keeps its
// import-order-agnostic posture.
type ListenLike = { mock: { calls: unknown[][] } };
export function emitterFor(
  listenMock: ListenLike,
  event: string
): (payload: unknown) => void {
  const call = listenMock.mock.calls.find((c) => c[0] === event);
  if (!call) {
    throw new Error(`tauri test mock: no listener registered for "${event}"`);
  }
  const cb = call[1] as (e: { payload: unknown }) => void;
  return (payload: unknown) => cb({ payload });
}

// The window's `onFocusChanged` sibling of `emitterFor`. App subscribes via
// `getCurrentWindow().onFocusChanged(cb)` in `onMounted`, so the callback is the
// first (only) arg of the single registration; capture it to drive App's
// focus-refresh path by feeding it focus transitions the way wry's window would.
// Same `vi`-free, post-mount-flush contract as `emitterFor`.
export function focusEmitter(
  onFocusChangedMock: ListenLike
): (focused: boolean) => void {
  const call = onFocusChangedMock.mock.calls[0];
  if (!call) {
    throw new Error("tauri test mock: onFocusChanged was never registered");
  }
  const cb = call[0] as (e: { payload: boolean }) => void;
  return (focused: boolean) => cb({ payload: focused });
}
