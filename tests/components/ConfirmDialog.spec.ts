// ConfirmDialog.vue is the design package's confirmation dialog translated to
// Vue (preview/confirmation-dialog.html): presentational, parent-owned
// open/busy, confirm/cancel only ever emitted. Pins the dialog contract that
// carries real logic: the a11y wiring (role/aria-modal/labelledby/describedby),
// initial focus on Cancel (the safe action), Escape and scrim-click as Cancel —
// both inert while busy — and the busy state's disabled actions + status line.
//
// Mounted with `attachTo` so document.activeElement reflects focus moves (the
// component focuses via real DOM focus()).

import { test, expect, afterEach } from "vitest";
import { mount, VueWrapper } from "@vue/test-utils";
import { nextTick } from "vue";
import ConfirmDialog from "../../src/components/ConfirmDialog.vue";

const baseProps = {
  open: true,
  title: "Replace all analytical data?",
  body: "Importing this archive replaces everything. This cannot be undone.",
  confirmLabel: "Replace and import",
  busy: false,
  busyStatus: "Replacing all analytical data. This may take a moment.",
};

let wrapper: VueWrapper | null = null;

afterEach(() => {
  wrapper?.unmount();
  wrapper = null;
});

function makeWrapper(overrides: Partial<typeof baseProps> = {}) {
  wrapper = mount(ConfirmDialog, {
    props: { ...baseProps, ...overrides },
    attachTo: document.body,
  });
  return wrapper;
}

function buttons(w: VueWrapper) {
  const all = w.findAll("button");
  return {
    cancel: all.find((b) => b.text() === "Cancel")!,
    confirm: all.find((b) => b.text() === baseProps.confirmLabel)!,
  };
}

test("closed renders nothing; open renders the a11y-wired dialog", async () => {
  const w = makeWrapper({ open: false });
  expect(w.find(".dialog-scrim").exists()).toBe(false);

  await w.setProps({ open: true });
  const dialog = w.find('[role="dialog"]');
  expect(dialog.exists()).toBe(true);
  expect(dialog.attributes("aria-modal")).toBe("true");
  const titleId = dialog.attributes("aria-labelledby")!;
  const bodyId = dialog.attributes("aria-describedby")!;
  expect(w.find(`#${titleId}`).text()).toBe(baseProps.title);
  expect(w.find(`#${bodyId}`).text()).toBe(baseProps.body);
});

test("initial focus lands on Cancel, the safe action", async () => {
  const w = makeWrapper({ open: false });
  await w.setProps({ open: true });
  await nextTick();
  expect(document.activeElement?.textContent?.trim()).toBe("Cancel");
});

test("an optional detail renders as a second body paragraph", async () => {
  const w = makeWrapper();
  expect(w.find("#confirm-dialog-body").findAll("p")).toHaveLength(1);

  await w.setProps({
    detail:
      "The selected archive was created 2026-07-05 and holds 30 reports, 214 learnings, and 34 files.",
  });
  const paragraphs = w.find("#confirm-dialog-body").findAll("p");
  expect(paragraphs).toHaveLength(2);
  // Both paragraphs live inside the aria-describedby target, so a screen
  // reader announces the specifics along with the destructive scope.
  expect(paragraphs[1].text()).toContain("created 2026-07-05");
});

test("confirm and cancel clicks emit their events", async () => {
  const w = makeWrapper();
  const { cancel, confirm } = buttons(w);
  await confirm.trigger("click");
  expect(w.emitted("confirm")).toHaveLength(1);
  await cancel.trigger("click");
  expect(w.emitted("cancel")).toHaveLength(1);
});

test("Escape cancels — but not while busy", async () => {
  const w = makeWrapper({ open: false });
  await w.setProps({ open: true });
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
  expect(w.emitted("cancel")).toHaveLength(1);

  await w.setProps({ busy: true });
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
  expect(w.emitted("cancel")).toHaveLength(1); // unchanged
});

test("a click on the scrim cancels; a click inside the panel does not", async () => {
  const w = makeWrapper();
  await w.find(".dialog-body").trigger("click");
  expect(w.emitted("cancel")).toBeUndefined();
  await w.find(".dialog-scrim").trigger("click");
  expect(w.emitted("cancel")).toHaveLength(1);
});

test("busy disables both actions, shows the status line, and inerts the scrim", async () => {
  const w = makeWrapper({ busy: true });
  const { cancel, confirm } = buttons(w);
  expect(cancel.attributes("disabled")).toBeDefined();
  expect(confirm.attributes("disabled")).toBeDefined();
  const status = w.find(".dialog-status");
  expect(status.exists()).toBe(true);
  expect(status.attributes("role")).toBe("status");
  expect(status.text()).toBe(baseProps.busyStatus);

  await w.find(".dialog-scrim").trigger("click");
  await confirm.trigger("click");
  expect(w.emitted("cancel")).toBeUndefined();
  expect(w.emitted("confirm")).toBeUndefined();
});

test("closing restores focus to the element focused before opening", async () => {
  const opener = document.createElement("button");
  opener.textContent = "Import archive…";
  document.body.appendChild(opener);
  opener.focus();

  const w = makeWrapper({ open: false });
  await w.setProps({ open: true });
  await nextTick();
  expect(document.activeElement?.textContent?.trim()).toBe("Cancel");

  await w.setProps({ open: false });
  await nextTick();
  expect(document.activeElement).toBe(opener);
  opener.remove();
});
