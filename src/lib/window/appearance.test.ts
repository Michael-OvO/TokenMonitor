import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mockInvoke = vi.fn();
const mockSetBackgroundColor = vi.fn();
const mockGetCurrentWebviewWindow = vi.fn(() => ({
  setBackgroundColor: mockSetBackgroundColor,
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getCurrentWebviewWindow: () => mockGetCurrentWebviewWindow(),
}));

const {
  WINDOW_CORNER_RADIUS,
  parseCssColor,
  readSurfaceColor,
  syncNativeWindowSurface,
} = await import("./appearance.js");

beforeEach(() => {
  mockInvoke.mockReset();
  mockSetBackgroundColor.mockReset();
  mockGetCurrentWebviewWindow.mockClear();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("parseCssColor", () => {
  it("parses short and long hex colors", () => {
    expect(parseCssColor("#141416")).toEqual({
      red: 20,
      green: 20,
      blue: 22,
      alpha: 255,
    });
    expect(parseCssColor("#fff8")).toEqual({
      red: 255,
      green: 255,
      blue: 255,
      alpha: 136,
    });
    expect(parseCssColor("#abc")).toEqual({
      red: 170,
      green: 187,
      blue: 204,
      alpha: 255,
    });
    expect(parseCssColor("#ffffff80")).toEqual({
      red: 255,
      green: 255,
      blue: 255,
      alpha: 128,
    });
  });

  it("parses rgb(a) colors and clamps out-of-range channels", () => {
    expect(parseCssColor("rgba(74, 123, 157, 0.25)")).toEqual({
      red: 74,
      green: 123,
      blue: 157,
      alpha: 64,
    });
    expect(parseCssColor("rgb(300, -10, 10)")).toEqual({
      red: 255,
      green: 0,
      blue: 10,
      alpha: 255,
    });
  });

  it("returns null for malformed or unsupported colors", () => {
    expect(parseCssColor("#ggg")).toBeNull();
    expect(parseCssColor("rgb(10, 20)")).toBeNull();
    expect(parseCssColor("rgba(10, twenty, 30, 1)")).toBeNull();
    expect(parseCssColor("transparent")).toBeNull();
    expect(parseCssColor("")).toBeNull();
  });
});

describe("readSurfaceColor", () => {
  it("reads the --surface CSS variable from the provided root", () => {
    const root = {} as HTMLElement;

    expect(
      readSurfaceColor(root, () => ({
        getPropertyValue: () => " #FFFFFF ",
      }) as unknown as CSSStyleDeclaration),
    ).toEqual({
      red: 255,
      green: 255,
      blue: 255,
      alpha: 255,
    });
  });
});

describe("syncNativeWindowSurface", () => {
  it("returns early when there is no document", async () => {
    await syncNativeWindowSurface(mockInvoke as never);

    expect(mockGetCurrentWebviewWindow).not.toHaveBeenCalled();
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it("returns early when the surface color is unavailable", async () => {
    vi.stubGlobal("document", { documentElement: {} });
    vi.stubGlobal("getComputedStyle", () => ({
      getPropertyValue: () => "transparent",
    }));

    await syncNativeWindowSurface(mockInvoke as never);

    expect(mockGetCurrentWebviewWindow).not.toHaveBeenCalled();
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it("sends the parsed surface color to both the native window and the backend", async () => {
    vi.stubGlobal("document", { documentElement: {} });
    vi.stubGlobal("getComputedStyle", () => ({
      getPropertyValue: () => "rgba(74, 123, 157, 0.25)",
    }));
    mockSetBackgroundColor.mockResolvedValueOnce(undefined);
    mockInvoke.mockResolvedValueOnce(undefined);

    await syncNativeWindowSurface(mockInvoke as never);

    expect(mockGetCurrentWebviewWindow).toHaveBeenCalledTimes(1);
    // Webview bg is always transparent so CSS border-radius corners work on all platforms
    expect(mockSetBackgroundColor).toHaveBeenCalledWith({
      red: 0,
      green: 0,
      blue: 0,
      alpha: 0,
    });
    expect(mockInvoke).toHaveBeenCalledWith("set_window_surface", {
      surface: {
        red: 74,
        green: 123,
        blue: 157,
        alpha: 64,
      },
      cornerRadius: WINDOW_CORNER_RADIUS,
    });
  });

});
