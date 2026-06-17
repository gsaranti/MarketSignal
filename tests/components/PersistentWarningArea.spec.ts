// Component tests for PersistentWarningArea.vue — props in, no emits (the collapse
// state is internal). Mock-free like ResearchDocuments/Settings (no @tauri-apps/api
// import). These pin the warning band's contract: the visibility gate (silent until
// there's something to say), the config-check error row, one row per backend
// category with its items joined, the collapsed issue-count pluralization, the
// disclosure a11y wiring, and the signature watcher that re-expands the band when a
// NEW warning appears after the user collapsed it.

import { test, expect } from "vitest";
import { mount } from "@vue/test-utils";
import { nextTick } from "vue";
import PersistentWarningArea from "../../src/components/PersistentWarningArea.vue";
import { deepFreeze } from "../helpers/freeze";
import type { ValidationReport } from "../../src/types";

// Two active categories — the band groups them into one block. Read-only by design;
// freezing makes a future in-place mutation throw at the write site.
const twoCategoryReport: ValidationReport = deepFreeze({
  is_blocked: true,
  categories: [
    { kind: "tokens", title: "Missing API tokens", items: ["OpenAI", "Anthropic"], dismiss_id: null },
    { kind: "providers", title: "Missing provider credentials", items: ["FMP", "Tavily"], dismiss_id: null },
  ],
});

const oneCategoryReport: ValidationReport = deepFreeze({
  is_blocked: true,
  categories: [{ kind: "tokens", title: "Missing API tokens", items: ["OpenAI"], dismiss_id: null }],
});

test("stays silent when there is nothing to report", () => {
  const wrapper = mount(PersistentWarningArea, { props: { report: null, error: null } });
  expect(wrapper.find(".warning-area").exists()).toBe(false);
});

test("renders one row per category with its items joined by '; '", () => {
  const wrapper = mount(PersistentWarningArea, {
    props: { report: twoCategoryReport, error: null },
  });
  const rows = wrapper.findAll(".warning-row");
  expect(rows).toHaveLength(2);
  expect(rows[0].find(".warning-label").text()).toBe("Missing API tokens");
  expect(rows[0].find(".warning-body").text()).toBe("OpenAI; Anthropic");
  expect(rows[1].find(".warning-body").text()).toBe("FMP; Tavily");
});

test("surfaces the config-check error as its own labelled row", () => {
  const wrapper = mount(PersistentWarningArea, {
    props: { report: null, error: "network down" },
  });
  const rows = wrapper.findAll(".warning-row");
  expect(rows).toHaveLength(1);
  expect(rows[0].find(".warning-label").text()).toBe("Configuration");
  expect(rows[0].find(".warning-body").text()).toBe(
    "Couldn't check configuration — network down"
  );
});

test("the collapsed count pluralizes on the issue total (error row included)", async () => {
  // One category + a config error = 2 issues.
  const wrapper = mount(PersistentWarningArea, {
    props: { report: oneCategoryReport, error: "boom" },
  });
  await wrapper.find(".warning-toggle").trigger("click"); // expanded -> collapsed
  expect(wrapper.find(".warning-count").text()).toContain("2 issues");

  // A single issue reads "1 issue".
  const single = mount(PersistentWarningArea, {
    props: { report: oneCategoryReport, error: null },
  });
  await single.find(".warning-toggle").trigger("click");
  expect(single.find(".warning-count").text()).toContain("1 issue");
});

test("the disclosure button wires aria-expanded/aria-controls and toggles the list", async () => {
  const wrapper = mount(PersistentWarningArea, {
    props: { report: twoCategoryReport, error: null },
  });
  const toggle = wrapper.find(".warning-toggle");
  expect(toggle.attributes("aria-controls")).toBe("warning-list");
  // Starts expanded (v-show leaves the list displayed).
  expect(toggle.attributes("aria-expanded")).toBe("true");
  expect(wrapper.find("#warning-list").attributes("style") ?? "").not.toContain("display: none");

  await toggle.trigger("click");
  expect(toggle.attributes("aria-expanded")).toBe("false");
  // Collapsed -> v-show sets the inline display:none.
  expect(wrapper.find("#warning-list").attributes("style") ?? "").toContain("display: none");
});

