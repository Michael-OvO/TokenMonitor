/**
 * A small show-then-refresh cache for a drill-in view (calendar month,
 * device list, one device). A view paints the cached value for its key at
 * once and refetches behind it; `begin`/`isCurrent` drop a response that a
 * later request has superseded (out-of-order month clicks, quick tab
 * switches). Oldest entries are evicted past `maxEntries`.
 */
export interface ViewCache<T> {
  peek(key: string): T | undefined;
  store(key: string, value: T): void;
  /** Start a request (for `key`); only the latest one is current. */
  begin(key?: string): number;
  isCurrent(request: number): boolean;
  /** True when no later request was started for the same key. */
  isLatestFor(key: string, request: number): boolean;
  clear(): void;
}

export function createViewCache<T>(maxEntries = 24): ViewCache<T> {
  const entries = new Map<string, T>();
  const latestByKey = new Map<string, number>();
  let latest = 0;
  return {
    peek: (key) => entries.get(key),
    store(key, value) {
      entries.delete(key);
      entries.set(key, value);
      while (entries.size > maxEntries) {
        const oldest = entries.keys().next().value as string;
        entries.delete(oldest);
      }
    },
    begin(key) {
      latest += 1;
      if (key !== undefined) latestByKey.set(key, latest);
      return latest;
    },
    isCurrent: (request) => request === latest,
    isLatestFor: (key, request) => (latestByKey.get(key) ?? request) <= request,
    clear: () => entries.clear(),
  };
}

/**
 * Load `key` into a view: paint the cached value at once (no loading state),
 * else show loading; fetch, cache, and apply the result only if no later
 * load started. A failed refetch keeps a cached value on screen; with none,
 * `onError` runs.
 */
export async function loadThroughViewCache<T>(
  cache: ViewCache<T>,
  key: string,
  fetch: () => Promise<T>,
  view: {
    apply: (value: T) => void;
    setLoading: (loading: boolean) => void;
    onError: (error: unknown) => void;
  },
): Promise<void> {
  const request = cache.begin(key);
  const cached = cache.peek(key);
  if (cached !== undefined) {
    view.apply(cached);
    view.setLoading(false);
  } else {
    view.setLoading(true);
  }
  try {
    const fresh = await fetch();
    // A slower, older response for this key must not overwrite a newer one.
    if (cache.isLatestFor(key, request)) cache.store(key, fresh);
    if (cache.isCurrent(request)) view.apply(fresh);
  } catch (error) {
    if (cache.isCurrent(request) && cached === undefined) view.onError(error);
  } finally {
    if (cache.isCurrent(request)) view.setLoading(false);
  }
}
