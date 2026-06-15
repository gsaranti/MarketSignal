// First SFC spec to exercise the `@tauri-apps/api` boundary. App.vue is the only
// component that imports it — four modules: core (`invoke`, 7 commands), event
// (`listen`), window (`getCurrentWindow().onFocusChanged`), and app (`getVersion`).
//
// This establishes the project's Tauri-mock pattern. `vi.mock` factories are
// hoisted above imports, so the mock *functions* are declared via `vi.hoisted`
// and their *implementations* come from `tests/helpers/tauri.ts` in `beforeEach`.
// `vitest.config.ts` sets `globals: false`, so every test helper (incl. `vi`) is
// imported explicitly.
//
// Three assertions: (1) App's `onMounted` bootstrap contract — the exact command /
// listener / window / version set it fires on mount, which doubles as proof the
// four-module mock is complete enough to mount the real App; (2) the `@save`
// wiring round-trips a child emit into `invoke("save_settings", payload)`; (3) the
// `@set-enabled` wiring round-trips into `invoke("set_job_enabled", { enabled })`.

import { describe, test, expect, beforeEach, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { makeInvokeRouter, unlisten } from "../helpers/tauri";

// Hoisted above the imports below so the `vi.mock` factories can close over these
// mock fns. (`vi.hoisted` runs before module imports; `vi` is available in it.)
const tauri = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  getVersion: vi.fn(),
  getCurrentWindow: vi.fn(),
  onFocusChanged: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: tauri.listen }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: tauri.getCurrentWindow,
}));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: tauri.getVersion }));

// Resolved only after the mocks are registered (hoisting makes the order here
// cosmetic, but App.vue must bind the mocked modules at import time).
import App from "../../src/App.vue";
import RecentReportsSidebar from "../../src/components/RecentReportsSidebar.vue";
import Settings from "../../src/components/Settings.vue";

beforeEach(() => {
  // Clear call history between tests, then (re)apply implementations.
  vi.clearAllMocks();
  tauri.invoke.mockImplementation(makeInvokeRouter());
  tauri.listen.mockResolvedValue(unlisten);
  tauri.getVersion.mockResolvedValue("9.9.9-test");
  tauri.onFocusChanged.mockResolvedValue(unlisten);
  tauri.getCurrentWindow.mockReturnValue({ onFocusChanged: tauri.onFocusChanged });
});

const invokedCommands = () => tauri.invoke.mock.calls.map((c) => c[0]);

describe("App.vue Tauri boundary", () => {
  test("mounts against the mock and fires the onMounted bootstrap cascade", async () => {
    const wrapper = mount(App);
    await flushPromises();

    // Version read, the focus subscription via getCurrentWindow(), and both
    // event listeners — all real onMounted glue.
    expect(tauri.getVersion).toHaveBeenCalledTimes(1);
    expect(tauri.getCurrentWindow).toHaveBeenCalled();
    expect(tauri.onFocusChanged).toHaveBeenCalledTimes(1);

    // Exactly the two listeners, no more — sorted-equality so the assertion is a
    // true set check (catches a dropped or duplicated listener), order-agnostic.
    const events = tauri.listen.mock.calls.map((c) => c[0]).sort();
    expect(events).toEqual(["job-finished", "job-progress"]);

    // Exactly the six refresh commands onMounted issues — sorted-equality enforces
    // the set with no extras or duplicates (not a subset), while tolerating any
    // reordering of the onMounted calls. Proves the four-module mock is complete
    // enough to mount the real App without a throw.
    expect([...invokedCommands()].sort()).toEqual([
      "check_configuration",
      "get_settings",
      "job_status",
      "list_reports",
      "list_research_archive",
      "list_research_inbox",
    ]);

    wrapper.unmount();
  });

  test("a Settings @save emit round-trips into invoke('save_settings', payload)", async () => {
    const wrapper = mount(App);
    await flushPromises();

    // Settings is v-if'd on view === 'settings'; the sidebar owns navigation.
    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "settings");
    await flushPromises();

    const settings = wrapper.findComponent(Settings);
    expect(settings.exists()).toBe(true);

    const payload = {
      models: { main: "gpt-x", bull: "gpt-x", bear: "gpt-x", balanced: "gpt-x" },
      credentials: {
        openai: null,
        anthropic: null,
        fmp: "new-fmp",
        fred: null,
        tavily: null,
      },
    };
    settings.vm.$emit("save", payload);
    await flushPromises();

    // The @save="saveSettings" wiring forwards the child payload verbatim.
    expect(tauri.invoke).toHaveBeenCalledWith("save_settings", payload);

    wrapper.unmount();
  });

  test("a Settings @set-enabled emit round-trips into invoke('set_job_enabled', { enabled })", async () => {
    const wrapper = mount(App);
    await flushPromises();

    wrapper.findComponent(RecentReportsSidebar).vm.$emit("navigate", "settings");
    await flushPromises();

    const settings = wrapper.findComponent(Settings);
    expect(settings.exists()).toBe(true);

    // The default mock reports the job enabled; toggling it off must reach the
    // backend. This covers App's @set-enabled="setJobEnabled" binding + handler —
    // glue the child-level Settings spec (which stops at the emit) can't see.
    settings.vm.$emit("set-enabled", false);
    await flushPromises();

    expect(tauri.invoke).toHaveBeenCalledWith("set_job_enabled", {
      enabled: false,
    });

    wrapper.unmount();
  });
});