test("re-expands when a NEW warning kind appears after the user collapsed the band", async () => {
  const wrapper = mount(PersistentWarningArea, {
    props: { report: oneCategoryReport, error: null },
  });
  await wrapper.find(".warning-toggle").trigger("click"); // collapse
  expect(wrapper.find(".warning-toggle").attributes("aria-expanded")).toBe("false");

  // A new category kind ('providers') joins the signature -> the watcher re-expands.
  await wrapper.setProps({ report: twoCategoryReport });
  await nextTick();
  expect(wrapper.find(".warning-toggle").attributes("aria-expanded")).toBe("true");
});

// The two non-blocking categories carry a dismiss control; the blocking config
// categories (twoCategoryReport above) do not. Kinds match the backend's kebab-case
// `WarningKind` serialization.
const dismissibleReport: ValidationReport = deepFreeze({
  is_blocked: false,
  categories: [
    {
      kind: "failed-job",
      title: "Last job failed",
      items: ["2026-06-14 — provider unreachable"],
      dismiss_id: "7",
    },
    {
      kind: "missed-scheduled-job",
      title: "Scheduled job missed",
      items: ["The scheduled run for 2026-06-14 09:00 did not start."],
      dismiss_id: "2026-06-14T09:00:00+00:00",
    },
  ],
});

test("offers a dismiss control on the non-blocking categories and emits the kind + identity", async () => {
  const wrapper = mount(PersistentWarningArea, {
    props: { report: dismissibleReport, error: null },
  });
  const buttons = wrapper.findAll(".warning-dismiss");
  expect(buttons).toHaveLength(2);
  // The accessible name carries the category title (the icon itself is aria-hidden).
  expect(buttons[0].attributes("aria-label")).toBe("Dismiss: Last job failed");
  expect(buttons[1].attributes("aria-label")).toBe("Dismiss: Scheduled job missed");

  await buttons[0].trigger("click");
  await buttons[1].trigger("click");
  // Emits the rendered identity alongside the kind so the backend dismisses *this*
  // warning, not a newer one it would re-derive.
  expect(wrapper.emitted("dismiss")).toEqual([
    ["failed-job", "7"],
    ["missed-scheduled-job", "2026-06-14T09:00:00+00:00"],
  ]);
});

test("blocking config categories and the error row carry no dismiss control", () => {
  // twoCategoryReport is two blocking config categories; plus a config-check error row.
  const wrapper = mount(PersistentWarningArea, {
    props: { report: twoCategoryReport, error: "boom" },
  });
  expect(wrapper.findAll(".warning-dismiss")).toHaveLength(0);
});

test("stays collapsed when the signature changes but introduces no NEW warning kind", async () => {
  // The negative arm of the watcher: the signature must actually CHANGE for the
  // watcher to fire (an items-only change wouldn't move it), so we DROP a kind
  // ('providers') rather than mutate items. 'tokens,providers' -> 'tokens' fires
  // the watcher, but every remaining kind was already present, so `appeared` is
  // false and the user's collapse must survive. Mirror of the re-expand test above.
  const wrapper = mount(PersistentWarningArea, {
    props: { report: twoCategoryReport, error: null },
  });
  await wrapper.find(".warning-toggle").trigger("click"); // collapse
  expect(wrapper.find(".warning-toggle").attributes("aria-expanded")).toBe("false");

  // Removing the 'providers' kind changes the signature without adding one.
  await wrapper.setProps({ report: oneCategoryReport });
  await nextTick();
  expect(wrapper.find(".warning-toggle").attributes("aria-expanded")).toBe("false");
});
