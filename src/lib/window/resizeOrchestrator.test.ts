import { afterEach, describe, expect, it, vi } from "vitest";
import { createResizeOrchestrator } from "./resizeOrchestrator.js";
import { WINDOW_WIDTH } from "./sizing.js";

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T | PromiseLike<T>) => void;
  reject: (reason?: unknown) => void;
};

type WindowStub = {
  innerHeight: number;
};

function createDeferred<T>(): Deferred<T> {
  let resolve!: Deferred<T>["resolve"];
  let reject!: Deferred<T>["reject"];
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

async function flushMicrotasks(times = 3): Promise<void> {
  for (let index = 0; index < times; index += 1) {
    await Promise.resolve();
  }
}

function installWindowStub(innerHeight = 320): WindowStub {
  const windowStub: WindowStub = { innerHeight };
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: windowStub,
  });
  return windowStub;
}

function installRafStub() {
  let nextId = 1;
  const frameQueue = new Map<number, FrameRequestCallback>();

  Object.defineProperty(globalThis, "requestAnimationFrame", {
    configurable: true,
    value: (callback: FrameRequestCallback) => {
      const id = nextId++;
      frameQueue.set(id, callback);
      return id;
    },
  });

  Object.defineProperty(globalThis, "cancelAnimationFrame", {
    configurable: true,
    value: (id: number) => {
      frameQueue.delete(id);
    },
  });

  return {
    runNextFrame(now: number) {
      const next = frameQueue.entries().next();
      if (next.done) {
        throw new Error("No queued animation frame to run");
      }
      const [id, callback] = next.value;
      frameQueue.delete(id);
      callback(now);
    },
  };
}

function createTestOrchestrator(options?: {
  invoke?: (cmd: string, args: Record<string, unknown>) => Promise<void>;
  popEl?: HTMLDivElement | null;
  footerEl?: HTMLElement | null;
}) {
  return createResizeOrchestrator({
    getPopEl: () => options?.popEl ?? null,
    getFooterEl: () => options?.footerEl ?? null,
    invoke: options?.invoke ?? (() => Promise.resolve()),
    onScrollLockChange: () => {},
    currentMonitor: async () => null,
    logDebug: () => {},
    formatDebugError: () => ({ message: "test" }),
  });
}

function createPopEl(initialHeight: number) {
  let currentHeight = initialHeight;

  return {
    element: {
      get scrollHeight() {
        return currentHeight;
      },
    } as HTMLDivElement,
    setHeight(nextHeight: number) {
      currentHeight = nextHeight;
    },
  };
}

afterEach(() => {
  vi.restoreAllMocks();
  delete (globalThis as Partial<typeof globalThis> & { window?: Window }).window;
  delete (globalThis as Partial<typeof globalThis> & {
    requestAnimationFrame?: typeof requestAnimationFrame;
  }).requestAnimationFrame;
  delete (globalThis as Partial<typeof globalThis> & {
    cancelAnimationFrame?: typeof cancelAnimationFrame;
  }).cancelAnimationFrame;
});

