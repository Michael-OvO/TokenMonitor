/** Detect the current operating system from the user agent string. */
export type Platform = "macos" | "windows" | "linux" | "unknown";

let cached: Platform | null = null;

export function detectPlatform(): Platform {
  if (cached) return cached;

  if (typeof navigator === "undefined") {
    cached = "unknown";
    return cached;
  }

  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes("mac")) cached = "macos";
  else if (ua.includes("win")) cached = "windows";
  else if (ua.includes("linux")) cached = "linux";
  else cached = "unknown";

  return cached;
}

export function isMacOS(): boolean {
  return detectPlatform() === "macos";
}

export function isWindows(): boolean {
  return detectPlatform() === "windows";
}

export function isLinux(): boolean {
  return detectPlatform() === "linux";
}

export function usesFloatingStatusWidget(): boolean {
  const platform = detectPlatform();
  return platform === "windows" || platform === "linux";
}
