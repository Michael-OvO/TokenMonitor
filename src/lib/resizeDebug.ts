import { get, writable } from "svelte/store";

const MAX_EVENTS = 500;

export type ResizeDebugDetails = Record<string, unknown>;

export type ResizeDebugEvent = {
  id: number;
  at: string;
  elapsedMs: number;
  type: string;
  details: ResizeDebugDetails;
};

export type ResizeDebugSnapshot = {
  reason: string;
  windowInnerHeight: number;
  visualViewportHeight: number | null;
  lastWindowH: number;
  maxWindowH: number;
  popScrollHeight: number | null;
  popClientHeight: number | null;
  popOffsetHeight: number | null;
  bodyScrollHeight: number | null;
  bodyClientHeight: number | null;
  docClientHeight: number | null;
};

type ResizeDebugState = {
  enabled: boolean;
  startedAt: number;
  nextId: number;
  events: ResizeDebugEvent[];
  snapshot: ResizeDebugSnapshot | null;
};

const defaultState = (): ResizeDebugState => ({
  enabled: true,
  startedAt: 0,
  nextId: 1,
  events: [],
  snapshot: null,
});

export const resizeDebugState = writable<ResizeDebugState>(defaultState());

let initialized = false;

function hasWindow(): boolean {
  return typeof window !== "undefined";
}

function nowMs(): number {
  return hasWindow() && typeof performance !== "undefined"
    ? performance.now()
    : Date.now();
}

function sanitizeValue(value: unknown, depth = 0): unknown {
  if (value == null) return value;
  if (typeof value === "number") return Number.isFinite(value) ? Number(value.toFixed(2)) : String(value);
  if (typeof value === "string" || typeof value === "boolean") return value;
  if (depth > 6) return "[depth-limit]";
  if (Array.isArray(value)) return value.slice(0, 20).map((item) => sanitizeValue(item, depth + 1));
  if (typeof value === "object") {
    const entries = Object.entries(value as Record<string, unknown>).slice(0, 40);
    return Object.fromEntries(entries.map(([key, val]) => [key, sanitizeValue(val, depth + 1)]));
  }
  return String(value);
}

function updateState(mutator: (state: ResizeDebugState) => ResizeDebugState) {
  resizeDebugState.update((state) => mutator(state));
}

function exposeGlobalDebugApi() {
  if (!hasWindow()) return;
  const api = {
    enable: () => setResizeDebugEnabled(true),
    disable: () => setResizeDebugEnabled(false),
    toggle: () => toggleResizeDebug(),
    clear: () => clearResizeDebug(),
    dump: () => get(resizeDebugState),
    copy: () => copyResizeDebugToClipboard(),
    log: (type: string, details?: ResizeDebugDetails) => logResizeDebug(type, details),
  };
  (window as Window & { __TM_RESIZE_DEBUG__?: typeof api }).__TM_RESIZE_DEBUG__ = api;
}

export function initResizeDebug() {
  if (!hasWindow() || initialized) return;
  initialized = true;
  const startedAt = nowMs();

  resizeDebugState.set({
    enabled: true,
    startedAt,
    nextId: 1,
    events: [],
    snapshot: null,
  });

  exposeGlobalDebugApi();

  logResizeDebug("debug:init", { mode: "always-on" });
}

export function setResizeDebugEnabled(enabled: boolean) {
  const current = get(resizeDebugState);
  updateState((state) => ({
    ...state,
    enabled,
    startedAt: enabled && !current.enabled ? nowMs() : state.startedAt || nowMs(),
    nextId: enabled && !current.enabled ? 1 : state.nextId,
    events: enabled && !current.enabled ? [] : state.events,
  }));

  if (enabled) {
    logResizeDebug("debug:enabled");
  }
}

export function toggleResizeDebug() {
  setResizeDebugEnabled(!get(resizeDebugState).enabled);
}

export function isResizeDebugEnabled(): boolean {
  return get(resizeDebugState).enabled;
}

export function clearResizeDebug() {
  updateState((state) => ({
    ...state,
    events: [],
    nextId: 1,
    startedAt: nowMs(),
  }));
  logResizeDebug("debug:cleared");
}

export function logResizeDebug(type: string, details: ResizeDebugDetails = {}) {
  const state = get(resizeDebugState);
  if (!state.enabled) return;

  const event: ResizeDebugEvent = {
    id: state.nextId,
    at: new Date().toISOString(),
    elapsedMs: Number((nowMs() - state.startedAt).toFixed(1)),
    type,
    details: sanitizeValue(details) as ResizeDebugDetails,
  };

  updateState((current) => ({
    ...current,
    nextId: current.nextId + 1,
    events: [...current.events, event].slice(-MAX_EVENTS),
  }));
}

export function setResizeDebugSnapshot(snapshot: ResizeDebugSnapshot) {
  if (!get(resizeDebugState).enabled) return;
  updateState((state) => ({ ...state, snapshot }));
}

export function captureResizeDebugSnapshot(
  reason: string,
  popEl: HTMLDivElement | null,
  metrics: {
    lastWindowH: number;
    maxWindowH: number;
  },
): ResizeDebugSnapshot {
  const snapshot: ResizeDebugSnapshot = {
    reason,
    windowInnerHeight: hasWindow() ? window.innerHeight : 0,
    visualViewportHeight: hasWindow() ? window.visualViewport?.height ?? null : null,
    lastWindowH: metrics.lastWindowH,
    maxWindowH: metrics.maxWindowH,
    popScrollHeight: popEl?.scrollHeight ?? null,
    popClientHeight: popEl?.clientHeight ?? null,
    popOffsetHeight: popEl?.offsetHeight ?? null,
    bodyScrollHeight: typeof document !== "undefined" ? document.body?.scrollHeight ?? null : null,
    bodyClientHeight: typeof document !== "undefined" ? document.body?.clientHeight ?? null : null,
    docClientHeight: typeof document !== "undefined" ? document.documentElement?.clientHeight ?? null : null,
  };
  setResizeDebugSnapshot(snapshot);
  return snapshot;
}

export function formatDebugError(error: unknown): Record<string, unknown> {
  if (error instanceof Error) {
    return { name: error.name, message: error.message };
  }
  if (typeof error === "string") {
    return { message: error };
  }
  if (error && typeof error === "object") {
    return JSON.parse(JSON.stringify(error));
  }
  return { message: String(error) };
}

export async function copyResizeDebugToClipboard(): Promise<string> {
  const payload = JSON.stringify(get(resizeDebugState), null, 2);
  if (hasWindow() && navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(payload);
  } else {
    console.log(payload);
  }
  return payload;
}
