import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { Effect, EffectState, getCurrentWindow } from "@tauri-apps/api/window";
import { isMacOS, isWindows } from "../utils/platform.js";

export const WINDOW_CORNER_RADIUS = 14;
export type ThemePreference = "light" | "dark" | "system";

export async function syncNativeWindowSurface(): Promise<void> {
  if (typeof document === "undefined") return;
  await getCurrentWebviewWindow().setBackgroundColor({ red: 0, green: 0, blue: 0, alpha: 0 });
}

export async function syncNativeWindowTheme(theme: ThemePreference): Promise<void> {
  if (!isMacOS() && !isWindows()) return;
  await getCurrentWindow().setTheme(theme === "system" ? null : theme);
}

/**
 * Apply or remove native window visual effects (vibrancy/blur).
 *
 * - macOS: uses NSVisualEffectView with `hudWindow` material.
 *   On macOS Tahoe (26+), the system automatically renders this with
 *   Liquid Glass aesthetics.
 * - Windows 11: uses Mica for a subtle system-matched glass.
 * - Windows 10: uses Acrylic blur.
 * - Linux: noop (Tauri silently ignores unsupported effects).
 */
export async function setNativeGlassEffect(enabled: boolean): Promise<void> {
  if (typeof document === "undefined") return;

  const win = getCurrentWebviewWindow();

  if (!enabled) {
    await win.clearEffects();
    return;
  }

  if (isMacOS()) {
    await win.setEffects({
      effects: [Effect.HudWindow],
      state: EffectState.Active,
      radius: WINDOW_CORNER_RADIUS,
    });
  } else if (isWindows()) {
    // Mica is preferred on Windows 11; Acrylic is the fallback for Win10.
    // Tauri uses the first supported effect from the list.
    await win.setEffects({
      effects: [Effect.Mica, Effect.Acrylic],
    });
  }
  // Linux: no native effects available — noop.
}
