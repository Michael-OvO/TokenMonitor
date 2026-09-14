import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const repoRoot = process.cwd();

function readRepoFile(path) {
  return readFileSync(join(repoRoot, path), "utf8");
}

describe("native permission surface", () => {
  it("keeps the native permission surface minimal (no fs; dialog limited to Save + folder Open)", () => {
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
    // The only dialog capabilities are the narrow Save dialog used by usage
    // export and the directory-only Open dialog used to pick the auto-export
    // destination folder (the import flow uses an in-webview <input type=file>,
    // not a dialog). No broad `dialog:default`.
    const dialogPermissions = flatPermissions.filter((permission) =>
      permission.startsWith("dialog:"),
    );
    expect(dialogPermissions).toEqual(["dialog:allow-save", "dialog:allow-open"]);
    expect(flatPermissions).not.toContain("dialog:default");
    expect(flatPermissions.some((permission) => permission.startsWith("notification:"))).toBe(false);
  });

  it("keeps updater checks on the in-app banner path instead of OS notifications", () => {
    const scheduler = readRepoFile("src-tauri/src/updater/scheduler.rs");

    expect(scheduler).not.toContain("NotificationExt");
    expect(scheduler).not.toContain(".notification()");
    expect(scheduler).toContain("permission-free");
  });

  it("keeps first-run setup on PermissionsOnboarding without Keychain prompts", () => {
    const onboarding = readRepoFile("src/lib/components/PermissionsOnboarding.svelte");
    const app = readRepoFile("src/App.svelte");
    const enableStart = app.indexOf("async function handleEnableRateLimits()");
    const enableEnd = app.indexOf("async function handleInstallStatusline()");
    const enableHandler = app.slice(enableStart, enableEnd);

    expect(onboarding).not.toContain("requestClaudeKeychainAccessOnce");
    expect(onboarding).not.toContain("markClaudeKeychainAccessHandled");
    expect(onboarding).toContain("installStatusline");
    expect(onboarding).toContain("checkStatusline");
    expect(enableHandler).not.toContain("showKeychainPermissionPanel = true");
    expect(enableHandler).toContain("await enableRateLimits()");
  });
});
