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
  RateLimitProviderId,
  HeaderTabs,
  PercentageFormat,
  SshHostConfig,
  TrayConfig,
  UsageProvider,
} from "../types/index.js";
import { CURRENT_ONBOARDING_VERSION, compareSemver } from "../changelog.js";
import { setCurrency } from "../utils/format.js";
import { logger } from "../utils/logger.js";
import { isLinux } from "../utils/platform.js";
export type ClaudePlanTier = "Free" | "Pro" | "Max5x" | "Max20x" | "Custom";

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
  /**
   * Whether to compute live rate-limit data. The fetch is fully local now
   * (statusline event file + JSONL parser), so this is effectively a
   * cosmetic toggle — kept so users can hide the rate-limit row entirely.
   */
  rateLimitsEnabled: boolean;
  /** Optional Cursor Admin/Analytics API key. Stored locally and never logged. */
  cursorApiKey: string;
  /** Legacy: set once the user has handled the Keychain access prompt. */
  keychainAccessRequested: boolean;
  /** Set once the user has seen (and dismissed) the first-launch welcome. */
  hasSeenWelcome: boolean;
  /**
   * Stamp of the last `CURRENT_ONBOARDING_VERSION` value the user accepted.
   * `null` for never-onboarded fresh installs. When the saved value is
   * older than the current build's onboarding version, `loadSettings`
   * forces `hasSeenWelcome=false` so the wizard re-opens with the
   * "What's New" step. The wizard writes the current version back here
   * on finish, so subsequent launches skip the wizard until we bump the
   * constant again.
   */
  lastOnboardedVersion: string | null;
  /**
   * Set to `true` after the user has installed the TokenMonitor statusline
   * into Claude Code's `~/.claude/settings.json`. The onboarding wizard
   * uses this to short-circuit the install card on subsequent launches
   * without re-probing the filesystem.
   */
  statuslineInstalled: boolean;
  /**
   * Plan tier used when computing rolling-window utilization percentages.
   * `Custom` lets the user paste their own per-window budgets when the
   * built-in tier defaults don't match their actual Anthropic limits.
   */
  claudePlanTier: ClaudePlanTier;
  /** Custom 5h budget in tokens. Only honored when `claudePlanTier === "Custom"`. */
  claudePlanCustomFiveHourTokens: number | null;
  /** Custom weekly budget in tokens. Only honored when `claudePlanTier === "Custom"`. */
  claudePlanCustomWeeklyTokens: number | null;
  /**
   * User-controlled toggle for the local session-log parser. When `false`,
   * the backend's `usage_access_enabled` atomic stays off and the parser
   * never reads `~/.claude/` or `~/.codex/`, so no data lands in the
   * dashboard. Defaults to `true` so a brand-new install starts tracking
   * once the wizard finishes; the Settings panel exposes it as the
   * "Session Logs" toggle. Decoupled from `hasSeenWelcome` so the user
   * can disable tracking without resetting the welcome flow.
   */
  usageAccessEnabled: boolean;
}

export const HEADER_TAB_ORDER: UsageProvider[] = [...USAGE_PROVIDER_ORDER];
export const MAX_HEADER_TAB_LABEL_LENGTH = 18;
export const SUPPORTED_THEMES = ["light", "dark", "system"] as const;
export const SUPPORTED_DEFAULT_PERIODS: DefaultPeriod[] = ["5h", "day", "week", "month"];
export const SUPPORTED_REFRESH_INTERVALS = [30, 60, 300, 0] as const;
export const SUPPORTED_CURRENCIES = ["USD", "EUR", "GBP", "JPY", "CNY"] as const;
export const SUPPORTED_CLAUDE_PLAN_TIERS: ClaudePlanTier[] = [
  "Free",
  "Pro",
  "Max5x",
  "Max20x",
  "Custom",
];

const SUPPORTED_BAR_DISPLAYS: BarDisplay[] = ["off", "single", "both", "custom"];
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
    barProviders: [...RATE_LIMIT_PROVIDER_ORDER],
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
  rateLimitsEnabled: false,
  cursorApiKey: "",
  keychainAccessRequested: false,
  hasSeenWelcome: false,
  lastOnboardedVersion: null,
  statuslineInstalled: false,
  claudePlanTier: "Pro",
  claudePlanCustomFiveHourTokens: null,
  claudePlanCustomWeeklyTokens: null,
  usageAccessEnabled: true,
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

