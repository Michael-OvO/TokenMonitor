import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import { popoverVisible, refreshWhenVisible, trackPopoverVisibility } from "./visibility.js";

beforeEach(() => popoverVisible.set(true));

describe("trackPopoverVisibility", () => {
  it("seeds from the window, but never over an event that came first", async () => {
    let send: (visible: boolean) => void = () => {};
    const onEvent = async (handler: (visible: boolean) => void) => {
      send = handler;
      return () => {};
    };

    await trackPopoverVisibility(onEvent, async () => false);
    expect(get(popoverVisible)).toBe(false);

    await trackPopoverVisibility(onEvent, async () => {
      send(true); // shown while the seed was in flight
      return false;
    });
    expect(get(popoverVisible)).toBe(true);
  });
});

describe("refreshWhenVisible", () => {
  it("runs at once while shown and once at the next show after any number of hidden requests", () => {
    const refresh = vi.fn();
    const view = refreshWhenVisible(refresh);

    view.request();
    expect(refresh).toHaveBeenCalledTimes(1);

    popoverVisible.set(false);
    view.request();
    view.request();
    expect(refresh).toHaveBeenCalledTimes(1);

    popoverVisible.set(true);
    expect(refresh).toHaveBeenCalledTimes(2);
    popoverVisible.set(false);
    popoverVisible.set(true);
    expect(refresh).toHaveBeenCalledTimes(2);
    view.stop();
  });

  it("still runs while hidden once the last run is old enough", () => {
    vi.useFakeTimers();
    try {
      const refresh = vi.fn();
      const view = refreshWhenVisible(refresh, 60_000);
      popoverVisible.set(false);

      view.request();
      expect(refresh).not.toHaveBeenCalled();
      vi.advanceTimersByTime(60_000);
      view.request();
      expect(refresh).toHaveBeenCalledTimes(1);

      popoverVisible.set(true);
      expect(refresh).toHaveBeenCalledTimes(1);
      view.stop();
    } finally {
      vi.useRealTimers();
    }
  });
});
