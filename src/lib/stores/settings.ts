import { get, writable } from "svelte/store";
import { load } from "@tauri-apps/plugin-store";
import type {
  BarDisplay,
  CostPrecision,
  DefaultPeriod,
  DefaultProvider,
  HeaderTabs,
  PercentageFormat,
  TrayConfig,
  UsageProvider,
} from "../types/index.js";
import { setCurrency } from "../utils/format.js";
import { formatDebugError, logResizeDebug } from "../resizeDebug.js";

export interface Settings {
  theme: "light" | "dark" | "system";
  defaultProvider: UsageProvider;
  defaultPeriod: DefaultPeriod;
  refreshInterval: number; // seconds: 30, 60, 300, 0 = off
  costAlertThreshold: number;
  launchAtLogin: boolean;
  currency: string;
  hiddenModels: string[];
  headerTabs: HeaderTabs;
  brandTheming: boolean;
  trayConfig: TrayConfig;
  claudePlan: number;
  codexPlan: number;
  glassEffect: boolean;
  showModelChangeStats: boolean;
}

export const HEADER_TAB_ORDER: UsageProvider[] = ["all", "claude", "codex"];
export const MAX_HEADER_TAB_LABEL_LENGTH = 18;
export const SUPPORTED_THEMES = ["light", "dark", "system"] as const;
export const SUPPORTED_DEFAULT_PERIODS: DefaultPeriod[] = ["5h", "day", "week", "month"];
export const SUPPORTED_REFRESH_INTERVALS = [30, 60, 300, 0] as const;
export const SUPPORTED_CURRENCIES = ["USD", "EUR", "GBP", "JPY", "CNY"] as const;
export const SUPPORTED_CLAUDE_PLANS = [0, 20, 100, 200] as const;
export const SUPPORTED_CODEX_PLANS = [0, 20, 200] as const;

const SUPPORTED_BAR_DISPLAYS: BarDisplay[] = ["off", "single", "both"];
const SUPPORTED_BAR_PROVIDERS: DefaultProvider[] = ["claude", "codex"];
const SUPPORTED_PERCENTAGE_FORMATS: PercentageFormat[] = ["compact", "verbose"];
const SUPPORTED_COST_PRECISIONS: CostPrecision[] = ["whole", "full"];

export const DEFAULT_HEADER_TABS: HeaderTabs = {
  all: { label: "All", enabled: true },
  claude: { label: "Claude", enabled: true },
  codex: { label: "Codex", enabled: true },
};

function isUsageProvider(value: unknown): value is UsageProvider {
  return value === "all" || value === "claude" || value === "codex";
}

const DEFAULTS: Settings = {
  theme: SUPPORTED_THEMES[2],
  defaultProvider: "claude",
  defaultPeriod: "day",
  refreshInterval: 30,
  costAlertThreshold: 0,
  launchAtLogin: false,
  currency: SUPPORTED_CURRENCIES[0],
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
  claudePlan: 0,
  codexPlan: 0,
  glassEffect: true,
  showModelChangeStats: false,
};

function normalizeBoolean(value: unknown, fallback: boolean): boolean {
  return typeof value === "boolean" ? value : fallback;
}

function normalizeStringChoice<T extends string>(
  value: unknown,
  options: readonly T[],
  fallback: T,
): T {
  return typeof value === "string" && options.includes(value as T) ? (value as T) : fallback;
}

function normalizeNumericChoice<T extends number>(
  value: unknown,
  options: readonly T[],
  fallback: T,
): T {
  const numeric = normalizeFiniteNumber(value);
  return numeric !== null && options.includes(numeric as T) ? (numeric as T) : fallback;
}

function normalizeFiniteNumber(value: unknown): number | null {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value === "string" && value.trim()) {
    const parsed = Number(value);
    if (Number.isFinite(parsed)) return parsed;
  }
  return null;
}

function normalizeCurrency(value: unknown): string {
  if (typeof value !== "string") return DEFAULTS.currency;
  const normalized = value.trim().toUpperCase();
  return normalizeStringChoice(normalized, SUPPORTED_CURRENCIES, DEFAULTS.currency);
}

function normalizeCostAlertThreshold(value: unknown): number {
  const numeric = normalizeFiniteNumber(value);
  if (numeric === null || numeric < 0) return 0;
  return Math.round(numeric * 100) / 100;
}

