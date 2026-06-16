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
import { deepFreeze } from "../helpers/freeze";
import type { SettingsView, TruncationStats } from "../../src/types";

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
  truncationStats: null as TruncationStats | null,
};

// makeWrapper spreads baseProps *shallowly*, so its nested objects (settingsView,
// testing, testResults) are shared by reference across every wrapper. The fixture
// is read-only by design — deep-freezing makes that a guarantee: a test that
// mutates a nested prop in place would throw at the write rather than silently
// leak into the next test. (Settings.vue copies props into local form state, so a
// frozen `settings` mounts unchanged.)
deepFreeze(baseProps);

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

// --- Truncation diagnostics section ----------------------------------------
// The read-only telemetry block: omitted when unavailable, an empty state when
// nothing's been recorded, and a label/value readout (+ per-format breakdown)
// once truncations exist.

function diagnostics(wrapper: ReturnType<typeof makeWrapper>) {
  return wrapper.find('section[aria-labelledby="sec-diagnostics"]');
}

test("the diagnostics section is omitted when stats are unavailable (null)", () => {
  const wrapper = makeWrapper({ truncationStats: null });
  expect(diagnostics(wrapper).exists()).toBe(false);
});

test("a zero aggregate renders the empty state, not the readout", () => {
  const stats: TruncationStats = {
    total_truncations: 0,
    total_docs_parsed: 12,
    unaligned_truncations: 0,
    reports_affected: 0,
    total_chars_dropped: 0,
    by_format: [],
    latest_captured_at: null,
  };
  const wrapper = makeWrapper({ truncationStats: stats });
  const section = diagnostics(wrapper);
  expect(section.exists()).toBe(true);
  expect(section.find(".trunc-empty").exists()).toBe(true);
  expect(section.find(".trunc-stats").exists()).toBe(false);
});

test("a populated aggregate renders the rate, counts, formatted chars, and per-format breakdown", () => {
  const stats: TruncationStats = {
    total_truncations: 3,
    total_docs_parsed: 11,
    unaligned_truncations: 0,
    reports_affected: 2,
    total_chars_dropped: 29000,
    by_format: [
      { format: "pdf", count: 2 },
      { format: "html", count: 1 },
    ],
    latest_captured_at: "2026-06-08T09:00:00+00:00",
  };
  const wrapper = makeWrapper({ truncationStats: stats });
  const section = diagnostics(wrapper);
  expect(section.find(".trunc-empty").exists()).toBe(false);

  // Scalar readout: the truncated-of-parsed rate (3/11 = 27.3%), reports
  // affected, and thousands-grouped chars dropped.
  const values = section.findAll(".trunc-row dd").map((d) => d.text());
  expect(values).toContain("3 of 11 (27.3%)");
  expect(values).toContain("2");
  expect(values).toContain("29,000");

  // Per-format breakdown, in the backend's descending-count order.
  const formats = section.findAll(".trunc-format-item").map((li) => ({
    name: li.find(".trunc-format-name").text(),
    count: li.find(".trunc-format-count").text(),
  }));
  expect(formats).toEqual([
    { name: "pdf", count: "2" },
    { name: "html", count: "1" },
  ]);
});

test("a missing denominator falls back to the bare truncation count, not 'of 0'", () => {
  const stats: TruncationStats = {
    total_truncations: 3,
    total_docs_parsed: 0,
    unaligned_truncations: 0,
    reports_affected: 2,
    total_chars_dropped: 29000,
    by_format: [{ format: "pdf", count: 3 }],
    latest_captured_at: "2026-06-08T09:00:00+00:00",
  };
  const wrapper = makeWrapper({ truncationStats: stats });
  const values = diagnostics(wrapper)
    .findAll(".trunc-row dd")
    .map((d) => d.text());
  expect(values).toContain("3");
  expect(values.some((v) => v.includes("of 0"))).toBe(false);
});

test("a numerator above the denominator falls back to the bare count, not >100%", () => {
  // The two telemetry writes are independent best-effort, so a truncation count
  // ahead of its parse-run count is possible; it must not render an over-100% rate.
  const stats: TruncationStats = {
    total_truncations: 5,
    total_docs_parsed: 3,
    unaligned_truncations: 0,
    reports_affected: 2,
    total_chars_dropped: 29000,
    by_format: [{ format: "pdf", count: 5 }],
    latest_captured_at: "2026-06-08T09:00:00+00:00",
  };
  const wrapper = makeWrapper({ truncationStats: stats });
  const values = diagnostics(wrapper)
    .findAll(".trunc-row dd")
    .map((d) => d.text());
  expect(values).toContain("5");
  expect(values.some((v) => v.includes("%"))).toBe(false);
});

test("an unaligned cohort suppresses the rate even when it would otherwise compute", () => {
  // 3 of 100 would render "3.0%", but a truncation without a parse-run
  // denominator means the cohorts are mismatched, so the rate is withheld until
  // the legacy rows age out — the bare count shows in the meantime.
  const stats: TruncationStats = {
    total_truncations: 3,
    total_docs_parsed: 100,
    unaligned_truncations: 1,
    reports_affected: 2,
    total_chars_dropped: 29000,
    by_format: [{ format: "pdf", count: 3 }],
    latest_captured_at: "2026-06-08T09:00:00+00:00",
  };
  const wrapper = makeWrapper({ truncationStats: stats });
  const values = diagnostics(wrapper)
    .findAll(".trunc-row dd")
    .map((d) => d.text());
  expect(values).toContain("3");
  expect(values).not.toContain("3 of 100 (3.0%)");
  expect(values.some((v) => v.includes("%"))).toBe(false);
});
