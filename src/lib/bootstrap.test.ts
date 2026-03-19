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
    trayConfig: {
      barDisplay: 'both',
      barProvider: 'claude',
      showPercentages: false,
      percentageFormat: 'compact',
      showCost: true,
      costPrecision: 'full',
    },
    claudePlan: 0,
    codexPlan: 0,
    glassEffect: true,
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
    const applyGlassFn = vi.fn();
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
      applyGlassFn,
      syncNativeWindowSurfaceFn,
    });

    expect(applyThemeFn).toHaveBeenCalledWith("system");
    expect(applyGlassFn).toHaveBeenCalledWith(true);
    expect(invokeFn).toHaveBeenCalledWith("set_glass_effect", { enabled: true });
    expect(syncNativeWindowSurfaceFn).toHaveBeenCalledWith(invokeFn, true);
    expect(invokeFn).toHaveBeenCalledWith("set_refresh_interval", { interval: 300 });
    expect(invokeFn).toHaveBeenCalledWith("set_tray_config", {
      config: expect.objectContaining({ showCost: true }),
      claudeUtil: null,
      codexUtil: null,
    });
    expect(get(activeProvider)).toBe("codex");
    expect(get(activePeriod)).toBe("month");
    expect(runtime).toEqual({ provider: "codex", period: "month" });
  });

  it("keeps local startup state even when refresh interval IPC fails", async () => {
    const invokeFn = vi.fn().mockRejectedValue(new Error("ipc not ready"));
    const applyThemeFn = vi.fn();
    const applyGlassFn = vi.fn();
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
        applyGlassFn,
        syncNativeWindowSurfaceFn,
      }),
    ).resolves.toEqual({
      provider: "codex",
      period: "5h",
    });

    expect(applyThemeFn).toHaveBeenCalledWith("dark");
    expect(applyGlassFn).toHaveBeenCalledWith(true);
    expect(syncNativeWindowSurfaceFn).toHaveBeenCalledWith(invokeFn, true);
    expect(get(activeProvider)).toBe("codex");
    expect(get(activePeriod)).toBe("5h");
  });

  it("applies glass effect on startup", async () => {
    const invokeFn = vi.fn().mockResolvedValue(undefined);
    const applyGlassFn = vi.fn();
    const applyThemeFn = vi.fn();
    const syncNativeWindowSurfaceFn = vi.fn().mockResolvedValue(undefined);

    await initializeRuntimeFromSettings(
      makeSettings({ glassEffect: true }),
      { invokeFn, applyThemeFn, applyGlassFn, syncNativeWindowSurfaceFn },
    );

    expect(applyGlassFn).toHaveBeenCalledWith(true);
    expect(invokeFn).toHaveBeenCalledWith("set_glass_effect", { enabled: true });
    expect(syncNativeWindowSurfaceFn).toHaveBeenCalledWith(invokeFn, true);
  });

  it("does not enable glass when setting is false", async () => {
    const invokeFn = vi.fn().mockResolvedValue(undefined);
    const applyGlassFn = vi.fn();
    const applyThemeFn = vi.fn();
    const syncNativeWindowSurfaceFn = vi.fn().mockResolvedValue(undefined);

    await initializeRuntimeFromSettings(
      makeSettings({ glassEffect: false }),
      { invokeFn, applyThemeFn, applyGlassFn, syncNativeWindowSurfaceFn },
    );

    expect(applyGlassFn).toHaveBeenCalledWith(false);
    expect(invokeFn).toHaveBeenCalledWith("set_glass_effect", { enabled: false });
    expect(syncNativeWindowSurfaceFn).toHaveBeenCalledWith(invokeFn, false);
  });
});
