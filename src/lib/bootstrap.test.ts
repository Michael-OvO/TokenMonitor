import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import { initializeRuntimeFromSettings } from "./bootstrap.js";
import { activePeriod, activeProvider } from "./stores/usage.js";
import type { Settings } from "./stores/settings.js";

let mockIsMacOS = true;
let mockUsesFloatingStatusWidget = false;
let mockIsWindows = false;

// Mock platform helpers so tests exercise all IPC paths deterministically.
vi.mock("./utils/platform.js", () => ({
  isMacOS: () => mockIsMacOS,
  isWindows: () => mockIsWindows,
  usesFloatingStatusWidget: () => mockUsesFloatingStatusWidget,
}));

// Mock updater store so bootstrap tests don't depend on Tauri event listeners.
vi.mock("./stores/updater.js", () => ({
  installUpdaterListeners: vi.fn().mockResolvedValue(undefined),
  hydrateUpdater: vi.fn().mockResolvedValue(undefined),
}));

function makeSettings(overrides: Partial<Settings> = {}): Settings {
  return {
    theme: "dark",
    defaultProvider: "claude",
    defaultPeriod: "day",
    refreshInterval: 30,
    costAlertThreshold: 0,
    launchAtLogin: false,
    showDockIcon: false,
    currency: "USD",
    hiddenModels: [],
    headerTabs: {
      all: { label: "All", enabled: true },
      claude: { label: "Claude", enabled: true },
      codex: { label: "Codex", enabled: true },
    },
    brandTheming: true,
    trayConfig: {
      barDisplay: 'both',
      barProvider: 'claude',
      showPercentages: false,
      percentageFormat: 'compact',
      showCost: true,
      costPrecision: 'full',
    },
    glassEffect: true,
    showModelChangeStats: false,
    floatBall: false,
    taskbarPanel: false,
    sshHosts: [],
    debugLogging: false,
    rateLimitsEnabled: false,
    hasSeenWelcome: true,
    keychainAccessRequested: true,
    ...overrides,
  };
}

beforeEach(() => {
  activeProvider.set("claude");
  activePeriod.set("day");
  mockIsMacOS = true;
  mockIsWindows = false;
  mockUsesFloatingStatusWidget = false;
});

