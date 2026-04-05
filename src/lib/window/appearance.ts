import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { Effect, EffectState, getCurrentWindow } from "@tauri-apps/api/window";
import { isMacOS, isWindows } from "../utils/platform.js";

export const WINDOW_CORNER_RADIUS = 14;
export type ThemePreference = "light" | "dark" | "system";

export type WindowSurface = {
  red: number;
  green: number;
  blue: number;
  alpha: number;
};

function clampChannel(value: number): number {
  return Math.max(0, Math.min(255, Math.round(value)));
}

function parseHexChannel(value: string): number | null {
  const parsed = parseInt(value, 16);
  return Number.isFinite(parsed) ? parsed : null;
}

export function parseCssColor(value: string): WindowSurface | null {
  const color = value.trim();
  if (!color) return null;

  if (color.startsWith("#")) {
    const hex = color.slice(1);
    if (hex.length === 3 || hex.length === 4) {
      const [r, g, b, a = "f"] = hex.split("");
      const red = parseHexChannel(`${r}${r}`);
      const green = parseHexChannel(`${g}${g}`);
      const blue = parseHexChannel(`${b}${b}`);
      const alpha = parseHexChannel(`${a}${a}`);
      if ([red, green, blue, alpha].some((part) => part == null)) {
        return null;
      }
      return {
        red: red!,
        green: green!,
        blue: blue!,
        alpha: alpha!,
      };
    }

    if (hex.length === 6 || hex.length === 8) {
      const red = parseHexChannel(hex.slice(0, 2));
      const green = parseHexChannel(hex.slice(2, 4));
      const blue = parseHexChannel(hex.slice(4, 6));
      const alpha = hex.length === 8 ? parseHexChannel(hex.slice(6, 8)) : 255;
      if ([red, green, blue, alpha].some((part) => part == null)) {
        return null;
      }
      return {
        red: red!,
        green: green!,
        blue: blue!,
        alpha: alpha!,
      };
    }

    return null;
  }

  const rgbMatch = color.match(/^rgba?\((.+)\)$/i);
  if (!rgbMatch) return null;

  const parts = rgbMatch[1].split(",").map((part) => part.trim());
  if (parts.length < 3 || parts.length > 4) return null;

  const red = Number(parts[0]);
  const green = Number(parts[1]);
  const blue = Number(parts[2]);
  const alpha = parts[3] == null ? 255 : Number(parts[3]) * 255;

  if ([red, green, blue, alpha].some((part) => !Number.isFinite(part))) {
    return null;
  }

  return {
    red: clampChannel(red),
    green: clampChannel(green),
    blue: clampChannel(blue),
    alpha: clampChannel(alpha),
  };
}

export function readSurfaceColor(
  root: HTMLElement = document.documentElement,
  getStyles: typeof getComputedStyle = getComputedStyle,
): WindowSurface | null {
  return parseCssColor(getStyles(root).getPropertyValue("--surface"));
}

export async function syncNativeWindowSurface(
  invokeFn: typeof invoke = invoke,
  glassEnabled: boolean = false,
): Promise<void> {
  if (typeof document === "undefined") return;

  const surface = readSurfaceColor();
  if (!surface) return;

  // Always use transparent webview background so CSS border-radius corners
  // don't show the native window background (fixes black corners on Windows).
  const webviewBg = { red: 0, green: 0, blue: 0, alpha: 0 };

  await Promise.all([
    getCurrentWebviewWindow().setBackgroundColor(webviewBg),
    invokeFn("set_window_surface", {
      surface,
      cornerRadius: WINDOW_CORNER_RADIUS,
    }),
  ]);
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
