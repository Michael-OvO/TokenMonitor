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

vi.mock("../utils/logger.js", () => ({
  logger: { info: vi.fn(), debug: vi.fn(), warn: vi.fn(), error: vi.fn() },
}));

const DEFAULT_HEADER_TABS = {
  all: { label: "All", enabled: true },
  claude: { label: "Claude", enabled: true },
  codex: { label: "Codex", enabled: true },
} as const;

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
      showDockIcon: false,
      currency: "EUR",
      hiddenModels: ["haiku"],
      headerTabs: DEFAULT_HEADER_TABS,
      brandTheming: true,
      trayConfig: {
        barDisplay: 'both',
        barProvider: 'claude',
        showPercentages: false,
        percentageFormat: 'compact',
        showCost: false,
        costPrecision: 'full',
      },
      glassEffect: false,
      showModelChangeStats: false,
      floatBall: false,
      taskbarPanel: false,
      sshHosts: [],
      debugLogging: false,
      // Existing installs (non-empty saved settings) migrate to rate-limits
      // on, welcome-seen, AND keychain-prompt-already-shown so we don't pop
      // a tutorial on someone who's been using the app for months.
      rateLimitsEnabled: true,
      hasSeenWelcome: true,
      keychainAccessRequested: true,
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
      theme: "system",
      defaultProvider: "claude",
      defaultPeriod: "day",
      refreshInterval: 30,
      costAlertThreshold: 0,
      launchAtLogin: false,
      showDockIcon: false,
      currency: "USD",
      hiddenModels: [],
      headerTabs: DEFAULT_HEADER_TABS,
      brandTheming: true,
      trayConfig: {
        barDisplay: 'both',
        barProvider: 'claude',
        showPercentages: false,
        percentageFormat: 'compact',
        showCost: true,
        costPrecision: 'full',
      },
      glassEffect: false,
      showModelChangeStats: false,
      floatBall: false,
      taskbarPanel: false,
      sshHosts: [],
      debugLogging: false,
      // Failure path goes through pure defaults (no migration), so the
      // opt-in flags stay off.
      rateLimitsEnabled: false,
      hasSeenWelcome: false,
      keychainAccessRequested: false,
    });
    expect(get(settings)).toEqual(fallback);
    expect(mockSetCurrency).toHaveBeenCalledWith("USD");
    expect(warnSpy).toHaveBeenCalled();
  });

  it("defaults showModelChangeStats to false", async () => {
    mockLoad.mockResolvedValueOnce(makePersistedStore({}));
    const { loadSettings, settings } = await loadSettingsModule();
    await loadSettings();
    expect(get(settings).showModelChangeStats).toBe(false);
  });

  it("defaults showDockIcon to false", async () => {
    mockLoad.mockResolvedValueOnce(makePersistedStore({}));
    const { loadSettings, settings } = await loadSettingsModule();
    await loadSettings();
    expect(get(settings).showDockIcon).toBe(false);
  });

  it("normalizes ssh host include_in_stats flags", async () => {
    const store = makePersistedStore({
      sshHosts: [
        { alias: "devbox", enabled: true, include_in_stats: true },
        { alias: "lab", enabled: false },
      ],
    });
    mockLoad.mockResolvedValueOnce(store);

    const { loadSettings } = await loadSettingsModule();
    const loaded = await loadSettings();

    expect(loaded.sshHosts).toEqual([
      { alias: "devbox", enabled: true, include_in_stats: true },
      { alias: "lab", enabled: false, include_in_stats: false },
    ]);
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

  it("falls back to the first visible header tab when the saved default provider is hidden", async () => {
    const store = makePersistedStore({
      defaultProvider: "codex",
      headerTabs: {
        all: { label: "Overview", enabled: false },
        claude: { label: "Claude Code", enabled: true },
        codex: { label: "Codex", enabled: false },
      },
    });
    mockLoad.mockResolvedValueOnce(store);

    const { loadSettings } = await loadSettingsModule();
    const loaded = await loadSettings();

    expect(loaded.defaultProvider).toBe("claude");
    expect(loaded.headerTabs).toEqual({
      all: { label: "Overview", enabled: false },
      claude: { label: "Claude Code", enabled: true },
      codex: { label: "Codex", enabled: false },
    });
  });

  it("normalizes invalid persisted values back into a coherent settings graph", async () => {
    const store = makePersistedStore({
      theme: "midnight" as Settings["theme"],
      defaultProvider: "mystery" as Settings["defaultProvider"],
      defaultPeriod: "year" as Settings["defaultPeriod"],
      refreshInterval: "15" as unknown as Settings["refreshInterval"],
      costAlertThreshold: "-42.345" as unknown as Settings["costAlertThreshold"],
      launchAtLogin: "yes" as unknown as Settings["launchAtLogin"],
      currency: "cad",
      hiddenModels: ["", " Haiku ", "haiku", " GPT-5 ", 7 as unknown as string],
      headerTabs: {
        all: { label: "   ", enabled: false },
        claude: { label: "Claude Claude Claude", enabled: false },
        codex: { label: "Codex", enabled: false },
      },
      brandTheming: "yes" as unknown as Settings["brandTheming"],
      trayConfig: {
        barDisplay: "triple" as Settings["trayConfig"]["barDisplay"],
        barProvider: "all" as Settings["trayConfig"]["barProvider"],
        showPercentages: "yes" as unknown as Settings["trayConfig"]["showPercentages"],
        percentageFormat: "long" as Settings["trayConfig"]["percentageFormat"],
        showCost: "no" as unknown as Settings["trayConfig"]["showCost"],
        costPrecision: "many" as Settings["trayConfig"]["costPrecision"],
      },
      glassEffect: "no" as unknown as Settings["glassEffect"],
    });
    mockLoad.mockResolvedValueOnce(store);

    const { MAX_HEADER_TAB_LABEL_LENGTH, loadSettings } = await loadSettingsModule();
    const loaded = await loadSettings();

    expect(loaded).toMatchObject({
      theme: "system",
      defaultProvider: "all",
      defaultPeriod: "day",
      refreshInterval: 30,
      costAlertThreshold: 0,
      launchAtLogin: false,
      currency: "USD",
      hiddenModels: ["haiku", "gpt-5"],
      brandTheming: true,
      trayConfig: {
        barDisplay: "both",
        barProvider: "claude",
        showPercentages: false,
        percentageFormat: "compact",
        showCost: true,
        costPrecision: "full",
      },
      glassEffect: false,
    });
    expect(loaded.headerTabs).toEqual({
      all: { label: "All", enabled: true },
      claude: {
        label: "Claude Claude Clau".slice(0, MAX_HEADER_TAB_LABEL_LENGTH),
        enabled: false,
      },
      codex: { label: "Codex", enabled: false },
    });
    expect(loaded.headerTabs.claude.label).toHaveLength(MAX_HEADER_TAB_LABEL_LENGTH);
  });
});

