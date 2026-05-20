import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

const { mockInvoke, mockLogger } = vi.hoisted(() => ({
  mockInvoke: vi.fn(),
  mockLogger: {
    info: vi.fn(),
    debug: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
  },
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

vi.mock("@tauri-apps/plugin-store", () => ({
  load: vi.fn(),
}));

vi.mock("../utils/format.js", () => ({
  setCurrency: vi.fn(),
}));

vi.mock("../utils/logger.js", () => ({
  logger: mockLogger,
}));

import { normalizeSettings, settings } from "../stores/settings.js";
import {
  markClaudeKeychainAccessHandled,
  requestClaudeKeychainAccessOnce,
} from "./keychain.js";

describe("keychain permission helper", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    mockLogger.info.mockReset();
    mockLogger.error.mockReset();
    settings.set(normalizeSettings());
  });

  it("persists the one-shot flag before invoking the native Keychain prompt", async () => {
    mockInvoke.mockImplementationOnce(async () => {
      expect(get(settings).keychainAccessRequested).toBe(true);
      return { status: "granted" };
    });

    await expect(requestClaudeKeychainAccessOnce("test")).resolves.toEqual({ status: "granted" });

    expect(mockInvoke).toHaveBeenCalledWith("request_claude_keychain_access");
    expect(get(settings).keychainAccessRequested).toBe(true);
  });

  it("does not invoke the native prompt after the one-shot flag is set", async () => {
    await markClaudeKeychainAccessHandled();

    await expect(requestClaudeKeychainAccessOnce("test")).resolves.toEqual({
      status: "already_requested",
    });

    expect(mockInvoke).not.toHaveBeenCalled();
    expect(get(settings).keychainAccessRequested).toBe(true);
  });

  it("coalesces concurrent requests into one native prompt", async () => {
    let resolvePrompt: (value: { status: "granted" }) => void = () => {};
    mockInvoke.mockReturnValueOnce(
      new Promise<{ status: "granted" }>((resolve) => {
        resolvePrompt = resolve;
      }),
    );

    const first = requestClaudeKeychainAccessOnce("test");
    const second = requestClaudeKeychainAccessOnce("test");

    await Promise.resolve();
    await Promise.resolve();

    expect(mockInvoke).toHaveBeenCalledTimes(1);
    resolvePrompt({ status: "granted" });

    await expect(Promise.all([first, second])).resolves.toEqual([
      { status: "granted" },
      { status: "granted" },
    ]);
  });
});
