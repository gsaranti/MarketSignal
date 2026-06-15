// Component tests for ResearchDocuments.vue — the first SFC behavioral coverage
// in the project. Run via `npm test` (the `vitest run` leg) or `npx vitest run`.
// These pin the accessibility contract of the inbox failed-row that previously
// had no automated coverage (the type-check was the only floor): the full-name
// `:title` tooltip on the clipped name, the `aria-describedby` pairing the
// parse-failure reason to the row's Delete control, the error tag, and the
// inline two-step delete confirmation. Mounting the SFC here also proves the
// Vitest + @vue/test-utils + happy-dom toolchain compiles and mounts `.vue`.

import { test, expect } from "vitest";
import { mount, type DOMWrapper } from "@vue/test-utils";
import ResearchDocuments from "../../src/components/ResearchDocuments.vue";
import { deepFreeze } from "../helpers/freeze";
import type { ResearchDocument } from "../../src/types";

// A failing row (index 0, so its reason id is `parse-error-0`) followed by a
// healthy one — the pair lets each assertion contrast the two states.
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

// Shared at module scope and reused across tests via fresh wrapper arrays. They're
// read-only by design; deep-freezing makes that a guarantee — a future in-place
// mutation throws at the write rather than leaking into a later test.
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

  // The reason paragraph carries the index-keyed id the Delete references.
  const reason = wrapper.find("#parse-error-0");
  expect(reason.exists()).toBe(true);
  expect(reason.text()).toBe(failing.parse_error);

  // Resting Delete on the failing row points at that reason; the healthy row's
  // Delete has no describedby to dangle.
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

// Reach the confirm-state controls by their visible label rather than position,
// so the two-step assertions don't silently shift if the Cancel/Delete pair is
// ever reordered. Note the confirming control is itself labelled "Delete" (the
// danger button) — there is no button literally reading "Confirm".
function actionByLabel(row: DOMWrapper<Element>, label: string) {
  const btn = row.findAll(".docs-row-actions button").find((b) => b.text() === label);
  if (!btn) throw new Error(`no row action labelled "${label}"`);
  return btn;
}

test("inline delete is a two-step: the resting Delete reveals Cancel + a confirming Delete, which emits delete with the row name", async () => {
  const wrapper = makeWrapper([failing, healthy]);
  const row = wrapper.findAll(".docs-row")[0];

  // At rest the row shows a single "Delete" control.
  const resting = row.findAll(".docs-row-actions button");
  expect(resting).toHaveLength(1);
  expect(resting[0].text()).toBe("Delete");
  await resting[0].trigger("click");

  // Now a Cancel and a (danger) Delete are shown, and nothing has emitted yet.
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

  // Back to the single resting Delete, no emission.
  expect(row.findAll(".docs-row-actions button")).toHaveLength(1);
  expect(wrapper.emitted("delete")).toBeUndefined();
});