describe("header tab helpers", () => {
  it("treats copied header tab objects with the same values as equal", async () => {
    const { areHeaderTabsEqual } = await loadSettingsModule();

    expect(
      areHeaderTabsEqual(DEFAULT_HEADER_TABS, {
        all: { label: "All", enabled: true },
        claude: { label: "Claude", enabled: true },
        codex: { label: "Codex", enabled: true },
      }),
    ).toBe(true);
  });

  it("detects label and enabled changes in header tabs", async () => {
    const { areHeaderTabsEqual } = await loadSettingsModule();

    expect(
      areHeaderTabsEqual(DEFAULT_HEADER_TABS, {
        all: { label: "Overview", enabled: true },
        claude: { label: "Claude", enabled: true },
        codex: { label: "Codex", enabled: true },
      }),
    ).toBe(false);

    expect(
      areHeaderTabsEqual(DEFAULT_HEADER_TABS, {
        all: { label: "All", enabled: true },
        claude: { label: "Claude", enabled: false },
        codex: { label: "Codex", enabled: true },
      }),
    ).toBe(false);
  });

  it("forces at least one visible header tab when all are disabled", async () => {
    const { normalizeHeaderTabs, resolveVisibleProvider } = await loadSettingsModule();

    const normalized = normalizeHeaderTabs({
      all: { label: "Overview", enabled: false },
      claude: { label: "Claude Code", enabled: false },
      codex: { label: "Codex", enabled: false },
    });

    expect(normalized).toEqual({
      all: { label: "Overview", enabled: true },
      claude: { label: "Claude Code", enabled: false },
      codex: { label: "Codex", enabled: false },
    });
    expect(resolveVisibleProvider("codex", normalized)).toBe("all");
  });
});

describe("updateSetting", () => {
  it("updates the store, persists the merged payload, and applies currency changes immediately", async () => {
    const store = makePersistedStore({
      theme: "system",
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

  it("logs and persists the normalized value when an update input is invalid", async () => {
    const store = makePersistedStore({
      currency: "USD",
    });
    mockLoad.mockResolvedValueOnce(store);

    const { loadSettings, settings, updateSetting } = await loadSettingsModule();
    await loadSettings();
    mockSetCurrency.mockClear();

    await updateSetting("currency", "cad" as unknown as Settings["currency"]);

    expect(get(settings).currency).toBe("USD");
    expect(store.set).toHaveBeenCalledWith(
      "settings",
      expect.objectContaining({
        currency: "USD",
      }),
    );
    expect(mockSetCurrency).toHaveBeenCalledWith("USD");
  });

  it("keeps the in-memory update and warns when persistence fails", async () => {
    const store = makePersistedStore({
      theme: "system",
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

  it("normalizes header tab changes so the default provider stays visible", async () => {
    const store = makePersistedStore({
      defaultProvider: "codex",
      headerTabs: DEFAULT_HEADER_TABS,
    });
    mockLoad.mockResolvedValueOnce(store);

    const { loadSettings, settings, updateSetting } = await loadSettingsModule();
    await loadSettings();

    await updateSetting("headerTabs", {
      all: { label: "Overview", enabled: true },
      claude: { label: "Claude Code", enabled: false },
      codex: { label: "Codex", enabled: false },
    });

    expect(get(settings).defaultProvider).toBe("all");
    expect(store.set).toHaveBeenCalledWith(
      "settings",
      expect.objectContaining({
        defaultProvider: "all",
        headerTabs: {
          all: { label: "Overview", enabled: true },
          claude: { label: "Claude Code", enabled: false },
          codex: { label: "Codex", enabled: false },
        },
      }),
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
