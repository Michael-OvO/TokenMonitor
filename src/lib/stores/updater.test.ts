import { describe, it, expect, vi, beforeEach } from "vitest";

// Mock @tauri-apps/api/core and /event before importing the store.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

import { invoke } from "@tauri-apps/api/core";
import {
  updaterStore,
  hydrateUpdater,
  checkNow,
  installUpdate,
  skipVersion,
  dismissBanner,
  setAutoCheck,
} from "./updater.js";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  updaterStore.set({
    available: null,
    lastCheck: null,
    lastCheckError: null,
    skippedVersions: [],
    autoCheckEnabled: true,
    dismissedForSession: false,
    progress: null,
    currentVersion: "0.0.0",
    installMode: "auto",
  });
});

describe("updater store", () => {
  it("hydrate fetches status from IPC", async () => {
    mockInvoke.mockResolvedValueOnce({
      state: {
        available: { version: "0.8.0", currentVersion: "0.7.2", notes: null, pubDate: null },
        lastCheck: "2026-04-18T00:00:00Z",
        lastCheckError: null,
        skippedVersions: [],
        lastNotifiedVersion: null,
        autoCheckEnabled: true,
        progress: null,
        dismissedForSession: false,
      },
      currentVersion: "0.7.2",
      installMode: "auto",
    });
    await hydrateUpdater();
    const s = getStoreValue();
    expect(s.available?.version).toBe("0.8.0");
    expect(s.currentVersion).toBe("0.7.2");
    expect(s.installMode).toBe("auto");
  });

  it("checkNow calls updater_check_now", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    await checkNow();
    expect(mockInvoke).toHaveBeenCalledWith("updater_check_now");
  });

  it("installUpdate calls updater_install", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    await installUpdate();
    expect(mockInvoke).toHaveBeenCalledWith("updater_install");
  });

  it("skipVersion passes version argument", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    await skipVersion("0.8.0");
    expect(mockInvoke).toHaveBeenCalledWith("updater_skip_version", { version: "0.8.0" });
  });

  it("dismissBanner calls updater_dismiss", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    await dismissBanner();
    expect(mockInvoke).toHaveBeenCalledWith("updater_dismiss");
  });

  it("setAutoCheck passes enabled flag", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    await setAutoCheck(false);
    expect(mockInvoke).toHaveBeenCalledWith("updater_set_auto_check", { enabled: false });
  });
});

function getStoreValue() {
  let v: any;
  updaterStore.subscribe((val) => (v = val))();
  return v;
}
