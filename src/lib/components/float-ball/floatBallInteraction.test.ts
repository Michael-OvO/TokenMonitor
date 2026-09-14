import { describe, expect, it } from "vitest";
import {
  detectScreenToPhysicalScale,
  getPhysicalWindowPositionFromPointer,
  shouldHandleFloatBallPointerButton,
} from "./floatBallInteraction.js";

describe("shouldHandleFloatBallPointerButton", () => {
  it("accepts left and right click on Linux", () => {
    expect(shouldHandleFloatBallPointerButton(0, true)).toBe(true);
    expect(shouldHandleFloatBallPointerButton(2, true)).toBe(true);
  });

  it("keeps right click behavior unchanged on non-Linux platforms", () => {
    expect(shouldHandleFloatBallPointerButton(0, false)).toBe(true);
    expect(shouldHandleFloatBallPointerButton(2, false)).toBe(true);
  });
});

describe("detectScreenToPhysicalScale", () => {
  it("detects physical screen coordinates", () => {
    expect(
      detectScreenToPhysicalScale({
        scale: 2,
        windowX: 400,
        windowY: 200,
        clientX: 28,
        clientY: 18,
        screenX: 456,
        screenY: 236,
      }),
    ).toBe(1);
  });

  it("detects logical screen coordinates that still need scaling", () => {
    expect(
      detectScreenToPhysicalScale({
        scale: 2,
        windowX: 400,
        windowY: 200,
        clientX: 28,
        clientY: 18,
        screenX: 228,
        screenY: 118,
      }),
    ).toBe(2);
  });
});

describe("getPhysicalWindowPositionFromPointer", () => {
  it("converts logical pointer deltas back to physical window coordinates", () => {
    expect(
      getPhysicalWindowPositionFromPointer({
        startScreenX: 228,
        startScreenY: 118,
        startWindowX: 400,
        startWindowY: 200,
        screenX: 238,
        screenY: 130,
        screenToPhysicalScale: 2,
      }),
    ).toEqual({
      x: 420,
      y: 224,
    });
  });

  it("leaves physical pointer deltas unchanged", () => {
    expect(
      getPhysicalWindowPositionFromPointer({
        startScreenX: 456,
        startScreenY: 236,
        startWindowX: 400,
        startWindowY: 200,
        screenX: 468,
        screenY: 244,
        screenToPhysicalScale: 1,
      }),
    ).toEqual({
      x: 412,
      y: 208,
    });
  });
});
