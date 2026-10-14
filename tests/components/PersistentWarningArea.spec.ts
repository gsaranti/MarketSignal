// PersistentWarningArea is presentational, so no Tauri mocks.

import { test, expect } from "vitest";
import { mount } from "@vue/test-utils";
import { nextTick } from "vue";
import PersistentWarningArea from "../../src/components/PersistentWarningArea.vue";
import { deepFreeze } from "../helpers/freeze";
import type { ValidationReport } from "../../src/types";

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
  expect(toggle.attributes("aria-expanded")).toBe("true");
  expect(wrapper.find("#warning-list").attributes("style") ?? "").not.toContain("display: none");

  await toggle.trigger("click");
  expect(toggle.attributes("aria-expanded")).toBe("false");
  // v-show collapses through the inline display:none.
  expect(wrapper.find("#warning-list").attributes("style") ?? "").toContain("display: none");
});

test("re-expands when a NEW warning kind appears after the user collapsed the band", async () => {
  const wrapper = mount(PersistentWarningArea, {
    props: { report: oneCategoryReport, error: null },
  });
  await wrapper.find(".warning-toggle").trigger("click"); // collapse
  expect(wrapper.find(".warning-toggle").attributes("aria-expanded")).toBe("false");

  await wrapper.setProps({ report: twoCategoryReport });
  await nextTick();
  expect(wrapper.find(".warning-toggle").attributes("aria-expanded")).toBe("true");
});

// Kinds match the backend's kebab-case `WarningKind` serialization.
const dismissibleReport: ValidationReport = deepFreeze({
  is_blocked: false,
  categories: [
    {
      kind: "failed-job",
      title: "Last job failed",
      items: ["2026-06-14 — provider unreachable"],
      dismiss_id: "7",
    },
  ],
});

test("offers a dismiss control on the non-blocking category and emits the kind + identity", async () => {
  const wrapper = mount(PersistentWarningArea, {
    props: { report: dismissibleReport, error: null },
  });
  const buttons = wrapper.findAll(".warning-dismiss");
  expect(buttons).toHaveLength(1);
  expect(buttons[0].attributes("aria-label")).toBe("Dismiss: Last job failed");

  await buttons[0].trigger("click");
  // The rendered identity rides along so the backend dismisses this warning,
  // not a newer one it would re-derive.
  expect(wrapper.emitted("dismiss")).toEqual([["failed-job", "7"]]);
});

test("blocking config categories and the error row carry no dismiss control", () => {
  const wrapper = mount(PersistentWarningArea, {
    props: { report: twoCategoryReport, error: "boom" },
  });
  expect(wrapper.findAll(".warning-dismiss")).toHaveLength(0);
});

test("stays collapsed when the signature changes but introduces no NEW warning kind", async () => {
  // The signature must actually change for the watcher to fire (an items-only
  // change would not move it), so a kind is dropped rather than items mutated:
  // every remaining kind was already present, so the collapse must survive.
  const wrapper = mount(PersistentWarningArea, {
    props: { report: twoCategoryReport, error: null },
  });
  await wrapper.find(".warning-toggle").trigger("click"); // collapse
  expect(wrapper.find(".warning-toggle").attributes("aria-expanded")).toBe("false");

  await wrapper.setProps({ report: oneCategoryReport });
  await nextTick();
  expect(wrapper.find(".warning-toggle").attributes("aria-expanded")).toBe("false");
});

// happy-dom has no layout, so the overflow is stubbed on the element.
test("the bounded list takes a tab stop only while it overflows", async () => {
  const failed: ValidationReport = deepFreeze({
    is_blocked: false,
    categories: [
      {
        kind: "failed-job",
        title: "Last run failed",
        items: ["Portfolio Analysis failed: " + "a long error chain; ".repeat(40)],
        dismiss_id: "run-1",
      },
    ],
  });
  const wrapper = mount(PersistentWarningArea, { props: { report: null, error: "x" } });
  const list = wrapper.get("#warning-list").element as HTMLElement;
  expect(list.hasAttribute("tabindex")).toBe(false);

  Object.defineProperty(list, "scrollHeight", { configurable: true, value: 600 });
  Object.defineProperty(list, "clientHeight", { configurable: true, value: 162 });
  await wrapper.setProps({ report: failed, error: null });
  await nextTick();
  await nextTick();
  expect(list.getAttribute("tabindex")).toBe("0");

  Object.defineProperty(list, "scrollHeight", { configurable: true, value: 40 });
  Object.defineProperty(list, "clientHeight", { configurable: true, value: 40 });
  await wrapper.setProps({ report: oneCategoryReport });
  await nextTick();
  await nextTick();
  expect(list.hasAttribute("tabindex")).toBe(false);
});
