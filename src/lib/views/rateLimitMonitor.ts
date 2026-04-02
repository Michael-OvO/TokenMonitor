import { providerHasActiveCooldown } from "./rateLimits.js";
import {
  getRateLimitCacheFile,
  getRateLimitMinFetchIntervalMs,
  isRateLimitProvider,
  RATE_LIMIT_PROVIDER_ORDER as SUPPORTED_RATE_LIMIT_PROVIDER_ORDER,
  shouldPreservePeakRateLimitUtilization,
} from "../providerMetadata.js";
import type {
  ProviderRateLimits,
  RateLimitProviderId,
  RateLimitWindow,
  RateLimitProviderMonitorState,
  RateLimitRequestState,
  RateLimitsMonitorState,
  RateLimitsPayload,
  UsageProvider,
} from "../types/index.js";

export type RateLimitProvider = RateLimitProviderId;
export type RateLimitScope = UsageProvider;

interface RateLimitProviderPolicy {
  cacheFile: string;
  minFetchIntervalMs: number;
  preservePeakUtilization: boolean;
}

export const RATE_LIMIT_PROVIDER_ORDER: RateLimitProvider[] = [...SUPPORTED_RATE_LIMIT_PROVIDER_ORDER];

export const RATE_LIMIT_PROVIDER_POLICIES: Record<RateLimitProvider, RateLimitProviderPolicy> =
  RATE_LIMIT_PROVIDER_ORDER.reduce((policies, provider) => {
    policies[provider] = {
      cacheFile: getRateLimitCacheFile(provider),
      minFetchIntervalMs: getRateLimitMinFetchIntervalMs(provider),
      preservePeakUtilization: shouldPreservePeakRateLimitUtilization(provider),
    };
    return policies;
  }, {} as Record<RateLimitProvider, RateLimitProviderPolicy>);

export const DEFAULT_RATE_LIMIT_REQUEST_STATE: RateLimitRequestState = {
  loading: false,
  loaded: false,
  error: null,
  deferredUntil: null,
};

export const DEFAULT_RATE_LIMIT_PROVIDER_MONITOR_STATE: RateLimitProviderMonitorState = {
  ...DEFAULT_RATE_LIMIT_REQUEST_STATE,
  failureStreak: 0,
  lastAttemptAt: null,
  lastSuccessAt: null,
};

export function createRateLimitsPayload(): RateLimitsPayload {
  return RATE_LIMIT_PROVIDER_ORDER.reduce((payload, provider) => {
    payload[provider] = null;
    return payload;
  }, {} as RateLimitsPayload);
}

export function createRateLimitsMonitorState(): RateLimitsMonitorState {
  return RATE_LIMIT_PROVIDER_ORDER.reduce((state, provider) => {
    state[provider] = { ...DEFAULT_RATE_LIMIT_PROVIDER_MONITOR_STATE };
    return state;
  }, {} as RateLimitsMonitorState);
}

export function requestedProviders(scope: RateLimitScope): RateLimitProvider[] {
  if (scope === "all") return [...RATE_LIMIT_PROVIDER_ORDER];
  return isRateLimitProvider(scope) ? [scope] : [];
}

export function providerPayload(
  payload: RateLimitsPayload | null,
  provider: RateLimitProvider,
): ProviderRateLimits | null {
  if (!payload) return null;
  return payload[provider];
}

export function replaceProviderPayload(
  payload: RateLimitsPayload | null,
  provider: RateLimitProvider,
  next: ProviderRateLimits | null,
): RateLimitsPayload {
  const current = payload ?? createRateLimitsPayload();
  return {
    ...current,
    [provider]: next,
  };
}

export function hasUsableRateLimitWindows(
  rateLimits: ProviderRateLimits | null | undefined,
): boolean {
  return (rateLimits?.windows.length ?? 0) > 0;
}

export function inferLastSuccessfulAt(
  rateLimits: ProviderRateLimits | null | undefined,
): string | null {
  if (!rateLimits) return null;
  return hasUsableRateLimitWindows(rateLimits) ? rateLimits.fetchedAt : null;
}

