import { describe, expect, it, vi } from "vitest";
import { createViewCache, loadThroughViewCache } from "./viewCache.js";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function makeView() {
  const state = { value: undefined as string | undefined, loading: false, error: null as unknown };
  return {
    state,
    view: {
      apply: (v: string) => (state.value = v),
      setLoading: (l: boolean) => (state.loading = l),
      onError: (e: unknown) => (state.error = e),
    },
  };
}

describe("loadThroughViewCache", () => {
  it("shows loading on a miss, then the value, and caches it", async () => {
    const cache = createViewCache<string>();
    const { state, view } = makeView();
    const fetch = deferred<string>();

    const load = loadThroughViewCache(cache, "k", () => fetch.promise, view);
    expect(state.loading).toBe(true);
    fetch.resolve("fresh");
    await load;

    expect(state).toEqual({ value: "fresh", loading: false, error: null });
    expect(cache.peek("k")).toBe("fresh");
  });

  it("paints a cached value at once and swaps in the refetch", async () => {
    const cache = createViewCache<string>();
    cache.store("k", "old");
    const { state, view } = makeView();
    const fetch = deferred<string>();

    const load = loadThroughViewCache(cache, "k", () => fetch.promise, view);
    expect(state.value).toBe("old");
    expect(state.loading).toBe(false);
    fetch.resolve("new");
    await load;
    expect(state.value).toBe("new");
  });

  it("drops a response a later load superseded, but still caches it", async () => {
    const cache = createViewCache<string>();
    const { state, view } = makeView();
    const march = deferred<string>();
    const april = deferred<string>();

    const first = loadThroughViewCache(cache, "march", () => march.promise, view);
    const second = loadThroughViewCache(cache, "april", () => april.promise, view);
    april.resolve("april data");
    await second;
    march.resolve("march data");
    await first;

    expect(state.value).toBe("april data");
    expect(state.loading).toBe(false);
    expect(cache.peek("march")).toBe("march data");
  });

  it("keeps a cached value when the refetch fails; reports the error on a miss", async () => {
    const cache = createViewCache<string>();
    cache.store("hit", "kept");
    const hit = makeView();
    await loadThroughViewCache(cache, "hit", () => Promise.reject(new Error("x")), hit.view);
    expect(hit.state).toEqual({ value: "kept", loading: false, error: null });

    const miss = makeView();
    const onError = vi.spyOn(miss.view, "onError");
    await loadThroughViewCache(cache, "miss", () => Promise.reject(new Error("y")), miss.view);
    expect(onError).toHaveBeenCalledOnce();
    expect(miss.state.loading).toBe(false);
  });

  it("evicts the oldest entries past its bound", () => {
    const cache = createViewCache<number>(2);
    cache.store("a", 1);
    cache.store("b", 2);
    cache.store("a", 3); // refreshes a
    cache.store("c", 4);
    expect(cache.peek("b")).toBeUndefined();
    expect(cache.peek("a")).toBe(3);
    expect(cache.peek("c")).toBe(4);
  });
});
