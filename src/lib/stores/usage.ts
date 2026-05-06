import { writable, get } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { DEFAULT_USAGE_PROVIDER } from "../providerMetadata.js";
import type {
  UsagePayload,
  UsagePeriod,
  UsageProvider,
} from "../types/index.js";
import { formatDebugError, isResizeDebugEnabled, logResizeDebug } from "../uiStability.js";
import { logger } from "../utils/logger.js";

export const activeProvider = writable<UsageProvider>(DEFAULT_USAGE_PROVIDER);
export const activePeriod = writable<UsagePeriod>("day");
export const activeOffset = writable<number>(0);
export const chartMode = writable<"bar" | "line" | "pie">("bar");
export const chartSegmentMode = writable<"model" | "device">("model");
export const usageData = writable<UsagePayload | null>(null);
export const isLoading = writable(false);
export const isPlaceholderLoading = writable(false);

function emptyPayload(): UsagePayload {
  return {
    total_cost: 0,
    total_tokens: 0,
    session_count: 0,
    input_tokens: 0,
    output_tokens: 0,
    cache_read_tokens: 0,
    cache_write_5m_tokens: 0,
    cache_write_1h_tokens: 0,
    web_search_requests: 0,
    chart_buckets: [],
    model_breakdown: [],
    active_block: null,
    five_hour_cost: 0,
    last_updated: new Date().toISOString(),
    from_cache: false,
    period_label: "",
    has_earlier_data: false,
    change_stats: null,
    subagent_stats: null,
    usage_source: "parser",
    usage_warning: null,
    device_breakdown: null,
    device_chart_buckets: null,
    provider_detected: null,
  };
}
/**
 * Shallow comparison of the key numeric fields that drive UI rendering.
 * Avoids triggering a Svelte store update (and downstream re-renders /
 * ResizeObserver cycles) when the background refresh returns identical data.
 */
export function shallowPayloadEqual(a: UsagePayload, b: UsagePayload): boolean {
  return (
    a.total_cost === b.total_cost &&
    a.total_tokens === b.total_tokens &&
    a.five_hour_cost === b.five_hour_cost &&
    a.session_count === b.session_count &&
    a.input_tokens === b.input_tokens &&
    a.output_tokens === b.output_tokens &&
    a.chart_buckets.length === b.chart_buckets.length &&
    a.model_breakdown.length === b.model_breakdown.length &&
    a.model_breakdown.every((model, i) =>
      model.model_key === b.model_breakdown[i]?.model_key &&
      model.cost === b.model_breakdown[i]?.cost &&
      model.tokens === b.model_breakdown[i]?.tokens,
    ) &&
    a.period_label === b.period_label &&
    a.has_earlier_data === b.has_earlier_data &&
    a.usage_warning === b.usage_warning &&
    (a.device_breakdown?.length ?? 0) === (b.device_breakdown?.length ?? 0) &&
    (a.device_breakdown ?? []).every(
      (d, i) =>
        d.total_cost === b.device_breakdown?.[i]?.total_cost &&
        d.include_in_stats === b.device_breakdown?.[i]?.include_in_stats,
    ) &&
    (a.device_chart_buckets?.length ?? 0) === (b.device_chart_buckets?.length ?? 0)
  );
}

export const setupStatus = writable({ ready: false, installing: false, error: null as string | null });

// ── Frontend payload cache ──────────────────────────────────────────
// Eliminates IPC round-trips on tab switches.  Tab clicks resolve from
// a synchronous Map lookup (~0 ms) instead of an async Tauri invoke.
// Fresh data is fetched silently in the background (stale-while-revalidate).

const payloadCache = new Map<string, { data: UsagePayload; at: number }>();
const CACHE_TTL = 300_000; // 5 min — generous; background refresh keeps it current

/** Cold-path fetchData only: concurrent calls for the same key share one IPC round-trip. */
const fetchInFlight = new Map<string, Promise<UsagePayload>>();

function cacheKey(provider: string, period: string, offset: number = 0) {
  return `${provider}:${period}:${offset}`;
}

type CacheEntryScope = {
  key: string;
  provider: UsageProvider;
  period: UsagePeriod;
  offset: number;
};

function parseCacheEntryScope(key: string): CacheEntryScope | null {
  const [provider, period, offsetText, ...rest] = key.split(":");
  if (
    rest.length > 0 ||
    !provider ||
    (period !== "5h" && period !== "day" && period !== "week" && period !== "month" && period !== "year")
  ) {
    return null;
  }

  const offset = Number(offsetText);
  if (!Number.isInteger(offset)) {
    return null;
  }

  return {
    key,
    provider,
    period,
    offset,
  };
}

function invalidateMatchingUsageCache(
  matches: (entry: CacheEntryScope) => boolean,
) {
  for (const key of [...payloadCache.keys()]) {
    const entry = parseCacheEntryScope(key);
    if (entry && matches(entry)) {
      payloadCache.delete(key);
    }
  }

  for (const key of [...fetchInFlight.keys()]) {
    const entry = parseCacheEntryScope(key);
    if (entry && matches(entry)) {
      fetchInFlight.delete(key);
    }
  }

  currentCacheEpoch += 1;
  currentRequestId += 1;
  isLoading.set(false);
  isPlaceholderLoading.set(false);
}

function requestUsagePayload(
  provider: UsageProvider,
  period: UsagePeriod,
  offset: number,
) {
  return invoke<UsagePayload>("get_usage_data", { provider, period, offset });
}

function cachePayload(key: string, data: UsagePayload, epoch: number = currentCacheEpoch) {
  if (epoch !== currentCacheEpoch) return false;
  payloadCache.set(key, { data, at: Date.now() });
  return true;
}

