import { invoke } from "@tauri-apps/api/core";
import { updateSetting } from "../stores/settings.js";
import { logger } from "../utils/logger.js";

/**
 * Install / check the Claude Code statusline integration.
 *
 * The statusline replaces the previous Keychain + OAuth pipeline. We write
 * a tiny script under `<app_data>/statusline/` and patch
 * `~/.claude/settings.json` to point CC's `statusLine.command` at it.
 * Nothing here triggers an OS permission prompt — it's all plain file IO
 * inside the user's own home directory.
 */

export type InstallOutcome =
  | { status: "installed"; previousCommand: string | null }
  | { status: "already_installed" };

export type InstalledState =
  | { status: "installed" }
  | { status: "not_installed" }
  | { status: "script_missing" };

let installInFlight: Promise<InstallOutcome> | null = null;

/** Install the statusline script + patch settings.json. Idempotent. */
export async function installStatusline(
  logCategory = "statusline",
): Promise<InstallOutcome> {
  if (installInFlight) return installInFlight;

  installInFlight = (async () => {
    try {
      const outcome = await invoke<InstallOutcome>("install_statusline");
      if (outcome.status === "installed") {
        logger.info(
          logCategory,
          `Statusline installed${outcome.previousCommand ? ` (previous: ${outcome.previousCommand})` : ""}`,
        );
      }
      await updateSetting("statuslineInstalled", true);
      return outcome;
    } catch (error) {
      logger.error(logCategory, `Statusline install failed: ${error}`);
      throw error;
    }
  })();

  try {
    return await installInFlight;
  } finally {
    installInFlight = null;
  }
}

/** Probe install state. Cheap and side-effect free. */
export async function checkStatusline(): Promise<InstalledState> {
  return invoke<InstalledState>("check_statusline");
}
