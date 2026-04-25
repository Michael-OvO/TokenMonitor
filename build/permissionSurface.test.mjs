import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const repoRoot = process.cwd();

function readRepoFile(path) {
  return readFileSync(join(repoRoot, path), "utf8");
}

describe("native permission surface", () => {
  it("does not expose filesystem or dialog capabilities to the frontend", () => {
    const capabilities = JSON.parse(
      readRepoFile("src-tauri/capabilities/default.json"),
    );

    const flatPermissions = capabilities.permissions.map((permission) =>
      typeof permission === "string"
        ? permission
        : JSON.stringify(permission),
    );

    expect(flatPermissions.some((permission) => permission.startsWith("fs:"))).toBe(false);
    expect(flatPermissions.some((permission) => permission.startsWith("dialog:"))).toBe(false);
    expect(flatPermissions).not.toContain("notification:default");
    expect(flatPermissions).toContain("notification:allow-is-permission-granted");
  });

  it("keeps updater checks on the in-app banner path instead of OS notifications", () => {
    const scheduler = readRepoFile("src-tauri/src/updater/scheduler.rs");

    expect(scheduler).not.toContain("NotificationExt");
    expect(scheduler).not.toContain(".notification()");
    expect(scheduler).toContain("permission-free");
  });

  it("keeps Keychain access fallback-only instead of part of first-run setup", () => {
    const welcome = readRepoFile("src/lib/components/WelcomeCard.svelte");
    const app = readRepoFile("src/App.svelte");
    const enableStart = app.indexOf("async function handleEnableRateLimits()");
    const enableEnd = app.indexOf("async function handleShowKeychainFallback()");
    const enableHandler = app.slice(enableStart, enableEnd);

    expect(welcome).not.toContain("requestClaudeKeychainAccessOnce");
    expect(welcome).not.toContain("markClaudeKeychainAccessHandled");
    expect(welcome).toContain("PermissionDisclosure");
    expect(enableHandler).not.toContain("showKeychainPermissionPanel = true");
    expect(enableHandler).toContain("await enableRateLimits()");
  });
});