function applyUsageDataIfCurrent(requestId: number, data: UsagePayload): boolean {
  const appliedToUi = requestId === currentRequestId;
  if (appliedToUi) {
    const current = get(usageData);
    // Skip .set() when the new payload is structurally identical to what's
    // already displayed — prevents unnecessary re-renders and resize cycles.
    if (current === null || !shallowPayloadEqual(current, data)) {
      usageData.set(data);
    }
    isPlaceholderLoading.set(false);
  }
  return appliedToUi;
}

// Monotonically increasing request ID prevents stale responses from
// overwriting fresh data when the user rapidly switches tabs.
let currentRequestId = 0;
let currentCacheEpoch = 0;

export function clearUsageCache() {
  logger.info("usage", "Cache cleared");
  invalidateMatchingUsageCache(() => true);
}

export function clearUsageCacheForProviders(providers: Iterable<UsageProvider>) {
  const affectedProviders = new Set(providers);
  logger.info("usage", `Cache cleared for providers: ${[...affectedProviders].join(", ")}`);
  invalidateMatchingUsageCache(({ provider }) => affectedProviders.has(provider));
}

export function seedUsageCache(
  provider: UsageProvider,
  period: UsagePeriod,
  offset: number,
  data: UsagePayload,
) {
  cachePayload(cacheKey(provider, period, offset), data);
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

/** Shared context for debug logging within a single fetch call. */
interface FetchCtx {
  provider: string;
  period: string;
  offset: number;
  requestId: number;
  cacheKey: string;
}

function logPayloadWarning(
  ctx: Omit<FetchCtx, "requestId"> | FetchCtx,
  data: UsagePayload,
  source: string,
) {
  if (!data.usage_warning) return;
  logger.warn(
    "usage",
    `Backend warning (${source}): provider=${ctx.provider} period=${ctx.period} offset=${ctx.offset} warning=${data.usage_warning}`,
  );
}

function logBackgroundRefreshResult(
  ctx: FetchCtx,
  prev: UsagePayload,
  fresh: UsagePayload,
  appliedToUi: boolean,
) {
  if (isResizeDebugEnabled() && appliedToUi) {
    const costDelta = fresh.total_cost - prev.total_cost;
    const tokenDelta = Number(fresh.total_tokens) - Number(prev.total_tokens);
    if (Math.abs(costDelta) > 0.0001 || tokenDelta !== 0) {
      logResizeDebug("usage:background-refresh-delta", { ...ctx, costDelta, tokenDelta });
    }
  }
  void logUsageReadDebug("usage:background-refresh-resolved", {
    ...ctx, appliedToUi, fromPayloadCache: fresh.from_cache,
  });
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
  const cacheEpoch = currentCacheEpoch;
  const key = cacheKey(provider, period, offset);
  const ctx: FetchCtx = { provider, period, offset, requestId, cacheKey: key };
  logger.debug("usage", `Fetch: ${provider}/${period} offset=${offset}`);
  logResizeDebug("usage:fetch-start", { ...ctx, hadFrontendCache: payloadCache.has(key) });

  // ── Stale-while-revalidate: instant show + silent refresh ──
  const cached = payloadCache.get(key);
  if (cached && Date.now() - cached.at < CACHE_TTL) {
    if (get(usageData) !== cached.data) {
      usageData.set(cached.data);
    }
    isLoading.set(false);
    isPlaceholderLoading.set(false);
    logger.debug("usage", `Cache hit: ${key}`);
    logResizeDebug("usage:frontend-cache-hit", { ...ctx, cacheAgeMs: Date.now() - cached.at });
    // Silent background refresh — no loading indicator
    requestUsagePayload(provider, period, offset)
      .then((fresh: UsagePayload) => {
        logPayloadWarning(ctx, fresh, "background-refresh");
        cachePayload(key, fresh, cacheEpoch);
        const appliedToUi = applyUsageDataIfCurrent(requestId, fresh);
        logBackgroundRefreshResult(ctx, cached.data, fresh, appliedToUi);
      })
      .catch((error) => {
        logResizeDebug("usage:background-refresh-rejected", { ...ctx, error: formatDebugError(error) });
      });
    return;
  }

  // ── Cold path: no cache — show loading indicator ──
  if (cached) {
    usageData.set(cached.data);
    isPlaceholderLoading.set(false);
  } else {
    usageData.set(emptyPayload());
    isPlaceholderLoading.set(true);
  }
  isLoading.set(true);
  try {
    let pending = fetchInFlight.get(key);
    if (!pending) {
      pending = requestUsagePayload(provider, period, offset).finally(() => {
        fetchInFlight.delete(key);
      });
      fetchInFlight.set(key, pending);
    }
    const data = await pending;
    logPayloadWarning(ctx, data, "fetch");
    cachePayload(key, data, cacheEpoch);
    const appliedToUi = applyUsageDataIfCurrent(requestId, data);
    await logUsageReadDebug("usage:fetch-resolved", {
      ...ctx, appliedToUi, fromPayloadCache: data.from_cache,
    });
  } catch (e) {
    logger.error("usage", `Fetch failed: provider=${provider} period=${period} offset=${offset} error=${formatDebugError(e)}`);
    logResizeDebug("usage:fetch-rejected", { ...ctx, error: formatDebugError(e) });
  } finally {
    if (requestId === currentRequestId) {
      isLoading.set(false);
      isPlaceholderLoading.set(false);
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
  const cacheEpoch = currentCacheEpoch;
  requestUsagePayload(provider, period, offset)
    .then((data: UsagePayload) => {
      logPayloadWarning({ provider, period, offset, cacheKey: key }, data, "warm-cache");
      cachePayload(key, data, cacheEpoch);
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
