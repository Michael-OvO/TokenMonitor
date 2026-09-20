import { afterEach, describe, expect, it, vi } from "vitest";
import { disclosureMotion } from "./disclosureMotion.js";

function setup(initiallyOpen = false, reduced = false) {
  let currentHeight = 0;
  let contentHeight = 240;
  const preference = {
    matches: reduced,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  };
  vi.stubGlobal("window", { matchMedia: () => preference });
  const animations: Array<{ cancel: ReturnType<typeof vi.fn>; onfinish?: (() => void) | null }> = [];
  const node = {
    style: { height: "" },
    getBoundingClientRect: () => ({ height: currentHeight }),
    firstElementChild: { getBoundingClientRect: () => ({ height: contentHeight }) },
    animate: vi.fn(() => {
      const animation = { cancel: vi.fn(() => { currentHeight = 0; }) };
      animations.push(animation);
      return animation;
    }),
  };
  const action = disclosureMotion(node as unknown as HTMLElement, initiallyOpen);
  return {
    action, node, preference, animations,
    setCurrentHeight: (height: number) => { currentHeight = height; },
    setContentHeight: (height: number) => { contentHeight = height; },
  };
}

afterEach(() => vi.unstubAllGlobals());

describe("disclosureMotion", () => {
  it("starts collapsed without an entrance animation and reveals the content", () => {
    const { action, node } = setup();
    expect(node.style.height).toBe("0px");
    expect(node.animate).not.toHaveBeenCalled();
    action.update(true);
    expect(node.animate).toHaveBeenCalledWith(
      [{ height: "0px" }, { height: "240px" }],
      expect.objectContaining({ duration: 180 }),
    );
    expect(node.style.height).toBe("auto");
  });

  it("reverses from the current visual height without snapping to an endpoint", () => {
    const { action, node, animations, setCurrentHeight } = setup();
    action.update(true);
    setCurrentHeight(84);
    action.update(false);
    expect(animations[0].cancel).toHaveBeenCalledOnce();
    expect(node.animate).toHaveBeenLastCalledWith(
      [{ height: "84px" }, { height: "0px" }],
      expect.objectContaining({ duration: 140 }),
    );
    expect(node.style.height).toBe("0px");
  });

  it("remeasures changed content when reopening and leaves expanded height automatic", () => {
    const { action, node, setContentHeight } = setup(true);
    expect(node.style.height).toBe("auto");
    action.update(false);
    setContentHeight(360);
    action.update(true);
    expect(node.animate).toHaveBeenLastCalledWith(
      [{ height: "0px" }, { height: "360px" }], expect.any(Object),
    );
    expect(node.style.height).toBe("auto");
  });

  it("does not restart animations for an unchanged state", () => {
    const { action, node } = setup();
    action.update(true);
    action.update(true);
    expect(node.animate).toHaveBeenCalledOnce();
  });

  it("applies changes immediately for reduced motion", () => {
    const { action, node } = setup(false, true);
    action.update(true);
    expect(node.style.height).toBe("auto");
    action.update(false);
    expect(node.style.height).toBe("0px");
    expect(node.animate).not.toHaveBeenCalled();
  });

  it("cancels motion when the preference changes and removes its listener on destruction", () => {
    const { action, node, preference, animations } = setup();
    action.update(true);
    const onChange = preference.addEventListener.mock.calls[0][1];
    preference.matches = true;
    onChange();
    expect(animations[0].cancel).toHaveBeenCalledOnce();
    expect(node.style.height).toBe("auto");
    action.destroy();
    expect(preference.removeEventListener).toHaveBeenCalledWith("change", onChange);
  });

  it("cancels an active animation when its component is destroyed", () => {
    const { action, animations } = setup();
    action.update(true);
    action.destroy();
    expect(animations[0].cancel).toHaveBeenCalledOnce();
  });

  it("accepts custom durations and reports when the motion settles", () => {
    const { action, node, animations, setCurrentHeight } = setup();
    const onSettled = vi.fn();
    action.update({ open: true, openMs: 200, closeMs: 160, onSettled });
    expect(node.animate).toHaveBeenLastCalledWith(
      [{ height: "0px" }, { height: "240px" }],
      expect.objectContaining({ duration: 200 }),
    );
    expect(onSettled).not.toHaveBeenCalled();
    animations[0].onfinish?.();
    expect(onSettled).toHaveBeenCalledExactlyOnceWith(true);

    setCurrentHeight(240);
    action.update({ open: false, openMs: 200, closeMs: 160, onSettled });
    expect(node.animate).toHaveBeenLastCalledWith(
      expect.any(Array),
      expect.objectContaining({ duration: 160 }),
    );
    animations[1].onfinish?.();
    expect(onSettled).toHaveBeenLastCalledWith(false);
  });

  it("does not report a settle for motion cut short by a reversal", () => {
    const { action, animations, setCurrentHeight } = setup();
    const onSettled = vi.fn();
    action.update({ open: true, onSettled });
    setCurrentHeight(84);
    action.update({ open: false, onSettled });
    expect(animations[0].cancel).toHaveBeenCalledOnce();
    expect(animations[0].onfinish).toBeNull();
    expect(onSettled).not.toHaveBeenCalled();
    animations[1].onfinish?.();
    expect(onSettled).toHaveBeenCalledExactlyOnceWith(false);
  });

  it("reports an immediate settle when reduced motion skips the animation", () => {
    const { action, node } = setup(false, true);
    const onSettled = vi.fn();
    action.update({ open: true, onSettled });
    expect(node.animate).not.toHaveBeenCalled();
    expect(onSettled).toHaveBeenCalledExactlyOnceWith(true);
  });
});
