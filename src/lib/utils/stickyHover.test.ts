import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createStickyHover } from "./stickyHover.js";

const LEAVE_DELAY_MS = 150;

function setup(leaveDelayMs = LEAVE_DELAY_MS) {
  const onChange = vi.fn<(index: number) => void>();
  const hover = createStickyHover({ leaveDelayMs, onChange });
  return { hover, onChange };
}

function reported(onChange: ReturnType<typeof vi.fn>): number[] {
  return onChange.mock.calls.map(([index]) => index as number);
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("createStickyHover", () => {
  it("reports a target as soon as the pointer enters it", () => {
    const { hover, onChange } = setup();
    hover.enter(2);
    expect(reported(onChange)).toEqual([2]);
    expect(hover.current).toBe(2);
  });

  it("hands over directly when the pointer crosses the gap to a neighbour", () => {
    const { hover, onChange } = setup();
    hover.enter(0);
    hover.leave();
    vi.advanceTimersByTime(LEAVE_DELAY_MS - 1);
    hover.enter(1);
    vi.advanceTimersByTime(LEAVE_DELAY_MS * 2);
    expect(reported(onChange)).toEqual([0, 1]);
    expect(hover.current).toBe(1);
  });

  it("stays silent when the pointer returns to the same target within the grace", () => {
    const { hover, onChange } = setup();
    hover.enter(0);
    hover.leave();
    hover.enter(0);
    vi.advanceTimersByTime(LEAVE_DELAY_MS * 2);
    expect(reported(onChange)).toEqual([0]);
    expect(hover.current).toBe(0);
  });

  it("clears the target once the grace passes without a re-entry", () => {
    const { hover, onChange } = setup();
    hover.enter(0);
    hover.leave();
    vi.advanceTimersByTime(LEAVE_DELAY_MS - 1);
    expect(hover.current).toBe(0);
    vi.advanceTimersByTime(1);
    expect(hover.current).toBe(-1);
    expect(reported(onChange)).toEqual([0, -1]);
  });

  it("clears at once when the grace is zero", () => {
    const { hover, onChange } = setup(0);
    hover.enter(0);
    hover.leave();
    expect(hover.current).toBe(-1);
    expect(reported(onChange)).toEqual([0, -1]);
  });

  it("ignores a leave while nothing is hovered", () => {
    const { hover, onChange } = setup();
    hover.leave();
    vi.advanceTimersByTime(LEAVE_DELAY_MS * 2);
    expect(onChange).not.toHaveBeenCalled();
  });

  it("clear() drops the target at once and cancels the pending leave", () => {
    const { hover, onChange } = setup();
    hover.enter(0);
    hover.leave();
    hover.clear();
    expect(hover.current).toBe(-1);
    expect(reported(onChange)).toEqual([0, -1]);
    vi.advanceTimersByTime(LEAVE_DELAY_MS * 2);
    expect(reported(onChange)).toEqual([0, -1]);
  });

  it("clear() is silent while nothing is hovered", () => {
    const { hover, onChange } = setup();
    hover.clear();
    expect(onChange).not.toHaveBeenCalled();
  });

  it("dispose() cancels the pending leave without reporting", () => {
    const { hover, onChange } = setup();
    hover.enter(0);
    hover.leave();
    hover.dispose();
    vi.advanceTimersByTime(LEAVE_DELAY_MS * 2);
    expect(reported(onChange)).toEqual([0]);
  });
});