function normalizeSecretString(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
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
  const barDisplay = normalizeStringChoice(
    trayConfig?.barDisplay,
    SUPPORTED_BAR_DISPLAYS,
    DEFAULTS.trayConfig.barDisplay,
  );
  const barProvider = normalizeStringChoice(
    trayConfig?.barProvider,
    SUPPORTED_BAR_PROVIDERS,
    DEFAULTS.trayConfig.barProvider,
  );

  let barProviders: RateLimitProviderId[];
  if (Array.isArray(trayConfig?.barProviders)) {
    barProviders = (trayConfig!.barProviders as string[])
      .filter((p): p is RateLimitProviderId => SUPPORTED_BAR_PROVIDERS.includes(p as RateLimitProviderId));
  } else {
    if (barDisplay === 'off') barProviders = [];
    else if (barDisplay === 'single') barProviders = [barProvider];
    else barProviders = [...RATE_LIMIT_PROVIDER_ORDER];
  }

  const derivedDisplay: BarDisplay =
    barProviders.length === 0 ? 'off'
    : barProviders.length === 1 ? 'single'
    : 'custom';

  return {
    barDisplay: derivedDisplay,
    barProvider: barProviders[0] ?? barProvider,
    barProviders,
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
    rateLimitsEnabled: normalizeBoolean(saved?.rateLimitsEnabled, DEFAULTS.rateLimitsEnabled),
    cursorApiKey: normalizeSecretString(saved?.cursorApiKey),
    keychainAccessRequested: normalizeBoolean(saved?.keychainAccessRequested, DEFAULTS.keychainAccessRequested),
    hasSeenWelcome: normalizeBoolean(saved?.hasSeenWelcome, DEFAULTS.hasSeenWelcome),
    lastOnboardedVersion: normalizeOnboardedVersion(saved?.lastOnboardedVersion),
    statuslineInstalled: normalizeBoolean(saved?.statuslineInstalled, DEFAULTS.statuslineInstalled),
    claudePlanTier: normalizeStringChoice(
      saved?.claudePlanTier,
      SUPPORTED_CLAUDE_PLAN_TIERS,
      DEFAULTS.claudePlanTier,
    ),
    claudePlanCustomFiveHourTokens: normalizePositiveNumberOrNull(
      saved?.claudePlanCustomFiveHourTokens,
    ),
    claudePlanCustomWeeklyTokens: normalizePositiveNumberOrNull(
      saved?.claudePlanCustomWeeklyTokens,
    ),
    usageAccessEnabled: normalizeBoolean(saved?.usageAccessEnabled, DEFAULTS.usageAccessEnabled),
  };
}

function normalizePositiveNumberOrNull(value: unknown): number | null {
  const numeric = normalizeFiniteNumber(value);
  if (numeric === null || numeric <= 0) return null;
  return Math.round(numeric);
}

/**
 * Coerce a saved `lastOnboardedVersion` to a clean semver string or `null`.
 * Anything that doesn't look like a `MAJOR.MINOR(.PATCH)?` shape gets
 * dropped — better to re-onboard than to mis-compare a corrupted stamp.
 */
function normalizeOnboardedVersion(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  if (!trimmed) return null;
  return /^\d+\.\d+(\.\d+)?(-[\w.-]+)?$/.test(trimmed) ? trimmed : null;
}

export const settings = writable<Settings>(normalizeSettings());

let storeInstance: Awaited<ReturnType<typeof load>> | null = null;

