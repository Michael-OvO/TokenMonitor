import { writable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import type { UsagePayload } from "../types/index.js";
import { isResizeDebugEnabled, logResizeDebug } from "../resizeDebug.js";

export const activeProvider = writable<"all" | "claude" | "codex">("claude");
export const activePeriod = writable<"5h" | "day" | "week" | "month" | "year">("day");
export const activeOffset = writable<number>(0);
export const usageData = writable<UsagePayload | null>(null);
export const isLoading = writable(false);

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

// Monotonically increasing request ID prevents stale responses from
// overwriting fresh data when the user rapidly switches tabs.
let currentRequestId = 0;

function formatDebugError(error: unknown) {
  if (error instanceof Error) {
    return {
      name: error.name,
      message: error.message,
    };
  }

  if (typeof error === "string") {
    return { message: error };
  }

  if (error && typeof error === "object") {
    return JSON.parse(JSON.stringify(error));
  }

  return { message: String(error) };
}

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
export async function fetchData(provider: string, period: string, offset: number = 0) {
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
    logResizeDebug("usage:frontend-cache-hit", {
      provider,
      period,
      offset,
      requestId,
      cacheKey: key,
      cacheAgeMs: Date.now() - cached.at,
    });
    // Silent background refresh — no loading indicator
    invoke<UsagePayload>("get_usage_data", { provider, period, offset })
      .then((fresh: UsagePayload) => {
        payloadCache.set(key, { data: fresh, at: Date.now() });
        if (requestId === currentRequestId) {
          usageData.set(fresh);
        }
        void logUsageReadDebug("usage:background-refresh-resolved", {
          provider,
          period,
          offset,
          requestId,
          cacheKey: key,
          appliedToUi: requestId === currentRequestId,
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
    const data = await invoke<UsagePayload>("get_usage_data", {
      provider,
      period,
      offset,
    });
    if (requestId === currentRequestId) {
      payloadCache.set(key, { data, at: Date.now() });
      usageData.set(data);
    }
    await logUsageReadDebug("usage:fetch-resolved", {
      provider,
      period,
      offset,
      requestId,
      cacheKey: key,
      appliedToUi: requestId === currentRequestId,
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
export function warmCache(provider: string, period: string, offset: number = 0) {
  const key = cacheKey(provider, period, offset);
  invoke<UsagePayload>("get_usage_data", { provider, period, offset })
    .then((data: UsagePayload) => {
      payloadCache.set(key, { data, at: Date.now() });
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
export function warmAllPeriods(provider: string, skipPeriod?: string) {
  for (const p of WARM_PERIODS) {
    if (p !== skipPeriod) warmCache(provider, p);
  }
}
