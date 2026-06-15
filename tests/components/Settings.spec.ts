// Settings.vue is presentational — props in, four events out (save / set-enabled /
// set-dark / test) — and, unlike App.vue, imports no `@tauri-apps/api`, so it
// mounts with no Tauri mock, exactly like ResearchDocuments.spec.ts. (The handoff
// framed Settings as an invoke-caller; it isn't — only App.vue is.)
//
// Pins the emit contract that carries real logic: the save payload omits
// untouched credentials (the secret is never re-sent — Settings.vue onSave) and
// includes only the typed ones, a no-edit submit doesn't emit, and the
// appearance / schedule toggles flip the current prop value.

import { test, expect } from "vitest";
import { mount } from "@vue/test-utils";
import Settings from "../../src/components/Settings.vue";
import type { SettingsView } from "../../src/types";

const settingsView: SettingsView = {
  models: { main: "gpt-main", bull: "gpt-bull", bear: "gpt-bear", balanced: "gpt-bal" },
  // Both API tokens already configured, so `tokensSatisfied` holds without
  // retyping them — a single typed data-provider credential can drive a valid
  // save while the four other slots stay untouched.
  credentials: { openai: true, anthropic: true, fmp: false, fred: false, tavily: false },
  available_models: [
    { slug: "gpt-main", label: "GPT Main", provider: "OpenAI" },
    { slug: "gpt-bull", label: "GPT Bull", provider: "OpenAI" },
    { slug: "gpt-bear", label: "GPT Bear", provider: "OpenAI" },
    { slug: "gpt-bal", label: "GPT Balanced", provider: "OpenAI" },
  ],
};

const baseProps = {
  settings: settingsView,
  loading: false,
  saving: false,
  error: null as string | null,
  jobEnabled: true as boolean | null,
  jobBusy: false,
  dark: false,
  testing: { openai: false, anthropic: false, fmp: false, fred: false, tavily: false },
  testResults: { openai: null, anthropic: null, fmp: null, fred: null, tavily: null },
};

function makeWrapper(overrides: Partial<typeof baseProps> = {}) {
  return mount(Settings, { props: { ...baseProps, ...overrides } });
}

test("save emits only the typed credential, leaving untouched secrets null", async () => {
  const wrapper = makeWrapper();
  // Type a new FMP key; the four others (incl. both saved tokens) stay untouched.
  await wrapper.find("#cred-fmp").setValue("new-fmp-key");
  await wrapper.find("form.settings-form").trigger("submit");

  const saved = wrapper.emitted("save");
  expect(saved).toHaveLength(1);
  expect(saved![0][0]).toEqual({
    models: { main: "gpt-main", bull: "gpt-bull", bear: "gpt-bear", balanced: "gpt-bal" },
    credentials: { openai: null, anthropic: null, fmp: "new-fmp-key", fred: null, tavily: null },
  });
});

test("submitting with no edits does not emit save (nothing dirty)", async () => {
  const wrapper = makeWrapper();
  await wrapper.find("form.settings-form").trigger("submit");
  expect(wrapper.emitted("save")).toBeUndefined();
});

test("the appearance toggle emits set-dark with the flipped value", async () => {
  const wrapper = makeWrapper({ dark: false });
  await wrapper.find('section[aria-labelledby="sec-appearance"] button').trigger("click");
  expect(wrapper.emitted("set-dark")).toEqual([[true]]);
});

test("the schedule toggle emits set-enabled with the flipped value", async () => {
  const wrapper = makeWrapper({ jobEnabled: true });
  await wrapper.find('section[aria-labelledby="sec-schedule"] button').trigger("click");
  expect(wrapper.emitted("set-enabled")).toEqual([[false]]);
});
