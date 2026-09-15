// Component tests for ReasoningPane.vue — the bounded, self-scrolling reasoning
// well shared by every reasoning stream the tracker shows. These pin its
// rendering contract (caption + text), its scroll-group semantics (labeled,
// keyboard-reachable), and the per-pane auto-follow rule: pinned to the newest
// text while the reader sits at the bottom, left alone once they scroll up.
// happy-dom computes no layout, so the follow tests stub the well's scroll
// geometry on the element itself and assert what the component writes back.

import { test, expect } from "vitest";
import { mount } from "@vue/test-utils";
import { nextTick } from "vue";
import ReasoningPane from "../../src/components/ReasoningPane.vue";

function mountPane(text = "First thought.") {
  return mount(ReasoningPane, {
    props: { label: "Bull · Reasoning", accessibleLabel: "Bull reasoning", text },
  });
}

// Stub a fixed geometry on the well: a 600px-tall stream in a 200px-tall box,
// with a plain settable scrollTop the component's writes land on.
function stubGeometry(el: Element): { get top(): number } {
  let top = 0;
  Object.defineProperty(el, "scrollHeight", { configurable: true, get: () => 600 });
  Object.defineProperty(el, "clientHeight", { configurable: true, get: () => 200 });
  Object.defineProperty(el, "scrollTop", {
    configurable: true,
    get: () => top,
    set: (v: number) => {
      top = v;
    },
  });
  return {
    get top() {
      return top;
    },
  };
}

test("renders the caption and the streamed text in the well", () => {
  const wrapper = mountPane();
  expect(wrapper.find(".agent-thinking-label").text()).toBe("Bull · Reasoning");
  // Exact textContent, not the trimmed .text(): a <pre> preserves whitespace, so
  // template indentation leaking inside it would render as a blank first line.
  expect(wrapper.find(".agent-thinking-body").element.textContent).toBe("First thought.");
});

test("the well is a labeled, keyboard-reachable group rather than a landmark", () => {
  const well = mountPane().find(".agent-thinking-body");
  expect(well.attributes("role")).toBe("group");
  expect(well.attributes("aria-label")).toBe("Bull reasoning");
  expect(well.attributes("tabindex")).toBe("0");
});

test("a fresh mount with text already present starts at the tail", () => {
  // The tracker remounts when the user returns to it mid-run, so the pane must
  // not open on its first lines and jump on the next delta. The element does
  // not exist before mount, so the geometry is stubbed on the prototype and
  // restored afterwards.
  const proto = HTMLElement.prototype;
  const saved = {
    scrollHeight: Object.getOwnPropertyDescriptor(proto, "scrollHeight"),
    scrollTop: Object.getOwnPropertyDescriptor(proto, "scrollTop"),
  };
  let top = 0;
  Object.defineProperty(proto, "scrollHeight", { configurable: true, get: () => 600 });
  Object.defineProperty(proto, "scrollTop", {
    configurable: true,
    get: () => top,
    set: (v: number) => {
      top = v;
    },
  });
  try {
    mountPane("A long stream already on screen.");
    expect(top).toBe(600);
  } finally {
    for (const [name, desc] of Object.entries(saved)) {
      if (desc) Object.defineProperty(proto, name, desc);
      else delete (proto as unknown as Record<string, unknown>)[name];
    }
  }
});

test("follows the stream while the reader is at the bottom", async () => {
  const wrapper = mountPane();
  const el = wrapper.find(".agent-thinking-body").element;
  const geo = stubGeometry(el);
  await wrapper.setProps({ text: "First thought. Second thought." });
  await nextTick();
  expect(geo.top).toBe(600);
});

test("stops following once the reader scrolls up, and resumes at the bottom", async () => {
  const wrapper = mountPane();
  const well = wrapper.find(".agent-thinking-body");
  const geo = stubGeometry(well.element);

  // Scrolled well above the bottom (600 - 100 - 200 = 300px of slack): unpinned.
  well.element.scrollTop = 100;
  await well.trigger("scroll");
  await wrapper.setProps({ text: "First thought. Second thought." });
  await nextTick();
  expect(geo.top).toBe(100);

  // Back within the pin threshold (600 - 400 - 200 = 0px): follows again.
  well.element.scrollTop = 400;
  await well.trigger("scroll");
  await wrapper.setProps({ text: "First thought. Second thought. Third." });
  await nextTick();
  expect(geo.top).toBe(600);
});