function normalizeHiddenModels(value: unknown): string[] {
  if (!Array.isArray(value)) return [];

  const seen = new Set<string>();
  for (const entry of value) {
    if (typeof entry !== "string") continue;
    const normalized = entry.trim().toLowerCase();
    if (!normalized) continue;
    seen.add(normalized);
  }

  return Array.from(seen);
}

function normalizeHeaderTabLabel(provider: UsageProvider, label: unknown): string {
  if (typeof label !== "string") return DEFAULT_HEADER_TABS[provider].label;
  const trimmed = label.trim().slice(0, MAX_HEADER_TAB_LABEL_LENGTH);
  return trimmed || DEFAULT_HEADER_TABS[provider].label;
}

function normalizeHeaderTabEnabled(provider: UsageProvider, enabled: unknown): boolean {
  return typeof enabled === "boolean" ? enabled : DEFAULT_HEADER_TABS[provider].enabled;
}

export function normalizeHeaderTabs(headerTabs?: Partial<HeaderTabs> | null): HeaderTabs {
  const normalized: HeaderTabs = {
    all: {
      label: normalizeHeaderTabLabel("all", headerTabs?.all?.label),
      enabled: normalizeHeaderTabEnabled("all", headerTabs?.all?.enabled),
    },
    claude: {
      label: normalizeHeaderTabLabel("claude", headerTabs?.claude?.label),
      enabled: normalizeHeaderTabEnabled("claude", headerTabs?.claude?.enabled),
    },
    codex: {
      label: normalizeHeaderTabLabel("codex", headerTabs?.codex?.label),
      enabled: normalizeHeaderTabEnabled("codex", headerTabs?.codex?.enabled),
    },
  };

  if (!HEADER_TAB_ORDER.some((provider) => normalized[provider].enabled)) {
    normalized.all.enabled = true;
  }

  return normalized;
}

export function getVisibleHeaderProviders(headerTabs: HeaderTabs): UsageProvider[] {
  return HEADER_TAB_ORDER.filter((provider) => headerTabs[provider].enabled);
}

export function areHeaderTabsEqual(a: HeaderTabs, b: HeaderTabs): boolean {
  return HEADER_TAB_ORDER.every((provider) =>
    a[provider].enabled === b[provider].enabled &&
    a[provider].label === b[provider].label,
  );
}

export function resolveVisibleProvider(
  provider: UsageProvider | string | null | undefined,
  headerTabs: HeaderTabs,
): UsageProvider {
  const requested = isUsageProvider(provider) ? provider : DEFAULTS.defaultProvider;
  if (headerTabs[requested].enabled) return requested;
  return getVisibleHeaderProviders(headerTabs)[0] ?? "all";
}

function normalizeTrayConfig(trayConfig?: Partial<TrayConfig> | null): TrayConfig {
  return {
    barDisplay: normalizeStringChoice(
      trayConfig?.barDisplay,
      SUPPORTED_BAR_DISPLAYS,
      DEFAULTS.trayConfig.barDisplay,
    ),
    barProvider: normalizeStringChoice(
      trayConfig?.barProvider,
      SUPPORTED_BAR_PROVIDERS,
      DEFAULTS.trayConfig.barProvider,
    ),
    showPercentages: normalizeBoolean(
      trayConfig?.showPercentages,
      DEFAULTS.trayConfig.showPercentages,
    ),
    percentageFormat: normalizeStringChoice(
      trayConfig?.percentageFormat,
      SUPPORTED_PERCENTAGE_FORMATS,
      DEFAULTS.trayConfig.percentageFormat,
    ),
    showCost: normalizeBoolean(trayConfig?.showCost, DEFAULTS.trayConfig.showCost),
    costPrecision: normalizeStringChoice(
      trayConfig?.costPrecision,
      SUPPORTED_COST_PRECISIONS,
      DEFAULTS.trayConfig.costPrecision,
    ),
  };
}

