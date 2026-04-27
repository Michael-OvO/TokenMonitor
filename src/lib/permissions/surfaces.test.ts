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
    rateLimitsEnabled: false,
    hasSeenWelcome: false,
    keychainAccessRequested: false,
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

  it("keeps Keychain as an explicit macOS-only one-shot fallback", () => {
    const macSurfaces = getPermissionSurfaces(settings(), { macos: true });
    const keychain = permissionSurfaceById(macSurfaces, "keychain_fallback");

    expect(keychain?.requestPolicy).toBe("explicit_once");
    expect(keychain?.requestCopy).toContain("never opens the Keychain prompt automatically");

    const nonMacSurfaces = getPermissionSurfaces(settings(), { macos: false });
    expect(permissionSurfaceById(nonMacSurfaces, "keychain_fallback")).toBeUndefined();
  });

  it("describes Claude credentials as the silent primary live-limits path", () => {
    const surfaces = getPermissionSurfaces(settings({ rateLimitsEnabled: true }), {
      macos: true,
    });
    const credentials = permissionSurfaceById(surfaces, "claude_credentials");

    expect(credentials?.status).toBe("Enabled");
    expect(credentials?.requestPolicy).toBe("silent_when_enabled");
    expect(credentials?.requestCopy).toContain("does not show a Keychain prompt");
  });

  it("documents update checks as banner-only with no notification prompt", () => {
    const surfaces = getPermissionSurfaces(settings(), { macos: true });
    const updates = permissionSurfaceById(surfaces, "updates");

    expect(updates?.status).toBe("Banner only");
    expect(updates?.requestPolicy).toBe("never_requests_os_prompt");
  });
});
