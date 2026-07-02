// Settings.vue is presentational — props in, three events out (save / set-dark /
// test) — and, unlike App.vue, imports no `@tauri-apps/api`, so it mounts with no
// Tauri mock, exactly like ResearchDocuments.spec.ts. (The handoff framed Settings
// as an invoke-caller; it isn't — only App.vue is.)
//
// Pins the emit contract that carries real logic: the save payload omits
// untouched credentials (the secret is never re-sent — Settings.vue onSave) and
// includes only the typed ones, a no-edit submit doesn't emit, and the appearance
// toggle flips the current prop value.

import { test, expect } from "vitest";
import { mount } from "@vue/test-utils";
import Settings from "../../src/components/Settings.vue";
import { deepFreeze } from "../helpers/freeze";
import { defaultSchwabStatus } from "../helpers/tauri";
import type { SettingsView, SchwabStatus, TruncationStats } from "../../src/types";

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
  dark: false,
  testing: { openai: false, anthropic: false, fmp: false, fred: false, tavily: false },
  testResults: { openai: null, anthropic: null, fmp: null, fred: null, tavily: null },
  truncationStats: null as TruncationStats | null,
  // A clean install: no Schwab credentials, no connection (the shared helper
  // fixture, spread so deepFreeze can't freeze the shared object). Schwab tests
  // override.
  schwabStatus: { ...defaultSchwabStatus } as SchwabStatus | null,
  schwabConnecting: false,
  schwabBusy: false,
  schwabError: null as string | null,
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

