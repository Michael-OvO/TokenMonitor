import { providerHasActiveCooldown } from "./rateLimitsView.js";
import type {
  ProviderRateLimits,
  RateLimitWindow,
  RateLimitProviderMonitorState,
  RateLimitRequestState,
  RateLimitsMonitorState,
  RateLimitsPayload,
  UsageProvider,
} from "./types/index.js";

export type RateLimitProvider = Exclude<UsageProvider, "all">;
export type RateLimitScope = UsageProvider;

interface RateLimitProviderPolicy {
  cacheFile: string;
  minFetchIntervalMs: number;
}

const CLAUDE_MIN_FETCH_INTERVAL_MS = 300_000;

export const RATE_LIMIT_PROVIDER_ORDER: RateLimitProvider[] = ["claude", "codex"];

export const RATE_LIMIT_PROVIDER_POLICIES: Record<RateLimitProvider, RateLimitProviderPolicy> = {
  claude: {
    cacheFile: "rate-limits-claude.json",
    minFetchIntervalMs: CLAUDE_MIN_FETCH_INTERVAL_MS,
  },
  codex: {
    cacheFile: "rate-limits-codex.json",
    minFetchIntervalMs: 0,
  },
};

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
  return {
    claude: null,
    codex: null,
  };
}

export function createRateLimitsMonitorState(): RateLimitsMonitorState {
  return {
    claude: { ...DEFAULT_RATE_LIMIT_PROVIDER_MONITOR_STATE },
    codex: { ...DEFAULT_RATE_LIMIT_PROVIDER_MONITOR_STATE },
  };
}

export function requestedProviders(scope: RateLimitScope): RateLimitProvider[] {
  return scope === "all" ? [...RATE_LIMIT_PROVIDER_ORDER] : [scope];
}

export function providerPayload(
  payload: RateLimitsPayload | null,
  provider: RateLimitProvider,
): ProviderRateLimits | null {
  if (!payload) return null;
  return provider === "claude" ? payload.claude : payload.codex;
}

export function replaceProviderPayload(
  payload: RateLimitsPayload | null,
  provider: RateLimitProvider,
  next: ProviderRateLimits | null,
): RateLimitsPayload {
  const current = payload ?? createRateLimitsPayload();
  return provider === "claude"
    ? { ...current, claude: next }
    : { ...current, codex: next };
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

function stabilizedCodexWindow(
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

function stabilizeCodexRateLimits(
  fresh: ProviderRateLimits,
  cached: ProviderRateLimits,
): ProviderRateLimits {
  if (fresh.provider !== "codex" || cached.provider !== "codex") return fresh;

  return {
    ...fresh,
    windows: fresh.windows.map((window) => stabilizedCodexWindow(window, cached.windows)),
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
    return stabilizeCodexRateLimits(fresh, cached);
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
