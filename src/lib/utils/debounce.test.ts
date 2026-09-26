import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { debounce } from "./debounce.js";

const SETTLE_MS = 800;

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("debounce", () => {
  it("runs once, the settle time after the last call", () => {
    const fn = vi.fn();
    const settled = debounce(fn, SETTLE_MS);
    settled.schedule();
    vi.advanceTimersByTime(500);
    settled.schedule();
    settled.schedule();
    vi.advanceTimersByTime(SETTLE_MS - 1);
    expect(fn).not.toHaveBeenCalled();

    vi.advanceTimersByTime(1);
    expect(fn).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(10 * SETTLE_MS);
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it("runs again for calls after it ran", () => {
    const fn = vi.fn();
    const settled = debounce(fn, SETTLE_MS);
    settled.schedule();
    vi.advanceTimersByTime(SETTLE_MS);
    settled.schedule();
    vi.advanceTimersByTime(SETTLE_MS);
    expect(fn).toHaveBeenCalledTimes(2);
  });

  it("flush runs a pending call at once, and nothing when none is pending", () => {
    const fn = vi.fn();
    const settled = debounce(fn, SETTLE_MS);
    settled.flush();
    expect(fn).not.toHaveBeenCalled();

    settled.schedule();
    settled.flush();
    expect(fn).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(10 * SETTLE_MS);
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it("cancel drops a pending call", () => {
    const fn = vi.fn();
    const settled = debounce(fn, SETTLE_MS);
    settled.schedule();
    settled.cancel();
    settled.flush();
    vi.advanceTimersByTime(10 * SETTLE_MS);
    expect(fn).not.toHaveBeenCalled();
  });
});
