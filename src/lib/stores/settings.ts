import { get, writable } from "svelte/store";
import { load } from "@tauri-apps/plugin-store";
import {
  ALL_USAGE_PROVIDER_ID,
  createDefaultHeaderTabs,
  DEFAULT_RATE_LIMIT_PROVIDER,
  DEFAULT_USAGE_PROVIDER,
  isUsageProvider,
  RATE_LIMIT_PROVIDER_ORDER,
  USAGE_PROVIDER_ORDER,
} from "../providerMetadata.js";
import type {
  BarDisplay,
  CostPrecision,
  DefaultPeriod,
  HeaderTabs,
  PercentageFormat,
  SshHostConfig,
  TrayConfig,
  UsageProvider,
} from "../types/index.js";
import { setCurrency } from "../utils/format.js";
import { logger } from "../utils/logger.js";
import { isMacOS } from "../utils/platform.js";
export interface Settings {
  theme: "light" | "dark" | "system";
  defaultProvider: UsageProvider;
  defaultPeriod: DefaultPeriod;
  refreshInterval: number; // seconds: 30, 60, 300, 0 = off
  costAlertThreshold: number;
  launchAtLogin: boolean;
  showDockIcon: boolean;
  currency: string;
  hiddenModels: string[];
  headerTabs: HeaderTabs;
  brandTheming: boolean;
  trayConfig: TrayConfig;
  glassEffect: boolean;
  showModelChangeStats: boolean;
  floatBall: boolean;
  taskbarPanel: boolean;
  sshHosts: SshHostConfig[];
  debugLogging: boolean;
}

export const HEADER_TAB_ORDER: UsageProvider[] = [...USAGE_PROVIDER_ORDER];
export const MAX_HEADER_TAB_LABEL_LENGTH = 18;
export const SUPPORTED_THEMES = ["light", "dark", "system"] as const;
export const SUPPORTED_DEFAULT_PERIODS: DefaultPeriod[] = ["5h", "day", "week", "month"];
export const SUPPORTED_REFRESH_INTERVALS = [30, 60, 300, 0] as const;
export const SUPPORTED_CURRENCIES = ["USD", "EUR", "GBP", "JPY", "CNY"] as const;

const SUPPORTED_BAR_DISPLAYS: BarDisplay[] = ["off", "single", "both"];
const SUPPORTED_BAR_PROVIDERS = [...RATE_LIMIT_PROVIDER_ORDER];
const SUPPORTED_PERCENTAGE_FORMATS: PercentageFormat[] = ["compact", "verbose"];
const SUPPORTED_COST_PRECISIONS: CostPrecision[] = ["whole", "full"];

export const DEFAULT_HEADER_TABS: HeaderTabs = createDefaultHeaderTabs();

const DEFAULTS: Settings = {
  theme: SUPPORTED_THEMES[2],
  defaultProvider: DEFAULT_USAGE_PROVIDER,
  defaultPeriod: "day",
  refreshInterval: 30,
  costAlertThreshold: 0,
  launchAtLogin: false,
  showDockIcon: false,
  currency: SUPPORTED_CURRENCIES[0],
  hiddenModels: [],
  headerTabs: DEFAULT_HEADER_TABS,
  brandTheming: true,
  trayConfig: {
    barDisplay: 'both',
    barProvider: DEFAULT_RATE_LIMIT_PROVIDER,
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
  const defaultLabel = DEFAULT_HEADER_TABS[provider]?.label ?? provider;
  if (typeof label !== "string") return defaultLabel;
  const trimmed = label.trim().slice(0, MAX_HEADER_TAB_LABEL_LENGTH);
  return trimmed || defaultLabel;
}

function normalizeHeaderTabEnabled(provider: UsageProvider, enabled: unknown): boolean {
  return typeof enabled === "boolean"
    ? enabled
    : (DEFAULT_HEADER_TABS[provider]?.enabled ?? true);
}

export function normalizeHeaderTabs(headerTabs?: Partial<HeaderTabs> | null): HeaderTabs {
  const normalized = createDefaultHeaderTabs();

  for (const provider of HEADER_TAB_ORDER) {
    normalized[provider] = {
      label: normalizeHeaderTabLabel(provider, headerTabs?.[provider]?.label),
      enabled: normalizeHeaderTabEnabled(provider, headerTabs?.[provider]?.enabled),
    };
  }

  if (!HEADER_TAB_ORDER.some((provider) => normalized[provider].enabled)) {
    normalized[HEADER_TAB_ORDER[0] ?? DEFAULT_USAGE_PROVIDER].enabled = true;
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
  return getVisibleHeaderProviders(headerTabs)[0] ?? HEADER_TAB_ORDER[0] ?? DEFAULTS.defaultProvider;
}

function normalizeSshHosts(value: unknown): SshHostConfig[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap((item) => {
    if (typeof item !== "object" || item === null) {
      return [];
    }

    const candidate = item as Record<string, unknown>;
    if (
      typeof candidate.alias !== "string" ||
      candidate.alias.trim() === "" ||
      typeof candidate.enabled !== "boolean"
    ) {
      return [];
    }

    return [{
      alias: candidate.alias.trim(),
      enabled: candidate.enabled,
      include_in_stats:
        typeof candidate.include_in_stats === "boolean" ? candidate.include_in_stats : false,
    }];
  });
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
    showDockIcon: normalizeBoolean(saved?.showDockIcon, DEFAULTS.showDockIcon),
    currency: normalizeCurrency(saved?.currency),
    hiddenModels: normalizeHiddenModels(saved?.hiddenModels),
    headerTabs,
    brandTheming: normalizeBoolean(saved?.brandTheming, DEFAULTS.brandTheming),
    trayConfig: normalizeTrayConfig(saved?.trayConfig),
    glassEffect: normalizeBoolean(saved?.glassEffect, DEFAULTS.glassEffect),
    showModelChangeStats: normalizeBoolean(saved?.showModelChangeStats, DEFAULTS.showModelChangeStats),
    floatBall: normalizeBoolean(saved?.floatBall, DEFAULTS.floatBall),
    taskbarPanel: normalizeBoolean(saved?.taskbarPanel, DEFAULTS.taskbarPanel),
    sshHosts: normalizeSshHosts(saved?.sshHosts),
    debugLogging: normalizeBoolean(saved?.debugLogging, DEFAULTS.debugLogging),
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
    return merged;
  } catch (e) {
    const fallback = normalizeSettings();
    storeInstance = null;
    settings.set(fallback);
    setCurrency(fallback.currency);
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
    console.warn("Failed to persist settings:", error);
  }
}

export async function updateSetting<K extends keyof Settings>(
  key: K,
  value: Settings[K],
) {
  logger.info("settings", `Changed: ${key}=${JSON.stringify(value)}`);
  const updated = normalizeSettings({ ...get(settings), [key]: value });
  settings.set(updated);

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
  // Glass effect requires native NSVisualEffectView blur — macOS only.
  // On Windows/Linux, enabling glass makes the window semi-transparent
  // with no blur backing, so force it off.
  const effective = enabled && isMacOS();
  document.documentElement.setAttribute("data-glass", effective ? "true" : "false");
}

export function applyProvider(provider: UsageProvider, brandTheming: boolean) {
  const root = document.documentElement;
  if (!brandTheming || provider === ALL_USAGE_PROVIDER_ID) {
    root.removeAttribute("data-provider");
  } else {
    root.setAttribute("data-provider", provider);
  }
}
