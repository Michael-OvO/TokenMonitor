import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

let mockIsMacOS = true;
let mockIsWindows = false;

const mockSetBackgroundColor = vi.fn();
const mockSetTheme = vi.fn();
const mockGetCurrentWebviewWindow = vi.fn(() => ({
  setBackgroundColor: mockSetBackgroundColor,
}));

vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getCurrentWebviewWindow: () => mockGetCurrentWebviewWindow(),
}));

vi.mock("../utils/platform.js", () => ({
  isMacOS: () => mockIsMacOS,
  isWindows: () => mockIsWindows,
}));

vi.mock("@tauri-apps/api/window", () => ({
  Effect: {
    HudWindow: "hudWindow",
    Mica: "mica",
    Acrylic: "acrylic",
  },
  EffectState: {
    Active: "active",
  },
  getCurrentWindow: () => ({
    setTheme: mockSetTheme,
  }),
}));

const { syncNativeWindowTheme, syncNativeWindowSurface } = await import("./appearance.js");

beforeEach(() => {
  mockIsMacOS = true;
  mockIsWindows = false;
  mockSetBackgroundColor.mockReset();
  mockSetTheme.mockReset();
  mockGetCurrentWebviewWindow.mockClear();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("syncNativeWindowSurface", () => {
  it("returns early when there is no document", async () => {
    await syncNativeWindowSurface();
    expect(mockGetCurrentWebviewWindow).not.toHaveBeenCalled();
  });

  it("keeps the webview background transparent", async () => {
    vi.stubGlobal("document", {});
    mockSetBackgroundColor.mockResolvedValueOnce(undefined);

    await syncNativeWindowSurface();

    expect(mockSetBackgroundColor).toHaveBeenCalledWith({
      red: 0,
      green: 0,
      blue: 0,
      alpha: 0,
    });
  });
});

describe("syncNativeWindowTheme", () => {
  it("maps system theme to the native follow-system mode", async () => {
    await syncNativeWindowTheme("system");
    expect(mockSetTheme).toHaveBeenCalledWith(null);
  });

  it("forwards explicit light and dark themes", async () => {
    await syncNativeWindowTheme("light");
    await syncNativeWindowTheme("dark");

    expect(mockSetTheme).toHaveBeenNthCalledWith(1, "light");
    expect(mockSetTheme).toHaveBeenNthCalledWith(2, "dark");
  });

  it("is a no-op on unsupported platforms", async () => {
    mockIsMacOS = false;
    mockIsWindows = false;

    await syncNativeWindowTheme("dark");

    expect(mockSetTheme).not.toHaveBeenCalled();
  });
});
