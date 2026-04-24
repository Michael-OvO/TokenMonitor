import { invoke } from "@tauri-apps/api/core";
import { get } from "svelte/store";
import { settings, updateSetting } from "../stores/settings.js";
import { logger } from "../utils/logger.js";

export type KeychainAccessOutcome =
  | { status: "granted" }
  | { status: "denied"; reason: string }
  | { status: "not_applicable" }
  | { status: "already_requested" };

let keychainRequestInFlight: Promise<KeychainAccessOutcome> | null = null;

function deniedOutcome(
  reason: unknown,
): Extract<KeychainAccessOutcome, { status: "denied" }> {
  return {
    status: "denied",
    reason: reason instanceof Error ? reason.message : String(reason),
  };
}

/**
 * Mark the Keychain permission flow handled before invoking the native prompt.
 * This preserves the "strictly once" contract even if the app is quit while the
 * macOS sheet is open or the user denies access.
 */
export async function requestClaudeKeychainAccessOnce(
  logCategory = "permissions",
): Promise<KeychainAccessOutcome> {
  if (keychainRequestInFlight) return keychainRequestInFlight;

  if (get(settings).keychainAccessRequested) {
    return { status: "already_requested" };
  }

  keychainRequestInFlight = (async () => {
    await updateSetting("keychainAccessRequested", true);

    try {
      const outcome = await invoke<KeychainAccessOutcome>("request_claude_keychain_access");
      if (outcome.status !== "granted" && outcome.status !== "already_requested") {
        logger.info(
          logCategory,
          `Keychain access not granted (${outcome.status})${"reason" in outcome ? ": " + outcome.reason : ""}`,
        );
      }
      return outcome;
    } catch (error) {
      const outcome = deniedOutcome(error);
      logger.error(logCategory, `Keychain access request failed: ${outcome.reason}`);
      return outcome;
    }
  })();

  try {
    return await keychainRequestInFlight;
  } finally {
    keychainRequestInFlight = null;
  }
}

export async function markClaudeKeychainAccessHandled(): Promise<void> {
  if (get(settings).keychainAccessRequested) return;
  await updateSetting("keychainAccessRequested", true);
}