export async function loadSettings(): Promise<Settings> {
  try {
    const store = await load("settings.json", { defaults: {}, autoSave: true });
    storeInstance = store;

    const saved = await store.get<Partial<Settings>>("settings");
    const legacyTrayConfig =
      saved && "showTrayAmount" in (saved as Record<string, unknown>) && !("trayConfig" in saved)
        ? {
            ...saved,
            trayConfig: {
              ...DEFAULTS.trayConfig,
              barDisplay: "off" as const,
              barProviders: [] as RateLimitProviderId[],
              showCost: (saved as Record<string, unknown>).showTrayAmount !== false,
            },
          }
        : saved;
    // Preserve behavior for existing installs: any saved settings file
    // means the user was already using the app, so silently opt them into
    // the features that now gate on explicit consent.
    const migrated =
      legacyTrayConfig && !("rateLimitsEnabled" in legacyTrayConfig)
        ? {
            ...legacyTrayConfig,
            rateLimitsEnabled: true,
            hasSeenWelcome: true,
          }
        : legacyTrayConfig;
    // The `keychainAccessRequested` field was retired with the OAuth
    // pipeline; if it shows up in an old settings.json we just ignore it.
    const merged = normalizeSettings(migrated);

    // Force re-onboarding when the saved `lastOnboardedVersion` is older
    // than the current build's onboarding version. We *only* flip the
    // welcome flag when the user already had it set (i.e. they were a
    // returning user) — fresh installs already have hasSeenWelcome=false
    // and don't need this branch. Setting hasSeenWelcome=false here is
    // an in-memory mutation; it persists to disk on the next setting
    // change, which is fine because re-running the migration is
    // idempotent and the wizard's handleFinish writes the current
    // version stamp back.
    const stampStale =
      merged.lastOnboardedVersion === null ||
      compareSemver(merged.lastOnboardedVersion, CURRENT_ONBOARDING_VERSION) < 0;
    if (merged.hasSeenWelcome && stampStale) {
      logger.info(
        "settings",
        `Re-onboarding: lastOnboardedVersion=${merged.lastOnboardedVersion ?? "null"} < current=${CURRENT_ONBOARDING_VERSION}`,
      );
      merged.hasSeenWelcome = false;
    }

    // Dev-only override: when `VITE_TM_FORCE_ONBOARDING` is set in the
    // environment (and we're in a dev build), simulate a returning user
    // upgrading from an older version on every launch. The wizard opens
    // with the "What's New" first step and the full permissions flow.
    // Production builds skip this branch entirely thanks to the DEV
    // gate, so the flag can never leak to end users by accident.
    if (import.meta.env.DEV) {
      // Log the flag's value on every load so it's obvious in DevTools
      // whether Vite picked it up. Common gotchas: `.env.local` lives at
      // the project root, must be present *when the dev server starts*,
      // and Vite hot-reloads but doesn't always re-evaluate env files
      // mid-session — kill and restart `tauri dev` after changing it.
      // eslint-disable-next-line no-console
      console.info(
        `[TokenMonitor] VITE_TM_FORCE_ONBOARDING=${JSON.stringify(
          import.meta.env.VITE_TM_FORCE_ONBOARDING,
        )} (DEV=${import.meta.env.DEV})`,
      );
      if (import.meta.env.VITE_TM_FORCE_ONBOARDING) {
        logger.info(
          "settings",
          `VITE_TM_FORCE_ONBOARDING active — re-opening onboarding (current=${CURRENT_ONBOARDING_VERSION})`,
        );
        merged.hasSeenWelcome = false;
        // Pin to a fixed older version so the changelog step renders
        // entries newer than this stamp. "0.0.0" guarantees every
        // existing changelog entry shows up regardless of what the
        // current version is.
        merged.lastOnboardedVersion = "0.0.0";
      }
    }

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

const WINDOW_STATE_KEY = "window_state";
interface WindowState {
  height?: number;
}

export async function getLastWindowHeight(): Promise<number | null> {
  if (!storeInstance) return null;
  try {
    const state = await storeInstance.get<WindowState>(WINDOW_STATE_KEY);
    const h = state?.height;
    return typeof h === "number" && Number.isFinite(h) && h > 0 ? h : null;
  } catch {
    return null;
  }
}

export async function setLastWindowHeight(height: number): Promise<void> {
  if (!storeInstance) return;
  if (!Number.isFinite(height) || height <= 0) return;
  try {
    await storeInstance.set(WINDOW_STATE_KEY, { height: Math.round(height) });
    await storeInstance.save();
  } catch (error) {
    console.warn("Failed to persist window state:", error);
  }
}

export async function updateSetting<K extends keyof Settings>(
  key: K,
  value: Settings[K],
) {
  const safeValue = key.toLowerCase().includes("key") || key.toLowerCase().includes("token")
    ? "[redacted]"
    : JSON.stringify(value);
  logger.info("settings", `Changed: ${key}=${safeValue}`);
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
  // Glass effect uses native window effects: NSVisualEffectView on macOS,
  // Mica/Acrylic on Windows. Linux has no native support — force off.
  const effective = enabled && !isLinux();
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