test("the appearance toggle (in the toolbar) emits set-dark with the flipped value", async () => {
  const wrapper = makeWrapper({ dark: false });
  await wrapper.find('.toolbar button[role="switch"]').trigger("click");
  expect(wrapper.emitted("set-dark")).toEqual([[true]]);
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
    total_original_chars: 48000,
    parse_runs_missing_original_chars: 0,
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

test("a populated aggregate renders both rates, counts, and per-format breakdown", () => {
  const stats: TruncationStats = {
    total_truncations: 3,
    total_docs_parsed: 11,
    unaligned_truncations: 0,
    total_original_chars: 100000,
    parse_runs_missing_original_chars: 0,
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

  // Scalar readout: the truncated-of-parsed doc rate (3/11 = 27.3%), reports
  // affected, and the chars-dropped-of-original ratio (29k/100k = 29.0%), each a
  // thousands-grouped "X of Y (Z%)".
  const values = section.findAll(".trunc-row dd").map((d) => d.text());
  expect(values).toContain("3 of 11 (27.3%)");
  expect(values).toContain("2");
  expect(values).toContain("29,000 of 100,000 (29.0%)");

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
    total_original_chars: 0,
    parse_runs_missing_original_chars: 0,
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
    total_original_chars: 0,
    parse_runs_missing_original_chars: 0,
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
    total_original_chars: 0,
    parse_runs_missing_original_chars: 0,
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

test("a chars-cohort gap suppresses the chars ratio while the doc rate still renders", () => {
  // The two ratios guard independently: every truncation report has a parse-run
  // (doc rate safe), but a parse run predates the chars denominator
  // (parse_runs_missing_original_chars > 0), so the chars ratio is withheld to
  // the bare count until that legacy row ages out — even though 29k/100k = 29.0%
  // would otherwise compute.
  const stats: TruncationStats = {
    total_truncations: 3,
    total_docs_parsed: 11,
    unaligned_truncations: 0,
    total_original_chars: 100000,
    parse_runs_missing_original_chars: 1,
    reports_affected: 2,
    total_chars_dropped: 29000,
    by_format: [{ format: "pdf", count: 3 }],
    latest_captured_at: "2026-06-08T09:00:00+00:00",
  };
  const wrapper = makeWrapper({ truncationStats: stats });
  const values = diagnostics(wrapper)
    .findAll(".trunc-row dd")
    .map((d) => d.text());
  // Doc rate still computed; chars ratio withheld to the bare grouped count.
  expect(values).toContain("3 of 11 (27.3%)");
  expect(values).toContain("29,000");
  expect(values).not.toContain("29,000 of 100,000 (29.0%)");
});

test("an unaligned truncation suppresses the chars ratio even with a nonzero denominator", () => {
  // A truncation whose report has no parse-run row (unaligned_truncations > 0)
  // puts its dropped chars in the numerator while its original chars never reach
  // total_original_chars — so the chars ratio is unmatched, exactly as the doc
  // rate is. The chars denominator is nonzero (a *newer* aligned run) and
  // parse_runs_missing_original_chars is 0, so only the unaligned arm can catch
  // it: 29k/100k = 29.0% would otherwise render dishonestly.
  const stats: TruncationStats = {
    total_truncations: 3,
    total_docs_parsed: 11,
    unaligned_truncations: 1,
    total_original_chars: 100000,
    parse_runs_missing_original_chars: 0,
    reports_affected: 2,
    total_chars_dropped: 29000,
    by_format: [{ format: "pdf", count: 3 }],
    latest_captured_at: "2026-06-08T09:00:00+00:00",
  };
  const wrapper = makeWrapper({ truncationStats: stats });
  const values = diagnostics(wrapper)
    .findAll(".trunc-row dd")
    .map((d) => d.text());
  // Both rates withheld (the unaligned gap hits both); chars shows the bare count.
  expect(values).toContain("29,000");
  expect(values).not.toContain("29,000 of 100,000 (29.0%)");
  expect(values.some((v) => v.includes("%"))).toBe(false);
});

// --- Charles Schwab connection ---------------------------------------------
// The credential + connect/disconnect surface: rendered on its own status channel,
// with save-before-connect gating and connection-state-driven copy and controls.

function schwabSection(wrapper: ReturnType<typeof makeWrapper>) {
  return wrapper.find('section[aria-labelledby="sec-schwab"]');
}

// The section's buttons are role-labelled by their text; find one by label.
function schwabButton(wrapper: ReturnType<typeof makeWrapper>, label: string) {
  return wrapper
    .findAll('section[aria-labelledby="sec-schwab"] button')
    .find((b) => b.text() === label);
}

const schwabConfigured: SchwabStatus = {
  client_id: "client-abc",
  secret_configured: true,
  connection: "not-connected",
  refresh_expires_at: null,
};

test("the Schwab section is omitted when status is unavailable (null)", () => {
  const wrapper = makeWrapper({ schwabStatus: null });
  expect(schwabSection(wrapper).exists()).toBe(false);
});

test("a clean install reads not connected and offers no Disconnect", () => {
  const wrapper = makeWrapper();
  expect(schwabSection(wrapper).find(".schwab-status").text()).toContain(
    "Not connected"
  );
  expect(schwabButton(wrapper, "Disconnect")).toBeUndefined();
});

test("save-schwab emits the client id and the typed secret", async () => {
  const wrapper = makeWrapper();
  await wrapper.find("#schwab-client-id").setValue("client-abc");
  await wrapper.find("#schwab-client-secret").setValue("dev-secret");
  await schwabButton(wrapper, "Save credentials")!.trigger("click");
  const saved = wrapper.emitted("save-schwab");
  expect(saved).toHaveLength(1);
  expect(saved![0][0]).toEqual({
    client_id: "client-abc",
    client_secret: "dev-secret",
  });
});

test("save-schwab leaves the secret null when only the client id changed", async () => {
  const wrapper = makeWrapper({ schwabStatus: schwabConfigured });
  await wrapper.find("#schwab-client-id").setValue("client-xyz");
  await schwabButton(wrapper, "Save credentials")!.trigger("click");
  expect(wrapper.emitted("save-schwab")![0][0]).toEqual({
    client_id: "client-xyz",
    client_secret: null,
  });
});

test("connect is disabled until both credentials are saved", () => {
  // client id saved but no secret configured → connect blocked.
  const wrapper = makeWrapper({
    schwabStatus: { ...schwabConfigured, secret_configured: false },
  });
  expect(schwabButton(wrapper, "Connect")!.attributes("disabled")).toBeDefined();
});

test("connect emits once saved credentials are complete and unedited", async () => {
  const wrapper = makeWrapper({ schwabStatus: schwabConfigured });
  const connect = schwabButton(wrapper, "Connect")!;
  expect(connect.attributes("disabled")).toBeUndefined();
  await connect.trigger("click");
  expect(wrapper.emitted("connect-schwab")).toHaveLength(1);
});

test("an unsaved edit re-disables connect (save before connecting)", async () => {
  const wrapper = makeWrapper({ schwabStatus: schwabConfigured });
  expect(
    schwabButton(wrapper, "Connect")!.attributes("disabled")
  ).toBeUndefined();
  await wrapper.find("#schwab-client-secret").setValue("rotated-secret");
  expect(schwabButton(wrapper, "Connect")!.attributes("disabled")).toBeDefined();
});

test("a busy run slot disables connect", () => {
  const wrapper = makeWrapper({ schwabStatus: schwabConfigured, schwabBusy: true });
  expect(schwabButton(wrapper, "Connect")!.attributes("disabled")).toBeDefined();
});

test("credential save is disabled while a connect or run is in flight", async () => {
  // A dirty edit that would otherwise be saveable, but a connect is mid-login: saving
  // now could swap the secret under the captured client id, so Save must be blocked.
  const connecting = makeWrapper({
    schwabStatus: schwabConfigured,
    schwabConnecting: true,
  });
  await connecting.find("#schwab-client-secret").setValue("rotated-secret");
  expect(
    schwabButton(connecting, "Save credentials")!.attributes("disabled")
  ).toBeDefined();

  // Same guard while any run holds the global slot.
  const busy = makeWrapper({ schwabStatus: schwabConfigured, schwabBusy: true });
  await busy.find("#schwab-client-secret").setValue("rotated-secret");
  expect(
    schwabButton(busy, "Save credentials")!.attributes("disabled")
  ).toBeDefined();
});

test("a connected account shows a live status, offers Reconnect, and Disconnect emits", async () => {
  const wrapper = makeWrapper({
    schwabStatus: {
      client_id: "client-abc",
      secret_configured: true,
      connection: "connected",
      refresh_expires_at: "2026-07-09T00:00:00+00:00",
    },
  });
  expect(schwabSection(wrapper).find(".schwab-status").text()).toContain(
    "Connected"
  );
  // Connect reads "Reconnect" once a session exists.
  expect(schwabButton(wrapper, "Reconnect")).toBeTruthy();
  const disconnect = schwabButton(wrapper, "Disconnect")!;
  expect(disconnect).toBeTruthy();
  await disconnect.trigger("click");
  expect(wrapper.emitted("disconnect-schwab")).toHaveLength(1);
});

test("a lapsed session reads as expired", () => {
  const wrapper = makeWrapper({
    schwabStatus: { ...schwabConfigured, connection: "expired" },
  });
  expect(schwabSection(wrapper).find(".schwab-status").text()).toContain(
    "expired"
  );
});