describe("initializeRuntimeFromSettings", () => {
  it("applies provider/period stores and forwards refresh interval to the backend", async () => {
    const invokeFn = vi.fn().mockResolvedValue(undefined);
    const applyThemeFn = vi.fn();
    const applyGlassFn = vi.fn();
    const syncNativeWindowThemeFn = vi.fn().mockResolvedValue(undefined);
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
      syncNativeWindowThemeFn,
      syncNativeWindowSurfaceFn,
    });

    expect(applyThemeFn).toHaveBeenCalledWith("system");
    expect(applyGlassFn).toHaveBeenCalledWith(true);
    expect(syncNativeWindowThemeFn).toHaveBeenCalledWith("system");
    // Native glass effect is applied via Tauri Window API (setEffects), not invokeFn.
    expect(invokeFn).toHaveBeenCalledWith("set_dock_icon_visible", { visible: false });
    expect(syncNativeWindowSurfaceFn).toHaveBeenCalledWith(invokeFn, true);
    expect(invokeFn).toHaveBeenCalledWith("set_refresh_interval", { interval: 300 });
    expect(invokeFn).toHaveBeenCalledWith("set_usage_access_enabled", { enabled: true });
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
    const syncNativeWindowThemeFn = vi.fn().mockResolvedValue(undefined);
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
        syncNativeWindowThemeFn,
        syncNativeWindowSurfaceFn,
      }),
    ).resolves.toEqual({
      provider: "codex",
      period: "5h",
    });

    expect(applyThemeFn).toHaveBeenCalledWith("dark");
    expect(applyGlassFn).toHaveBeenCalledWith(true);
    expect(syncNativeWindowThemeFn).toHaveBeenCalledWith("dark");
    expect(invokeFn).toHaveBeenCalledWith("set_dock_icon_visible", { visible: false });
    expect(syncNativeWindowSurfaceFn).toHaveBeenCalledWith(invokeFn, true);
    expect(get(activeProvider)).toBe("codex");
    expect(get(activePeriod)).toBe("5h");
  });

  it("keeps usage access disabled until the first-run disclosure is dismissed", async () => {
    const invokeFn = vi.fn().mockResolvedValue(undefined);
    const applyGlassFn = vi.fn();
    const applyThemeFn = vi.fn();
    const syncNativeWindowThemeFn = vi.fn().mockResolvedValue(undefined);
    const syncNativeWindowSurfaceFn = vi.fn().mockResolvedValue(undefined);

    await initializeRuntimeFromSettings(
      makeSettings({ hasSeenWelcome: false }),
      { invokeFn, applyThemeFn, applyGlassFn, syncNativeWindowThemeFn, syncNativeWindowSurfaceFn },
    );

    expect(invokeFn).toHaveBeenCalledWith("set_usage_access_enabled", { enabled: false });
  });

  it("falls back to a visible provider when the saved default tab is hidden", async () => {
    const invokeFn = vi.fn().mockResolvedValue(undefined);
    const applyGlassFn = vi.fn();
    const applyThemeFn = vi.fn();
    const syncNativeWindowThemeFn = vi.fn().mockResolvedValue(undefined);
    const syncNativeWindowSurfaceFn = vi.fn().mockResolvedValue(undefined);

    const runtime = await initializeRuntimeFromSettings(
      makeSettings({
        defaultProvider: "codex",
        headerTabs: {
          all: { label: "Overview", enabled: true },
          claude: { label: "Claude", enabled: false },
          codex: { label: "Codex", enabled: false },
        },
      }),
      { invokeFn, applyThemeFn, applyGlassFn, syncNativeWindowThemeFn, syncNativeWindowSurfaceFn },
    );

    expect(get(activeProvider)).toBe("all");
    expect(runtime).toEqual({ provider: "all", period: "day" });
  });

  it("applies glass effect on startup", async () => {
    const invokeFn = vi.fn().mockResolvedValue(undefined);
    const applyGlassFn = vi.fn();
    const applyThemeFn = vi.fn();
    const syncNativeWindowThemeFn = vi.fn().mockResolvedValue(undefined);
    const syncNativeWindowSurfaceFn = vi.fn().mockResolvedValue(undefined);

    await initializeRuntimeFromSettings(
      makeSettings({ glassEffect: true }),
      { invokeFn, applyThemeFn, applyGlassFn, syncNativeWindowThemeFn, syncNativeWindowSurfaceFn },
    );

    expect(applyGlassFn).toHaveBeenCalledWith(true);
    expect(syncNativeWindowThemeFn).toHaveBeenCalledWith("dark");
    // Native glass effect is applied via Tauri Window API (setEffects), not invokeFn.
    expect(invokeFn).toHaveBeenCalledWith("set_dock_icon_visible", { visible: false });
    expect(syncNativeWindowSurfaceFn).toHaveBeenCalledWith(invokeFn, true);
  });

  it("does not enable glass when setting is false", async () => {
    const invokeFn = vi.fn().mockResolvedValue(undefined);
    const applyGlassFn = vi.fn();
    const applyThemeFn = vi.fn();
    const syncNativeWindowThemeFn = vi.fn().mockResolvedValue(undefined);
    const syncNativeWindowSurfaceFn = vi.fn().mockResolvedValue(undefined);

    await initializeRuntimeFromSettings(
      makeSettings({ glassEffect: false }),
      { invokeFn, applyThemeFn, applyGlassFn, syncNativeWindowThemeFn, syncNativeWindowSurfaceFn },
    );

    expect(applyGlassFn).toHaveBeenCalledWith(false);
    expect(syncNativeWindowThemeFn).toHaveBeenCalledWith("dark");
    // Native glass effect is applied via Tauri Window API (setEffects), not invokeFn.
    expect(invokeFn).toHaveBeenCalledWith("set_dock_icon_visible", { visible: false });
    expect(syncNativeWindowSurfaceFn).toHaveBeenCalledWith(invokeFn, false);
  });

  it("applies dock icon visibility on startup", async () => {
    const invokeFn = vi.fn().mockResolvedValue(undefined);
    const applyGlassFn = vi.fn();
    const applyThemeFn = vi.fn();
    const syncNativeWindowThemeFn = vi.fn().mockResolvedValue(undefined);
    const syncNativeWindowSurfaceFn = vi.fn().mockResolvedValue(undefined);

    await initializeRuntimeFromSettings(
      makeSettings({ showDockIcon: true }),
      { invokeFn, applyThemeFn, applyGlassFn, syncNativeWindowThemeFn, syncNativeWindowSurfaceFn },
    );

    expect(invokeFn).toHaveBeenCalledWith("set_dock_icon_visible", { visible: true });
  });

  it("creates the floating widget when floatBall setting is enabled", async () => {
    mockIsMacOS = false;
    mockUsesFloatingStatusWidget = true;
    const invokeFn = vi.fn().mockResolvedValue(undefined);
    const applyGlassFn = vi.fn();
    const applyThemeFn = vi.fn();
    const syncNativeWindowThemeFn = vi.fn().mockResolvedValue(undefined);
    const syncNativeWindowSurfaceFn = vi.fn().mockResolvedValue(undefined);

    await initializeRuntimeFromSettings(
      makeSettings({ floatBall: true }),
      { invokeFn, applyThemeFn, applyGlassFn, syncNativeWindowThemeFn, syncNativeWindowSurfaceFn },
    );

    expect(invokeFn).toHaveBeenCalledWith("create_float_ball");
  });

  it("does not create the floating widget when floatBall setting is disabled", async () => {
    mockIsMacOS = false;
    mockUsesFloatingStatusWidget = true;
    const invokeFn = vi.fn().mockResolvedValue(undefined);
    const applyGlassFn = vi.fn();
    const applyThemeFn = vi.fn();
    const syncNativeWindowThemeFn = vi.fn().mockResolvedValue(undefined);
    const syncNativeWindowSurfaceFn = vi.fn().mockResolvedValue(undefined);

    await initializeRuntimeFromSettings(
      makeSettings({ floatBall: false }),
      { invokeFn, applyThemeFn, applyGlassFn, syncNativeWindowThemeFn, syncNativeWindowSurfaceFn },
    );

    expect(invokeFn).not.toHaveBeenCalledWith("create_float_ball");
  });
});
