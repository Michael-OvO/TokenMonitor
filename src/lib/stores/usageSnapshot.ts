import { load } from "@tauri-apps/plugin-store";
import { resolveUsageScope } from "../providerMetadata.js";
import { resolveVisibleProvider, type Settings } from "./settings.js";
import type { UsagePayload, UsagePeriod, UsageProvider } from "../types/index.js";
import { logger } from "../utils/logger.js";

/**
 * The last payload the default view displayed, kept on disk so a launch can
 * paint it (under the footer's "cached" label) while the first compute runs.
 * One entry only: the default view (default tab and period, offset 0).
 */
const SNAPSHOT_FILE = "usage-snapshot.json";
const SNAPSHOT_KEY = "defaultView";

type SnapshotSettings = Pick<
  Settings,
  | "defaultProvider"
  | "defaultPeriod"
  | "headerTabs"
  | "currency"
  | "hiddenModels"
  | "weekStart"
  | "rollingPeriods"
  | "remoteDeviceIncludes"
>;

interface StoredSnapshot {
  key: string;
  period: UsagePeriod;
  savedAt: number;
  payload: UsagePayload;
}

let storePromise: ReturnType<typeof load> | null = null;
let lastWritten: string | null = null;

function snapshotStore() {
  if (!storePromise) {
    storePromise = load(SNAPSHOT_FILE, { defaults: {}, autoSave: false });
    storePromise.catch(() => {
      storePromise = null;
    });
  }
  return storePromise;
}

/** Whether `provider/period/offset` is the view a launch opens on. */
export function isDefaultView(
  provider: UsageProvider,
  period: UsagePeriod,
  offset: number,
  s: SnapshotSettings,
): boolean {
  return (
    offset === 0 &&
    period === s.defaultPeriod &&
    provider === resolveVisibleProvider(s.defaultProvider, s.headerTabs)
  );
}

/** Everything that shapes the payload or how it reads: the backend scope
 * (the All tab follows the Header Tabs), the period and its settings, the
 * included remote devices, the currency and the hidden models. A snapshot
 * saved under other settings is never shown. */
export function usageSnapshotKey(
  provider: UsageProvider,
  period: UsagePeriod,
  s: SnapshotSettings,
): string {
  const excludedDevices = s.remoteDeviceIncludes
    .filter((d) => !d.include_in_stats)
    .map((d) => d.alias)
    .sort();
  return JSON.stringify([
    resolveUsageScope(provider, s.headerTabs),
    period,
    s.weekStart,
    s.rollingPeriods,
    s.currency,
    [...s.hiddenModels].sort(),
    excludedDevices,
  ]);
}

/** A snapshot still describes the current period: a 5h window under an hour
 * old, a day or week saved today, a month this month, a year this year. */
export function isSnapshotCurrent(period: UsagePeriod, savedAt: number, now = Date.now()): boolean {
  if (!(savedAt <= now)) return false;
  if (period === "5h") return now - savedAt < 60 * 60_000;
  const saved = new Date(savedAt);
  const today = new Date(now);
  if (saved.getFullYear() !== today.getFullYear()) return false;
  if (period === "year") return true;
  if (saved.getMonth() !== today.getMonth()) return false;
  if (period === "month") return true;
  return saved.getDate() === today.getDate();
}

/** The saved payload for `key`, marked `from_cache`, or null. Never throws. */
export async function readUsageSnapshot(
  key: string,
  now = Date.now(),
): Promise<UsagePayload | null> {
  try {
    const store = await snapshotStore();
    const saved = await store.get<StoredSnapshot>(SNAPSHOT_KEY);
    if (!saved || saved.key !== key || !saved.payload) return null;
    if (!isSnapshotCurrent(saved.period, saved.savedAt, now)) return null;
    return { ...saved.payload, from_cache: true };
  } catch (error) {
    logger.debug("usage", `usage snapshot read failed: ${error}`);
    return null;
  }
}

/** Save the default view's displayed payload. Repeats of the same payload
 * (same key and `last_updated`) are skipped. Never throws. */
export async function writeUsageSnapshot(
  key: string,
  period: UsagePeriod,
  payload: UsagePayload,
  now = Date.now(),
): Promise<void> {
  const stamp = `${key}@${payload.last_updated}`;
  if (stamp === lastWritten) return;
  lastWritten = stamp;
  try {
    const store = await snapshotStore();
    const snapshot: StoredSnapshot = {
      key,
      period,
      savedAt: now,
      payload: { ...payload, from_cache: false },
    };
    await store.set(SNAPSHOT_KEY, snapshot);
    await store.save();
  } catch (error) {
    if (lastWritten === stamp) lastWritten = null;
    logger.debug("usage", `usage snapshot write failed: ${error}`);
  }
}
