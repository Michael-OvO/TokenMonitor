import { get, writable } from "svelte/store";
import { load } from "@tauri-apps/plugin-store";
import type { DefaultPeriod, DefaultProvider, TrayConfig, UsageProvider } from "../types/index.js";
import { setCurrency } from "../utils/format.js";

export interface Settings {
  theme: "light" | "dark" | "system";
  defaultProvider: DefaultProvider;
  defaultPeriod: DefaultPeriod;
  refreshInterval: number; // seconds: 30, 60, 300, 0 = off
  costAlertThreshold: number;
  launchAtLogin: boolean;
  currency: string;
  hiddenModels: string[];
  brandTheming: boolean;
  trayConfig: TrayConfig;
  claudePlan: number;
  codexPlan: number;
  glassEffect: boolean;
}

const DEFAULTS: Settings = {
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
};

export const settings = writable<Settings>({ ...DEFAULTS });

let storeInstance: Awaited<ReturnType<typeof load>> | null = null;

export async function loadSettings(): Promise<Settings> {
  try {
    const store = await load("settings.json", { defaults: {}, autoSave: true });
    storeInstance = store;

    const saved = await store.get<Partial<Settings>>("settings");
    const merged = { ...DEFAULTS, ...saved };

    // Migrate legacy showTrayAmount → trayConfig
    if (saved && 'showTrayAmount' in saved && !('trayConfig' in saved)) {
      const legacy = saved as Record<string, unknown>;
      merged.trayConfig = {
        ...DEFAULTS.trayConfig,
        barDisplay: 'off',
        showCost: legacy.showTrayAmount !== false,
      };
    }
    delete (merged as Record<string, unknown>).showTrayAmount;

    settings.set(merged);
    setCurrency(merged.currency);
    return merged;
  } catch (e) {
    const fallback = { ...DEFAULTS };
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
  const updated = { ...get(settings), [key]: value };
  settings.set(updated);

  if (key === "currency" && typeof value === "string") {
    setCurrency(value);
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
