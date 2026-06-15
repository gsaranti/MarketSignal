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
} from "../../src/types";

// Minimal valid shapes for the commands App's `onMounted` cascade calls, so a
// full App mount completes without a hand-rolled fixture per spec. A clean,
// unblocked config with one enabled-but-idle job and nothing yet generated.
export const defaultValidation: ValidationReport = {
  categories: [],
  is_blocked: false,
};

export const defaultJobStatus: JobStatus = {
  enabled: true,
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
    // Action commands a spec may drive through a user interaction.
    save_settings: () => null,
    set_job_enabled: () => null,
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