function stabilizedPeakWindow(
  freshWindow: RateLimitWindow,
  cachedWindows: RateLimitWindow[],
): RateLimitWindow {
  const cachedWindow = cachedWindows.find(
    (window) => window.windowId === freshWindow.windowId && window.resetsAt === freshWindow.resetsAt,
  );
  if (!cachedWindow || freshWindow.utilization >= cachedWindow.utilization) {
    return freshWindow;
  }

  return {
    ...freshWindow,
    utilization: cachedWindow.utilization,
  };
}

function stabilizeProviderRateLimits(
  fresh: ProviderRateLimits,
  cached: ProviderRateLimits,
): ProviderRateLimits {
  if (!isRateLimitProvider(fresh.provider) || !isRateLimitProvider(cached.provider)) return fresh;
  if (fresh.provider !== cached.provider) return fresh;
  if (!RATE_LIMIT_PROVIDER_POLICIES[fresh.provider].preservePeakUtilization) return fresh;

  return {
    ...fresh,
    windows: fresh.windows.map((window) => stabilizedPeakWindow(window, cached.windows)),
  };
}

export function mergeProviderRateLimits(
  fresh: ProviderRateLimits | null | undefined,
  cached: ProviderRateLimits | null | undefined,
): ProviderRateLimits | null {
  if (fresh && cached && fresh.windows.length === 0 && fresh.error && cached.windows.length > 0) {
    return {
      ...cached,
      stale: true,
      error: fresh.error,
      retryAfterSeconds: fresh.retryAfterSeconds,
      cooldownUntil: fresh.cooldownUntil,
      fetchedAt: fresh.fetchedAt,
    };
  }

  if (fresh && cached) {
    return stabilizeProviderRateLimits(fresh, cached);
  }

  return fresh ?? cached ?? null;
}

function providerThrottleUntil(
  rateLimits: ProviderRateLimits | null,
  provider: RateLimitProvider,
  now = Date.now(),
): string | null {
  const minIntervalMs = RATE_LIMIT_PROVIDER_POLICIES[provider].minFetchIntervalMs;
  if (!rateLimits || minIntervalMs <= 0) return null;

  const fetchedAtMs = new Date(rateLimits.fetchedAt).getTime();
  if (!Number.isFinite(fetchedAtMs)) return null;

  const throttleUntilMs = fetchedAtMs + minIntervalMs;
  if (throttleUntilMs <= now) return null;

  return new Date(throttleUntilMs).toISOString();
}

export function providerDeferredUntil(
  rateLimits: ProviderRateLimits | null,
  provider: RateLimitProvider,
  now = Date.now(),
): string | null {
  const activeCooldownUntil = providerHasActiveCooldown(rateLimits, now)
    ? rateLimits?.cooldownUntil ?? null
    : null;
  const throttleUntil = providerThrottleUntil(rateLimits, provider, now);

  if (activeCooldownUntil && throttleUntil) {
    return new Date(activeCooldownUntil).getTime() >= new Date(throttleUntil).getTime()
      ? activeCooldownUntil
      : throttleUntil;
  }

  return activeCooldownUntil ?? throttleUntil;
}

export function eligibleProviders(
  payload: RateLimitsPayload | null,
  scope: RateLimitScope,
): RateLimitProvider[] {
  return requestedProviders(scope).filter((provider) => {
    return providerDeferredUntil(providerPayload(payload, provider), provider) === null;
  });
}

function earliestDeferredUntil(states: RateLimitProviderMonitorState[]): string | null {
  const deferred = states
    .map((state) => state.deferredUntil)
    .filter((value): value is string => Boolean(value));

  if (deferred.length === 0) return null;
  deferred.sort((left, right) => new Date(left).getTime() - new Date(right).getTime());
  return deferred[0];
}

export function scopeRateLimitRequestState(
  monitorState: RateLimitsMonitorState,
  scope: RateLimitScope,
): RateLimitRequestState {
  const states = requestedProviders(scope).map((provider) => monitorState[provider]);
  const visibleErrors = states
    .map((state) => state.error)
    .filter((value): value is string => Boolean(value));

  return {
    loading: states.some((state) => state.loading),
    loaded: states.some(
      (state) => state.loaded || state.lastAttemptAt !== null || state.lastSuccessAt !== null,
    ),
    error: visibleErrors[0] ?? null,
    deferredUntil: earliestDeferredUntil(states),
  };
}

export function shouldSuppressProviderError(
  hadPriorUsableData: boolean,
  failureStreak: number,
): boolean {
  return hadPriorUsableData && failureStreak === 0;
}