describe("createResizeOrchestrator", () => {
  it("coalesces overlapping size requests and only applies the latest pending height", async () => {
    installWindowStub(320);
    installRafStub();
    const popEl = createPopEl(420);
    const firstInvoke = createDeferred<void>();
    const secondInvoke = createDeferred<void>();
    const invoke = vi
      .fn<(cmd: string, args: Record<string, unknown>) => Promise<void>>()
      .mockImplementationOnce(() => firstInvoke.promise)
      .mockImplementationOnce(() => secondInvoke.promise);
    const orchestrator = createTestOrchestrator({
      invoke,
      popEl: popEl.element,
    });

    orchestrator.syncSizeAndVerify("first");
    popEl.setHeight(460);
    orchestrator.syncSizeAndVerify("second");
    popEl.setHeight(500);
    orchestrator.syncSizeAndVerify("third");

    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenLastCalledWith("set_window_size_and_align", {
      width: WINDOW_WIDTH,
      height: 420,
    });

    firstInvoke.resolve();
    await flushMicrotasks();

    expect(invoke).toHaveBeenCalledTimes(2);
    expect(invoke).toHaveBeenLastCalledWith("set_window_size_and_align", {
      width: WINDOW_WIDTH,
      height: 500,
    });

    secondInvoke.resolve();
    await flushMicrotasks();
    orchestrator.destroy();
  });

  it("snaps accordion toggles to the measured post-update height", async () => {
    installWindowStub(320);
    installRafStub();
    const popEl = createPopEl(460);
    const invoke = vi.fn(() => Promise.resolve());
    const orchestrator = createTestOrchestrator({
      invoke,
      popEl: popEl.element,
    });

    orchestrator.handleBreakdownAccordionToggle({
      durationMs: 120,
      expanding: true,
      scope: "main",
    });

    await flushMicrotasks();

    expect(invoke).toHaveBeenCalledTimes(1);
    const lastCall = invoke.mock.calls.at(-1);
    expect(lastCall).toBeDefined();
    if (!lastCall) {
      throw new Error("Expected accordion resize to invoke the native window command");
    }
    const [command, args] = lastCall as unknown as [
      string,
      { width: number; height: number },
    ];
    expect(command).toBe("set_window_size_and_align");
    expect(args).toMatchObject({
      width: WINDOW_WIDTH,
    });
    expect(args.height).toBe(460);

    orchestrator.destroy();
  });

  it("can reconcile the native window rect even when height is unchanged", async () => {
    installWindowStub(420);
    installRafStub();
    const popEl = createPopEl(420);
    const invoke = vi.fn(() => Promise.resolve());
    const orchestrator = createTestOrchestrator({
      invoke,
      popEl: popEl.element,
    });

    orchestrator.syncSizeAndVerify("same-height");
    expect(invoke).not.toHaveBeenCalled();

    orchestrator.reconcileWindowGeometry("monitor-change");
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenLastCalledWith("set_window_size_and_align", {
      width: WINDOW_WIDTH,
      height: 420,
    });

    orchestrator.destroy();
  });

  it("defers a shrink under the pointer on a bottom-anchored window, then snaps it in one resize on mouse-leave", async () => {
    installWindowStub(420);
    const { runNextFrame } = installRafStub();
    const popEl = createPopEl(420);
    const invoke = vi.fn(() => Promise.resolve());
    const orchestrator = createTestOrchestrator({
      invoke,
      popEl: popEl.element,
    });

    // Release the cold-launch shrink gate; lastWindowH settles at 420 with no resize.
    orchestrator.markInitialContentReady();
    await flushMicrotasks();
    expect(invoke).not.toHaveBeenCalled();

    // Bottom-anchored (Windows taskbar): a shrink moves the top edge, so it is
    // held back while the pointer is over the popover rather than applied.
    orchestrator.setAnchorEdge("bottom");
    orchestrator.setMouseOverWindow(true);
    popEl.setHeight(300);
    orchestrator.syncSizeAndVerify("content-shrank");
    await flushMicrotasks();
    expect(invoke).not.toHaveBeenCalled();

    // Pointer leaves: the deferred shrink must land as a single native resize,
    // with no animation frame in between. Easing it through per-frame
    // set_window_size_and_align calls made the card visibly fold up in
    // stutter-steps on macOS, and the focus-loss hide cut that off mid-fold.
    orchestrator.setMouseOverWindow(false);
    await flushMicrotasks();

    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenLastCalledWith("set_window_size_and_align", {
      width: WINDOW_WIDTH,
      height: 300,
    });
    expect(() => runNextFrame(16)).toThrow("No queued animation frame to run");

    orchestrator.destroy();
  });

  it("applies a shrink immediately under the pointer on a top-anchored window", async () => {
    installWindowStub(420);
    installRafStub();
    const popEl = createPopEl(420);
    const invoke = vi.fn(() => Promise.resolve());
    const orchestrator = createTestOrchestrator({
      invoke,
      popEl: popEl.element,
    });

    orchestrator.markInitialContentReady();
    await flushMicrotasks();
    expect(invoke).not.toHaveBeenCalled();

    // A tray popover hanging below the macOS menu bar (or Linux top-right)
    // shrinks from the bottom, so nothing moves under the cursor. Switching to
    // a shorter tab must resize right away instead of waiting for mouse-leave.
    orchestrator.setAnchorEdge("top");
    orchestrator.setMouseOverWindow(true);
    popEl.setHeight(300);
    orchestrator.syncSizeAndVerify("tab-switch");
    await flushMicrotasks();

    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenLastCalledWith("set_window_size_and_align", {
      width: WINDOW_WIDTH,
      height: 300,
    });

    orchestrator.destroy();
  });

  it("throttles follow-content updates so transitions do not resize every frame", async () => {
    installWindowStub(320);
    const { runNextFrame } = installRafStub();
    let now = 0;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    const popEl = createPopEl(340);
    const invoke = vi.fn(() => Promise.resolve());
    const orchestrator = createTestOrchestrator({
      invoke,
      popEl: popEl.element,
    });

    orchestrator.followContentDuringTransition(80, "test-transition");

    for (const frameNow of [0, 16, 32, 48, 64]) {
      popEl.setHeight(340 + frameNow);
      now = frameNow;
      runNextFrame(frameNow);
      await flushMicrotasks();
    }

    expect(invoke).toHaveBeenCalledTimes(3);
    orchestrator.destroy();
  });

  it("includes fixed footer chrome in the measured window height", async () => {
    installWindowStub(320);
    installRafStub();
    const popEl = createPopEl(400);
    const footerEl = { offsetHeight: 44 } as HTMLElement;
    const invoke = vi.fn(() => Promise.resolve());
    const orchestrator = createTestOrchestrator({
      invoke,
      popEl: popEl.element,
      footerEl,
    });

    orchestrator.syncSizeAndVerify("with-footer");
    await flushMicrotasks();

    expect(invoke).toHaveBeenCalledWith("set_window_size_and_align", {
      width: WINDOW_WIDTH,
      height: 444,
    });

    orchestrator.destroy();
  });

  it("follows the chart detail panel while it unrolls and snaps to its settled height", async () => {
    const windowStub = installWindowStub(320);
    const { runNextFrame } = installRafStub();
    let now = 0;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    const popEl = createPopEl(320);
    const invoke = vi.fn((_cmd: string, args: Record<string, unknown>) => {
      windowStub.innerHeight = args.height as number;
      return Promise.resolve();
    });
    const orchestrator = createTestOrchestrator({
      invoke,
      popEl: popEl.element,
    });

    orchestrator.markInitialContentReady();
    await flushMicrotasks();
    expect(invoke).not.toHaveBeenCalled();

    // The panel animates its height over 80ms. The window tracks it with at
    // most one native resize per throttle interval — instead of chasing every
    // ResizeObserver notification — and then lands exactly on the final height.
    orchestrator.setChartHoverActive(true, 80);
    const frames: Array<[number, number]> = [
      [0, 320], [16, 344], [32, 362], [48, 374], [64, 380], [80, 384], [112, 384], [144, 384],
    ];
    for (const [frameNow, height] of frames) {
      popEl.setHeight(height);
      now = frameNow;
      runNextFrame(frameNow);
      await flushMicrotasks();
    }

    expect(invoke.mock.calls.length).toBeLessThan(frames.length);
    expect(invoke).toHaveBeenLastCalledWith("set_window_size_and_align", {
      width: WINDOW_WIDTH,
      height: 384,
    });
    expect(() => runNextFrame(160)).toThrow("No queued animation frame to run");

    // While the panel stays open, observer-driven shrinks remain blocked so
    // sweeping across shorter buckets does not jitter the window.
    popEl.setHeight(350);
    orchestrator.resizeToContent("resize-observer");
    runNextFrame(176);
    await flushMicrotasks();
    expect(invoke).toHaveBeenLastCalledWith("set_window_size_and_align", {
      width: WINDOW_WIDTH,
      height: 384,
    });

    orchestrator.destroy();
  });

  it("follows the chart detail panel as it rolls up instead of snapping after it vanishes", async () => {
    const windowStub = installWindowStub(384);
    const { runNextFrame } = installRafStub();
    let now = 0;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    const popEl = createPopEl(384);
    const invoke = vi.fn((_cmd: string, args: Record<string, unknown>) => {
      windowStub.innerHeight = args.height as number;
      return Promise.resolve();
    });
    const orchestrator = createTestOrchestrator({
      invoke,
      popEl: popEl.element,
    });

    orchestrator.markInitialContentReady();
    orchestrator.setAnchorEdge("top");
    await flushMicrotasks();
    expect(invoke).not.toHaveBeenCalled();

    orchestrator.setChartHoverActive(false, 80);
    const frames: Array<[number, number]> = [
      [0, 384], [32, 352], [64, 328], [96, 320], [144, 320],
    ];
    for (const [frameNow, height] of frames) {
      popEl.setHeight(height);
      now = frameNow;
      runNextFrame(frameNow);
      await flushMicrotasks();
    }

    const heights = invoke.mock.calls.map(([, args]) => (args as { height: number }).height);
    expect(heights).toEqual([352, 328, 320]);
    expect(() => runNextFrame(160)).toThrow("No queued animation frame to run");

    orchestrator.destroy();
  });

  it("keeps the single-measure chart hover path when the panel does not animate", async () => {
    installWindowStub(384);
    const { runNextFrame } = installRafStub();
    const popEl = createPopEl(384);
    const invoke = vi.fn(() => Promise.resolve());
    const orchestrator = createTestOrchestrator({
      invoke,
      popEl: popEl.element,
    });

    orchestrator.markInitialContentReady();
    orchestrator.setAnchorEdge("top");
    await flushMicrotasks();

    // Reduced motion: the panel disappears at once, so one measured resize
    // on the next frame is all that is needed.
    popEl.setHeight(320);
    orchestrator.setChartHoverActive(false);
    runNextFrame(16);
    await flushMicrotasks();

    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenLastCalledWith("set_window_size_and_align", {
      width: WINDOW_WIDTH,
      height: 320,
    });
    expect(() => runNextFrame(32)).toThrow("No queued animation frame to run");

    orchestrator.destroy();
  });
});
