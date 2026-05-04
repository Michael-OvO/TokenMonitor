import { describe, expect, it } from "vitest";
import type { Settings } from "../stores/settings.js";
import { getPermissionSurfaces, permissionSurfaceById } from "./surfaces.js";

function settings(overrides: Partial<Settings> = {}): Settings {
  return {
    theme: "system",
    defaultProvider: "claude",
    defaultPeriod: "day",
    refreshInterval: 30,
    costAlertThreshold: 0,
    launchAtLogin: false,
    showDockIcon: false,
    currency: "USD",
    hiddenModels: [],
    headerTabs: {
      all: { label: "All", enabled: true },
      claude: { label: "Claude", enabled: true },
      codex: { label: "Codex", enabled: true },
      cursor: { label: "Cursor", enabled: true },
    },
    brandTheming: true,
    trayConfig: {
      barDisplay: "both",
      barProvider: "claude",
      showPercentages: false,
      percentageFormat: "compact",
      showCost: true,
      costPrecision: "full",
    },
    glassEffect: false,
    showModelChangeStats: false,
    floatBall: false,
    taskbarPanel: false,
    sshHosts: [],
    debugLogging: false,
    cursorApiKey: "",
    keychainAccessRequested: false,
    rateLimitsEnabled: false,
    hasSeenWelcome: false,
    lastOnboardedVersion: null,
    statuslineInstalled: false,
    claudePlanTier: "Pro",
    claudePlanCustomFiveHourTokens: null,
    claudePlanCustomWeeklyTokens: null,
    usageAccessEnabled: true,
    ...overrides,
  };
}

describe("permission surfaces", () => {
  it("documents every surfaced permission with a request policy and copy", () => {
    const surfaces = getPermissionSurfaces(settings(), { macos: true });

    expect(surfaces.length).toBeGreaterThan(0);
    for (const surface of surfaces) {
      expect(surface.title).not.toEqual("");
      expect(surface.why).not.toEqual("");
      expect(surface.requestCopy).not.toEqual("");
      expect(surface.requestPolicy).toMatch(/^[a-z_]+$/);
    }
  });

  it("surfaces the statusline as the explicit-install path for live limits", () => {
    const surfaces = getPermissionSurfaces(settings(), { macos: true });
    const statusline = permissionSurfaceById(surfaces, "claude_statusline");

    expect(statusline).toBeDefined();
    expect(statusline?.requestPolicy).toBe("explicit_install");
    // The user-visible reassurance that no OS prompt is involved is the
    // load-bearing claim of this rewrite — pin it explicitly so a future
    // copy edit can't quietly drop it.
    expect(statusline?.requestCopy).toContain("No Keychain prompt");
  });

  it("marks the statusline as Installed once the user has set it up", () => {
    const surfaces = getPermissionSurfaces(
      settings({ statuslineInstalled: true, rateLimitsEnabled: true }),
      { macos: true },
    );
    const statusline = permissionSurfaceById(surfaces, "claude_statusline");

    expect(statusline?.status).toBe("Installed");
    expect(statusline?.tone).toBe("ok");
  });

  it("documents update checks as banner-only with no notification prompt", () => {
    const surfaces = getPermissionSurfaces(settings(), { macos: true });
    const updates = permissionSurfaceById(surfaces, "updates");

    expect(updates?.status).toBe("Banner only");
    expect(updates?.requestPolicy).toBe("never_requests_os_prompt");
  });

  it("does not include any keychain surface", () => {
    const surfaces = getPermissionSurfaces(settings(), { macos: true });
    expect(surfaces.find((s) => s.id.includes("keychain" as never))).toBeUndefined();
  });
});
