import { derived, get, writable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { load } from "@tauri-apps/plugin-store";
import {
  createRateLimitsMonitorState,
  eligibleProviders,
  hasUsableRateLimitWindows,
  inferLastSuccessfulAt,
  mergeProviderRateLimits,
  providerDeferredUntil,
  providerPayload,
  RATE_LIMIT_PROVIDER_ORDER,
  RATE_LIMIT_PROVIDER_POLICIES,
  replaceProviderPayload,
  requestedProviders,
  scopeRateLimitRequestState,
  shouldSuppressProviderError,
  type RateLimitProvider,
  type RateLimitScope,
} from "../rateLimitMonitor.js";
import type {
  ProviderRateLimits,
  RateLimitsMonitorState,
  RateLimitsPayload,
} from "../types/index.js";

const CACHE_KEY = "payload";
const LEGACY_CACHE_FILE = "rate-limits.json";
const LAST_SUCCESSFUL_AT_KEY = "lastSuccessfulAt";

interface PersistedProviderRateLimitRecord {
  payload: ProviderRateLimits | null;
  lastSuccessfulAt: string | null;
}

export const rateLimitsData = writable<RateLimitsPayload | null>(null);
export const rateLimitsMonitorState = writable<RateLimitsMonitorState>(createRateLimitsMonitorState());

const requestedScopeStore = writable<RateLimitScope>("all");

export const rateLimitsRequestState = derived(
  [rateLimitsMonitorState, requestedScopeStore],
  ([$monitorState, $scope]) => scopeRateLimitRequestState($monitorState, $scope),
);

let providerStores: Partial<Record<RateLimitProvider, Awaited<ReturnType<typeof load>>>> = {};
let legacyStore: Awaited<ReturnType<typeof load>> | null = null;
let hydratePromise: Promise<void> | null = null;
let inflightFetches: Partial<Record<RateLimitProvider, Promise<void>>> = {};
let retryTimers: Partial<Record<RateLimitProvider, ReturnType<typeof setTimeout>>> = {};
let lastRequestedScope: RateLimitScope = "all";

async function ensureProviderStore(provider: RateLimitProvider) {
  if (providerStores[provider]) return providerStores[provider];
  const store = await load(RATE_LIMIT_PROVIDER_POLICIES[provider].cacheFile, {
    defaults: {},
    autoSave: true,
  });
  providerStores[provider] = store;
  return store;
}

async function ensureLegacyStore() {
  if (legacyStore) return legacyStore;
  legacyStore = await load(LEGACY_CACHE_FILE, { defaults: {}, autoSave: true });
  return legacyStore;
}

async function readPersistedProviderRecord(
  provider: RateLimitProvider,
): Promise<PersistedProviderRateLimitRecord> {
  const store = await ensureProviderStore(provider);
  const payload = (await store.get<ProviderRateLimits | null>(CACHE_KEY)) ?? null;
  const lastSuccessfulAt =
    (await store.get<string | null>(LAST_SUCCESSFUL_AT_KEY)) ?? inferLastSuccessfulAt(payload);
  return { payload, lastSuccessfulAt };
}

async function readLegacyPayload(): Promise<RateLimitsPayload | null> {
  try {
    const store = await ensureLegacyStore();
    return (await store.get<RateLimitsPayload | null>(CACHE_KEY)) ?? null;
  } catch {
    return null;
  }
}

async function persistProviderRecord(
  provider: RateLimitProvider,
  payload: ProviderRateLimits | null,
  lastSuccessfulAt: string | null,
): Promise<void> {
  try {
    const store = await ensureProviderStore(provider);
    await store.set(CACHE_KEY, payload);
    await store.set(LAST_SUCCESSFUL_AT_KEY, lastSuccessfulAt);
    await store.save();
  } catch (error) {
    console.warn(`Failed to persist ${provider} rate limits:`, error);
  }
}

function formatFetchError(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return "Failed to fetch rate limits.";
}

function updateProviderMonitorState(
  provider: RateLimitProvider,
  updater: (current: RateLimitsMonitorState[RateLimitProvider]) => RateLimitsMonitorState[RateLimitProvider],
) {
  rateLimitsMonitorState.update((state) => ({
    ...state,
    [provider]: updater(state[provider]),
  }));
}

function clearRetryTimer(provider: RateLimitProvider) {
  const timer = retryTimers[provider];
  if (!timer) return;
  clearTimeout(timer);
  delete retryTimers[provider];
}

function scheduleProviderRetry(
  payload: RateLimitsPayload | null,
  provider: RateLimitProvider,
) {
  clearRetryTimer(provider);

  const deferredUntil = providerDeferredUntil(providerPayload(payload, provider), provider);
  updateProviderMonitorState(provider, (state) => ({
    ...state,
    deferredUntil,
  }));

  if (!deferredUntil) return;

  const delay = new Date(deferredUntil).getTime() - Date.now();
  if (delay <= 0) return;

  retryTimers[provider] = setTimeout(() => {
    delete retryTimers[provider];
    void fetchRateLimits(provider);
  }, delay + 50);
}

function scheduleScopeRetries(
  payload: RateLimitsPayload | null,
  scope: RateLimitScope,
) {
  const activeProviders = new Set(requestedProviders(scope));
  for (const provider of RATE_LIMIT_PROVIDER_ORDER) {
    if (!activeProviders.has(provider)) {
      clearRetryTimer(provider);
      continue;
    }
    scheduleProviderRetry(payload, provider);
  }
}

function mergedPayloadOrNull(payload: RateLimitsPayload): RateLimitsPayload | null {
  if (!payload.claude && !payload.codex) return null;
  return payload;
}

function hydrateProviderMonitorState(
  provider: RateLimitProvider,
  payload: ProviderRateLimits | null,
  lastSuccessfulAt: string | null,
) {
  updateProviderMonitorState(provider, () => ({
    loading: false,
    loaded: payload !== null || lastSuccessfulAt !== null,
    error: hasUsableRateLimitWindows(payload) ? null : payload?.error ?? null,
    deferredUntil: providerDeferredUntil(payload, provider),
    failureStreak: 0,
    lastAttemptAt: payload?.fetchedAt ?? null,
    lastSuccessAt: lastSuccessfulAt,
  }));
}

async function fetchProviderRateLimits(provider: RateLimitProvider): Promise<void> {
  if (inflightFetches[provider]) return inflightFetches[provider];

  const currentPayload = get(rateLimitsData);
  const cachedProvider = providerPayload(currentPayload, provider);

  updateProviderMonitorState(provider, (state) => ({
    ...state,
    loading: true,
    error: null,
    deferredUntil: providerDeferredUntil(cachedProvider, provider),
  }));

  const startedAt = new Date().toISOString();

  inflightFetches[provider] = (async () => {
    try {
      const freshPayload = await invoke<RateLimitsPayload>("get_rate_limits", {
        provider,
      });

      const freshProvider = providerPayload(freshPayload, provider);
      const payloadBeforeMerge = get(rateLimitsData);
      const cachedBeforeMerge = providerPayload(payloadBeforeMerge, provider);
      const previousMonitor = get(rateLimitsMonitorState)[provider];
      const mergedProvider = mergeProviderRateLimits(freshProvider, cachedBeforeMerge);
      const nextPayload = mergedPayloadOrNull(
        replaceProviderPayload(payloadBeforeMerge, provider, mergedProvider),
      );

      const hadPriorUsableData = hasUsableRateLimitWindows(cachedBeforeMerge);
      const freshError = freshProvider?.error ?? null;
      const lastAttemptAt = freshProvider?.fetchedAt ?? startedAt;
      const failureStreak = freshError ? previousMonitor.failureStreak + 1 : 0;
      const lastSuccessfulAt = freshError
        ? previousMonitor.lastSuccessAt
        : freshProvider?.fetchedAt ?? previousMonitor.lastSuccessAt ?? startedAt;

      rateLimitsData.set(nextPayload);
      updateProviderMonitorState(provider, (state) => ({
        ...state,
        loading: false,
        loaded: true,
        error: shouldSuppressProviderError(hadPriorUsableData, previousMonitor.failureStreak)
          ? null
          : hasUsableRateLimitWindows(mergedProvider)
            ? null
            : freshError,
        deferredUntil: providerDeferredUntil(mergedProvider, provider),
        failureStreak,
        lastAttemptAt,
        lastSuccessAt: lastSuccessfulAt,
      }));

      await persistProviderRecord(provider, mergedProvider, lastSuccessfulAt);
      scheduleProviderRetry(nextPayload, provider);
    } catch (error) {
      console.error(`Failed to fetch ${provider} rate limits:`, error);
      const previousMonitor = get(rateLimitsMonitorState)[provider];
      const currentData = get(rateLimitsData);
      const currentProvider = providerPayload(currentData, provider);
      const hadPriorUsableData = hasUsableRateLimitWindows(currentProvider);
      const formattedError = formatFetchError(error);

      updateProviderMonitorState(provider, (state) => ({
        ...state,
        loading: false,
        loaded: true,
        error: shouldSuppressProviderError(hadPriorUsableData, previousMonitor.failureStreak)
          ? null
          : formattedError,
        deferredUntil: providerDeferredUntil(currentProvider, provider),
        failureStreak: previousMonitor.failureStreak + 1,
        lastAttemptAt: startedAt,
      }));

      scheduleProviderRetry(currentData, provider);
    } finally {
      delete inflightFetches[provider];
    }
  })();

  return inflightFetches[provider];
}

export async function hydrateRateLimits(): Promise<void> {
  if (hydratePromise) return hydratePromise;

  hydratePromise = (async () => {
    try {
      const [legacyPayload, claudeRecord, codexRecord] = await Promise.all([
        readLegacyPayload(),
        readPersistedProviderRecord("claude"),
        readPersistedProviderRecord("codex"),
      ]);

      const payload: RateLimitsPayload = {
        claude: claudeRecord.payload ?? legacyPayload?.claude ?? null,
        codex: codexRecord.payload ?? legacyPayload?.codex ?? null,
      };

      const normalizedPayload = mergedPayloadOrNull(payload);
      if (normalizedPayload) {
        rateLimitsData.set(normalizedPayload);
      }

      const claudeLastSuccess = claudeRecord.lastSuccessfulAt ?? inferLastSuccessfulAt(payload.claude);
      const codexLastSuccess = codexRecord.lastSuccessfulAt ?? inferLastSuccessfulAt(payload.codex);

      hydrateProviderMonitorState("claude", payload.claude, claudeLastSuccess);
      hydrateProviderMonitorState("codex", payload.codex, codexLastSuccess);

      if (legacyPayload) {
        const migrations: Promise<void>[] = [];
        if (!claudeRecord.payload && payload.claude) {
          migrations.push(persistProviderRecord("claude", payload.claude, claudeLastSuccess));
        }
        if (!codexRecord.payload && payload.codex) {
          migrations.push(persistProviderRecord("codex", payload.codex, codexLastSuccess));
        }
        await Promise.all(migrations);
      }

      scheduleScopeRetries(normalizedPayload, lastRequestedScope);
    } catch (error) {
      // Reset so the next call retries instead of returning a stale rejection.
      hydratePromise = null;
      throw error;
    }
  })();

  return hydratePromise;
}

export async function fetchRateLimits(scope: RateLimitScope = "all"): Promise<void> {
  lastRequestedScope = scope;
  requestedScopeStore.set(scope);
  await hydrateRateLimits();

  const cached = get(rateLimitsData);
  scheduleScopeRetries(cached, scope);

  const providers = eligibleProviders(cached, scope);
  if (providers.length === 0) return;

  await Promise.all(providers.map((provider) => fetchProviderRateLimits(provider)));
}
