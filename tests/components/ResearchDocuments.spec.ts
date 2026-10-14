import { test, expect } from "vitest";
import { mount, type DOMWrapper } from "@vue/test-utils";
import ResearchDocuments from "../../src/components/ResearchDocuments.vue";
import { deepFreeze } from "../helpers/freeze";
import type { ResearchDocument } from "../../src/types";

// Mounted at index 0, so its reason id is `parse-error-0`.
const failing: ResearchDocument = {
  name: "a-very-long-quarterly-macro-outlook-research-document-name.pdf",
  format: "pdf",
  supported: true,
  size_bytes: 2048,
  modified: "2026-06-10T12:00:00Z",
  parse_error: "Encrypted PDF: could not extract text",
};

const healthy: ResearchDocument = {
  name: "weekly-notes.md",
  format: "md",
  supported: true,
  size_bytes: 512,
  modified: "2026-06-11T09:00:00Z",
  parse_error: null,
};

deepFreeze(failing);
deepFreeze(healthy);

function makeWrapper(documents: ResearchDocument[]) {
  return mount(ResearchDocuments, {
    props: {
      documents,
      loading: false,
      error: null,
      title: "INBOX",
      lede: "Drop research files here.",
      emptyTitle: "Empty",
      emptyBody: "No documents yet.",
      errorLabel: "Couldn't read this folder",
      revealLabel: "Add files…",
      revealTitle: "Open the inbox folder",
    },
  });
}

test("applies a full-name title tooltip to every row name", () => {
  const wrapper = makeWrapper([failing, healthy]);
  const names = wrapper.findAll(".docs-row-name");
  expect(names).toHaveLength(2);
  // The name clips with ellipsis (nowrap); the unconditional title is the only
  // way a long failed-row name stays readable.
  expect(names[0].attributes("title")).toBe(failing.name);
  expect(names[1].attributes("title")).toBe(healthy.name);
});

test("ties the parse-failure reason to the failing row's Delete via aria-describedby; healthy rows carry none", () => {
  const wrapper = makeWrapper([failing, healthy]);
  const rows = wrapper.findAll(".docs-row");

  const reason = wrapper.find("#parse-error-0");
  expect(reason.exists()).toBe(true);
  expect(reason.text()).toBe(failing.parse_error);

  const failingDelete = rows[0].find(".docs-row-actions button");
  expect(failingDelete.attributes("aria-describedby")).toBe("parse-error-0");
  const healthyDelete = rows[1].find(".docs-row-actions button");
  expect(healthyDelete.attributes("aria-describedby")).toBeUndefined();
});

test("renders the parse-failed tag only on the failing row", () => {
  const wrapper = makeWrapper([failing, healthy]);
  const rows = wrapper.findAll(".docs-row");
  const tags = wrapper.findAll(".docs-tag--error");
  expect(tags).toHaveLength(1);
  expect(tags[0].text()).toBe("parse failed");
  expect(rows[0].find(".docs-tag--error").exists()).toBe(true);
  expect(rows[1].find(".docs-tag--error").exists()).toBe(false);
});

// By label rather than position, so a reordered Cancel/Delete pair cannot shift
// the assertions. The confirming control is itself labelled "Delete"; no button
// reads "Confirm".
function actionByLabel(row: DOMWrapper<Element>, label: string) {
  const btn = row.findAll(".docs-row-actions button").find((b) => b.text() === label);
  if (!btn) throw new Error(`no row action labelled "${label}"`);
  return btn;
}

test("inline delete is a two-step: the resting Delete reveals Cancel + a confirming Delete, which emits delete with the row name", async () => {
  const wrapper = makeWrapper([failing, healthy]);
  const row = wrapper.findAll(".docs-row")[0];

  const resting = row.findAll(".docs-row-actions button");
  expect(resting).toHaveLength(1);
  expect(resting[0].text()).toBe("Delete");
  await resting[0].trigger("click");

  expect(row.findAll(".docs-row-actions button").map((b) => b.text())).toEqual([
    "Cancel",
    "Delete",
  ]);
  expect(wrapper.emitted("delete")).toBeUndefined();

  await actionByLabel(row, "Delete").trigger("click");
  expect(wrapper.emitted("delete")).toEqual([[failing.name]]);
});

test("Cancel backs out of the confirm step without emitting delete", async () => {
  const wrapper = makeWrapper([failing, healthy]);
  const row = wrapper.findAll(".docs-row")[0];

  await actionByLabel(row, "Delete").trigger("click");
  await actionByLabel(row, "Cancel").trigger("click");

  expect(row.findAll(".docs-row-actions button")).toHaveLength(1);
  expect(wrapper.emitted("delete")).toBeUndefined();
});
