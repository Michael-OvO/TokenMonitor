import { describe, expect, it, vi } from "vitest";
import { scrollFade } from "./scrollFade.js";

function setup(scrollHeight: number, clientHeight = 108) {
  const attrs = new Set<string>();
  const listeners = new Map<string, () => void>();
  const node = {
    scrollTop: 0,
    clientHeight,
    scrollHeight,
    toggleAttribute: vi.fn((name: string, force: boolean) => {
      if (force) attrs.add(name);
      else attrs.delete(name);
    }),
    addEventListener: vi.fn((type: string, fn: () => void) => listeners.set(type, fn)),
    removeEventListener: vi.fn((type: string) => listeners.delete(type)),
  };
  const action = scrollFade(node as unknown as HTMLElement);
  return {
    node, action, listeners,
    moreBelow: () => attrs.has("data-more-below"),
  };
}

describe("scrollFade", () => {
  it("leaves a list that fits unmarked", () => {
    expect(setup(93).moreBelow()).toBe(false);
  });

  it("marks an overflowing list until it is scrolled to the bottom", () => {
    const { node, listeners, moreBelow } = setup(140);
    expect(moreBelow()).toBe(true);
    node.scrollTop = 31.5;
    listeners.get("scroll")?.();
    expect(moreBelow()).toBe(false);
    node.scrollTop = 10;
    listeners.get("scroll")?.();
    expect(moreBelow()).toBe(true);
  });

  it("ignores a sub-pixel remainder at the bottom", () => {
    expect(setup(108.6).moreBelow()).toBe(false);
  });

  it("stops listening on destroy", () => {
    const { action, node } = setup(140);
    action.destroy();
    expect(node.removeEventListener).toHaveBeenCalledWith("scroll", expect.any(Function));
  });
});
