import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const repoRoot = process.cwd();

function readRepoFile(path) {
  return readFileSync(join(repoRoot, path), "utf8");
}

describe("native permission surface", () => {
  it("keeps the native permission surface minimal (no fs; dialog limited to Save)", () => {
    const capabilities = JSON.parse(
      readRepoFile("src-tauri/capabilities/default.json"),
    );

    const flatPermissions = capabilities.permissions.map((permission) =>
      typeof permission === "string"
        ? permission
        : JSON.stringify(permission),
    );

    // Filesystem access stays fully closed.
    expect(flatPermissions.some((permission) => permission.startsWith("fs:"))).toBe(false);
    // The ONLY dialog capability is the narrow Save dialog used by usage export
    // (the import flow uses an in-webview <input type=file>, not a dialog).
    const dialogPermissions = flatPermissions.filter((permission) =>
      permission.startsWith("dialog:"),
    );
    expect(dialogPermissions).toEqual(["dialog:allow-save"]);
    expect(flatPermissions).not.toContain("dialog:default");
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
