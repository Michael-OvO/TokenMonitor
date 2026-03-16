import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import { initializeRuntimeFromSettings } from "./bootstrap.js";
import { activePeriod, activeProvider } from "./stores/usage.js";
import type { Settings } from "./stores/settings.js";

function makeSettings(overrides: Partial<Settings> = {}): Settings {
  return {
    theme: "dark",
    defaultProvider: "claude",
    defaultPeriod: "day",
    refreshInterval: 30,
    costAlertThreshold: 0,
    launchAtLogin: false,
    currency: "USD",
    hiddenModels: [],
    brandTheming: true,
    showTrayAmount: true,
    ...overrides,
  };
}

beforeEach(() => {
  activeProvider.set("claude");
  activePeriod.set("day");
});

describe("initializeRuntimeFromSettings", () => {
  it("applies provider/period stores and forwards refresh interval to the backend", async () => {
    const invokeFn = vi.fn().mockResolvedValue(undefined);
    const applyThemeFn = vi.fn();
    const syncNativeWindowSurfaceFn = vi.fn().mockResolvedValue(undefined);
    const saved = makeSettings({
      theme: "system",
      defaultProvider: "codex",
      defaultPeriod: "month",
      refreshInterval: 300,
    });

    const runtime = await initializeRuntimeFromSettings(saved, {
      invokeFn,
      applyThemeFn,
      syncNativeWindowSurfaceFn,
    });

    expect(applyThemeFn).toHaveBeenCalledWith("system");
    expect(syncNativeWindowSurfaceFn).toHaveBeenCalledWith(invokeFn);
    expect(invokeFn).toHaveBeenCalledWith("set_refresh_interval", { interval: 300 });
    expect(invokeFn).toHaveBeenCalledWith("set_show_tray_amount", { show: true });
    expect(get(activeProvider)).toBe("codex");
    expect(get(activePeriod)).toBe("month");
    expect(runtime).toEqual({ provider: "codex", period: "month" });
  });

  it("keeps local startup state even when refresh interval IPC fails", async () => {
    const invokeFn = vi.fn().mockRejectedValue(new Error("ipc not ready"));
    const applyThemeFn = vi.fn();
    const syncNativeWindowSurfaceFn = vi.fn().mockResolvedValue(undefined);
    const saved = makeSettings({
      defaultProvider: "codex",
      defaultPeriod: "5h",
      refreshInterval: 0,
    });

    await expect(
      initializeRuntimeFromSettings(saved, {
        invokeFn,
        applyThemeFn,
        syncNativeWindowSurfaceFn,
      }),
    ).resolves.toEqual({
      provider: "codex",
      period: "5h",
    });

    expect(applyThemeFn).toHaveBeenCalledWith("dark");
    expect(syncNativeWindowSurfaceFn).toHaveBeenCalledWith(invokeFn);
    expect(invokeFn).toHaveBeenCalledWith("set_refresh_interval", { interval: 0 });
    expect(get(activeProvider)).toBe("codex");
    expect(get(activePeriod)).toBe("5h");
  });
});
