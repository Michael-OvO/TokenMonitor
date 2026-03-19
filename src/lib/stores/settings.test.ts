import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import type { Settings } from "./settings.js";

const mockLoad = vi.fn();
const mockSetCurrency = vi.fn();

vi.mock("@tauri-apps/plugin-store", () => ({
  load: (...args: unknown[]) => mockLoad(...args),
}));

vi.mock("../utils/format.js", () => ({
  setCurrency: (...args: unknown[]) => mockSetCurrency(...args),
}));

function makePersistedStore(saved: Partial<Settings> | null = {}) {
  return {
    get: vi.fn().mockResolvedValue(saved),
    set: vi.fn().mockResolvedValue(undefined),
    save: vi.fn().mockResolvedValue(undefined),
  };
}

async function loadSettingsModule() {
  return import("./settings.js");
}

beforeEach(() => {
  vi.resetModules();
  mockLoad.mockReset();
  mockSetCurrency.mockReset();
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("loadSettings", () => {
  it("merges persisted settings with defaults and updates the active currency", async () => {
    const store = makePersistedStore({
      theme: "light",
      defaultProvider: "codex",
      currency: "EUR",
      hiddenModels: ["haiku"],
      refreshInterval: 300,
      trayConfig: {
        barDisplay: 'both',
        barProvider: 'claude',
        showPercentages: false,
        percentageFormat: 'compact',
        showCost: false,
        costPrecision: 'full',
      },
    });
    mockLoad.mockResolvedValueOnce(store);

    const { loadSettings, settings } = await loadSettingsModule();
    const loaded = await loadSettings();

    expect(mockLoad).toHaveBeenCalledWith("settings.json", {
      defaults: {},
      autoSave: true,
    });
    expect(store.get).toHaveBeenCalledWith("settings");
    expect(loaded).toEqual({
      theme: "light",
      defaultProvider: "codex",
      defaultPeriod: "day",
      refreshInterval: 300,
      costAlertThreshold: 0,
      launchAtLogin: false,
      currency: "EUR",
      hiddenModels: ["haiku"],
      brandTheming: true,
      trayConfig: {
        barDisplay: 'both',
        barProvider: 'claude',
        showPercentages: false,
        percentageFormat: 'compact',
        showCost: false,
        costPrecision: 'full',
      },
      claudePlan: 0,
      codexPlan: 0,
      glassEffect: true,
    });
    expect(get(settings)).toEqual(loaded);
    expect(mockSetCurrency).toHaveBeenCalledWith("EUR");
  });

  it("falls back to defaults and resets the store state when loading fails", async () => {
    const store = makePersistedStore({
      theme: "light",
      currency: "EUR",
      defaultProvider: "codex",
    });
    mockLoad.mockResolvedValueOnce(store).mockRejectedValueOnce(new Error("disk read failed"));

    const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
    const { loadSettings, settings } = await loadSettingsModule();

    await loadSettings();
    mockSetCurrency.mockClear();

    const fallback = await loadSettings();

    expect(fallback).toEqual({
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
    });
    expect(get(settings)).toEqual(fallback);
    expect(mockSetCurrency).toHaveBeenCalledWith("USD");
    expect(warnSpy).toHaveBeenCalled();
  });
});

describe("loadSettings migration", () => {
  it("migrates showTrayAmount: true to trayConfig.showCost === true and barDisplay === 'off'", async () => {
    const legacy = { showTrayAmount: true } as unknown as Record<string, unknown>;
    const store = makePersistedStore(legacy as Partial<Settings>);
    mockLoad.mockResolvedValueOnce(store);

    const { loadSettings } = await loadSettingsModule();
    const loaded = await loadSettings();

    expect(loaded.trayConfig.showCost).toBe(true);
    expect(loaded.trayConfig.barDisplay).toBe('off');
  });

  it("migrates showTrayAmount: false to trayConfig.showCost === false and barDisplay === 'off'", async () => {
    const legacy = { showTrayAmount: false } as unknown as Record<string, unknown>;
    const store = makePersistedStore(legacy as Partial<Settings>);
    mockLoad.mockResolvedValueOnce(store);

    const { loadSettings } = await loadSettingsModule();
    const loaded = await loadSettings();

    expect(loaded.trayConfig.showCost).toBe(false);
    expect(loaded.trayConfig.barDisplay).toBe('off');
  });
});

describe("updateSetting", () => {
  it("updates the store, persists the merged payload, and applies currency changes immediately", async () => {
    const store = makePersistedStore({
      theme: "dark",
      currency: "USD",
      hiddenModels: ["haiku"],
    });
    mockLoad.mockResolvedValueOnce(store);

    const { loadSettings, settings, updateSetting } = await loadSettingsModule();
    await loadSettings();
    mockSetCurrency.mockClear();

    await updateSetting("currency", "GBP");

    expect(get(settings).currency).toBe("GBP");
    expect(store.set).toHaveBeenCalledWith(
      "settings",
      expect.objectContaining({
        currency: "GBP",
        hiddenModels: ["haiku"],
      }),
    );
    await vi.waitFor(() => {
      expect(store.save).toHaveBeenCalledTimes(1);
    });
    expect(mockSetCurrency).toHaveBeenCalledWith("GBP");
  });

  it("keeps the in-memory update and warns when persistence fails", async () => {
    const store = makePersistedStore({
      theme: "dark",
      currency: "USD",
      brandTheming: true,
    });
    store.set.mockRejectedValueOnce(new Error("disk full"));
    mockLoad.mockResolvedValueOnce(store);

    const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
    const { loadSettings, settings, updateSetting } = await loadSettingsModule();
    await loadSettings();

    await updateSetting("brandTheming", false);

    expect(get(settings).brandTheming).toBe(false);
    expect(store.save).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalledWith(
      "Failed to persist settings:",
      expect.any(Error),
    );
  });
});

describe("theme and provider application", () => {
  it("writes the expected data attributes onto the document root", async () => {
    const root = {
      setAttribute: vi.fn(),
      removeAttribute: vi.fn(),
    };
    vi.stubGlobal("document", {
      documentElement: root,
    });

    const { applyTheme, applyProvider } = await loadSettingsModule();

    applyTheme("light");
    expect(root.setAttribute).toHaveBeenCalledWith("data-theme", "light");

    applyTheme("system");
    expect(root.removeAttribute).toHaveBeenCalledWith("data-theme");

    applyProvider("codex", true);
    expect(root.setAttribute).toHaveBeenCalledWith("data-provider", "codex");

    applyProvider("all", true);
    applyProvider("claude", false);
    expect(root.removeAttribute).toHaveBeenCalledWith("data-provider");
  });
});
