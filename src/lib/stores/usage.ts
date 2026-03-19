import { writable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import type {
  UsagePayload,
  UsagePeriod,
  UsageProvider,
} from "../types/index.js";
import { formatDebugError, isResizeDebugEnabled, logResizeDebug } from "../resizeDebug.js";

export const activeProvider = writable<UsageProvider>("claude");
export const activePeriod = writable<UsagePeriod>("day");
export const activeOffset = writable<number>(0);
export const usageData = writable<UsagePayload | null>(null);
export const isLoading = writable(false);

/** Per-provider 5h data, populated when provider="all" and period="5h". */
export const splitFiveHourData = writable<{
  claude: UsagePayload | null;
  codex: UsagePayload | null;
}>({ claude: null, codex: null });

function emptyPayload(): UsagePayload {
  return {
    total_cost: 0,
    total_tokens: 0,
    session_count: 0,
    input_tokens: 0,
    output_tokens: 0,
    chart_buckets: [],
    model_breakdown: [],
    active_block: null,
    five_hour_cost: 0,
    last_updated: new Date().toISOString(),
    from_cache: false,
    period_label: "",
    has_earlier_data: false,
  };
}
export const setupStatus = writable({ ready: false, installing: false, error: null as string | null });

// ── Frontend payload cache ──────────────────────────────────────────
// Eliminates IPC round-trips on tab switches.  Tab clicks resolve from
// a synchronous Map lookup (~0 ms) instead of an async Tauri invoke.
// Fresh data is fetched silently in the background (stale-while-revalidate).

const payloadCache = new Map<string, { data: UsagePayload; at: number }>();
const CACHE_TTL = 300_000; // 5 min — generous; background refresh keeps it current

function cacheKey(provider: string, period: string, offset: number = 0) {
  return `${provider}:${period}:${offset}`;
}

function requestUsagePayload(
  provider: UsageProvider,
  period: UsagePeriod,
  offset: number,
) {
  return invoke<UsagePayload>("get_usage_data", { provider, period, offset });
}

function cachePayload(key: string, data: UsagePayload) {
  payloadCache.set(key, { data, at: Date.now() });
}

function applyUsageDataIfCurrent(requestId: number, data: UsagePayload): boolean {
  const appliedToUi = requestId === currentRequestId;
  if (appliedToUi) {
    usageData.set(data);
  }
  return appliedToUi;
}

// Monotonically increasing request ID prevents stale responses from
// overwriting fresh data when the user rapidly switches tabs.
let currentRequestId = 0;

async function logUsageReadDebug(
  type: string,
  details: Record<string, unknown>,
) {
  if (!isResizeDebugEnabled()) return;

  try {
    const backendReport = await invoke("get_last_usage_debug");
    logResizeDebug(type, {
      ...details,
      backendReport,
    });
  } catch (error) {
    logResizeDebug(type, {
      ...details,
      backendDebugError: formatDebugError(error),
    });
  }
}

/**
 * Fetch data for a provider/period.
 *
 * - If the frontend cache has data, it is shown **immediately** (synchronous)
 *   and a background IPC refresh is kicked off.
 * - If no cached data exists, a blocking IPC fetch is performed with a
 *   loading indicator.
 */
export async function fetchData(
  provider: UsageProvider,
  period: UsagePeriod,
  offset: number = 0,
) {
  const requestId = ++currentRequestId;
  const key = cacheKey(provider, period, offset);
  logResizeDebug("usage:fetch-start", {
    provider,
    period,
    offset,
    requestId,
    cacheKey: key,
    hadFrontendCache: payloadCache.has(key),
  });

  // ── Stale-while-revalidate: instant show + silent refresh ──
  const cached = payloadCache.get(key);
  if (cached && Date.now() - cached.at < CACHE_TTL) {
    usageData.set(cached.data);
    // A warm-cache navigation should never inherit a stale blocking
    // loader from an earlier cold request that is no longer the active view.
    isLoading.set(false);
    logResizeDebug("usage:frontend-cache-hit", {
      provider,
      period,
      offset,
      requestId,
      cacheKey: key,
      cacheAgeMs: Date.now() - cached.at,
    });
    // Silent background refresh — no loading indicator
    requestUsagePayload(provider, period, offset)
      .then((fresh: UsagePayload) => {
        cachePayload(key, fresh);
        const appliedToUi = applyUsageDataIfCurrent(requestId, fresh);
        void logUsageReadDebug("usage:background-refresh-resolved", {
          provider,
          period,
          offset,
          requestId,
          cacheKey: key,
          appliedToUi,
          fromPayloadCache: fresh.from_cache,
        });
      })
      .catch((error) => {
        logResizeDebug("usage:background-refresh-rejected", {
          provider,
          period,
          offset,
          requestId,
          cacheKey: key,
          error: formatDebugError(error),
        });
      });
    return;
  }

  // ── Cold path: no cache — show loading indicator ──
  if (cached) {
    // Expired but exists — show stale data while we fetch
    usageData.set(cached.data);
  } else {
    // No cache at all — clear stale data from a potentially different
    // provider/period so the UI never shows wrong-provider models.
    usageData.set(emptyPayload());
  }
  isLoading.set(true);
  try {
    const data = await requestUsagePayload(provider, period, offset);
    cachePayload(key, data);
    const appliedToUi = applyUsageDataIfCurrent(requestId, data);
    await logUsageReadDebug("usage:fetch-resolved", {
      provider,
      period,
      offset,
      requestId,
      cacheKey: key,
      appliedToUi,
      fromPayloadCache: data.from_cache,
    });
  } catch (e) {
    if (requestId === currentRequestId) {
      console.error("Failed to fetch usage data:", e);
    }
    logResizeDebug("usage:fetch-rejected", {
      provider,
      period,
      offset,
      requestId,
      cacheKey: key,
      error: formatDebugError(e),
    });
  } finally {
    if (requestId === currentRequestId) {
      isLoading.set(false);
    }
  }
}

/**
 * Warm backend + frontend caches for a provider/period.
 * Fire-and-forget: the resolved payload is stored in the frontend cache
 * so subsequent tab switches are synchronous.
 */
export function warmCache(
  provider: UsageProvider,
  period: UsagePeriod,
  offset: number = 0,
) {
  const key = cacheKey(provider, period, offset);
  requestUsagePayload(provider, period, offset)
    .then((data: UsagePayload) => {
      cachePayload(key, data);
      void logUsageReadDebug("usage:warm-cache-resolved", {
        provider,
        period,
        offset,
        cacheKey: key,
        fromPayloadCache: data.from_cache,
      });
    })
    .catch((error) => {
      logResizeDebug("usage:warm-cache-rejected", {
        provider,
        period,
        offset,
        cacheKey: key,
        error: formatDebugError(error),
      });
    });
}

const WARM_PERIODS = ["5h", "day", "week", "month"] as const;

/**
 * Pre-warm lightweight period tabs for a provider.
 * Skips the period already being fetched to avoid redundant work.
 * Intentionally excludes `year` because it is the most expensive aggregation.
 */
export function warmAllPeriods(provider: UsageProvider, skipPeriod?: UsagePeriod) {
  for (const p of WARM_PERIODS) {
    if (p !== skipPeriod) warmCache(provider, p);
  }
}

/**
 * Fetch per-provider 5h data for the split view (all provider, 5h period).
 * Fetches Claude and Codex independently and stores results in splitFiveHourData.
 */
export async function fetchSplitFiveHour() {
  const [claude, codex] = await Promise.all([
    requestUsagePayload("claude", "5h", 0),
    requestUsagePayload("codex", "5h", 0),
  ]);
  cachePayload(cacheKey("claude", "5h", 0), claude);
  cachePayload(cacheKey("codex", "5h", 0), codex);
  splitFiveHourData.set({ claude, codex });
}