export function normalizeSettings(saved?: Partial<Settings> | null): Settings {
  const headerTabs = normalizeHeaderTabs(saved?.headerTabs);
  return {
    theme: normalizeStringChoice(saved?.theme, SUPPORTED_THEMES, DEFAULTS.theme),
    defaultProvider: resolveVisibleProvider(saved?.defaultProvider ?? DEFAULTS.defaultProvider, headerTabs),
    defaultPeriod: normalizeStringChoice(
      saved?.defaultPeriod,
      SUPPORTED_DEFAULT_PERIODS,
      DEFAULTS.defaultPeriod,
    ),
    refreshInterval: normalizeNumericChoice(
      saved?.refreshInterval,
      SUPPORTED_REFRESH_INTERVALS,
      DEFAULTS.refreshInterval,
    ),
    costAlertThreshold: normalizeCostAlertThreshold(saved?.costAlertThreshold),
    launchAtLogin: normalizeBoolean(saved?.launchAtLogin, DEFAULTS.launchAtLogin),
    currency: normalizeCurrency(saved?.currency),
    hiddenModels: normalizeHiddenModels(saved?.hiddenModels),
    headerTabs,
    brandTheming: normalizeBoolean(saved?.brandTheming, DEFAULTS.brandTheming),
    trayConfig: normalizeTrayConfig(saved?.trayConfig),
    claudePlan: normalizeNumericChoice(
      saved?.claudePlan,
      SUPPORTED_CLAUDE_PLANS,
      DEFAULTS.claudePlan,
    ),
    codexPlan: normalizeNumericChoice(
      saved?.codexPlan,
      SUPPORTED_CODEX_PLANS,
      DEFAULTS.codexPlan,
    ),
    glassEffect: normalizeBoolean(saved?.glassEffect, DEFAULTS.glassEffect),
    showModelChangeStats: normalizeBoolean(saved?.showModelChangeStats, DEFAULTS.showModelChangeStats),
  };
}

export const settings = writable<Settings>(normalizeSettings());

let storeInstance: Awaited<ReturnType<typeof load>> | null = null;

export async function loadSettings(): Promise<Settings> {
  try {
    const store = await load("settings.json", { defaults: {}, autoSave: true });
    storeInstance = store;

    const saved = await store.get<Partial<Settings>>("settings");
    const migrated =
      saved && "showTrayAmount" in (saved as Record<string, unknown>) && !("trayConfig" in saved)
        ? {
            ...saved,
            trayConfig: {
              ...DEFAULTS.trayConfig,
              barDisplay: "off" as const,
              showCost: (saved as Record<string, unknown>).showTrayAmount !== false,
            },
          }
        : saved;
    const merged = normalizeSettings(migrated);

    settings.set(merged);
    setCurrency(merged.currency);
    logResizeDebug("settings:loaded", {
      theme: merged.theme,
      defaultProvider: merged.defaultProvider,
      defaultPeriod: merged.defaultPeriod,
      refreshInterval: merged.refreshInterval,
      headerTabs: merged.headerTabs,
    });
    return merged;
  } catch (e) {
    const fallback = normalizeSettings();
    storeInstance = null;
    settings.set(fallback);
    setCurrency(fallback.currency);
    logResizeDebug("settings:load-failed", {
      error: formatDebugError(e),
    });
    console.warn("Failed to load settings, using defaults:", e);
    return fallback;
  }
}

async function persistSettings(next: Settings): Promise<void> {
  if (!storeInstance) return;

  try {
    await storeInstance.set("settings", next);
    await storeInstance.save();
  } catch (error) {
    logResizeDebug("settings:persist-failed", {
      error: formatDebugError(error),
    });
    console.warn("Failed to persist settings:", error);
  }
}

export async function updateSetting<K extends keyof Settings>(
  key: K,
  value: Settings[K],
) {
  const updated = normalizeSettings({ ...get(settings), [key]: value });
  const normalizedValue = updated[key];
  settings.set(updated);
  logResizeDebug("settings:update", {
    key,
    value: normalizedValue,
  });

  if (key === "currency") {
    setCurrency(updated.currency);
  }

  await persistSettings(updated);
}

export function applyTheme(theme: Settings["theme"]) {
  const root = document.documentElement;
  if (theme === "system") {
    root.removeAttribute("data-theme");
  } else {
    root.setAttribute("data-theme", theme);
  }
}

export function applyGlass(enabled: boolean) {
  document.documentElement.setAttribute("data-glass", enabled ? "true" : "false");
}

export function applyProvider(provider: UsageProvider, brandTheming: boolean) {
  const root = document.documentElement;
  if (!brandTheming || provider === "all") {
    root.removeAttribute("data-provider");
  } else {
    root.setAttribute("data-provider", provider);
  }
}
