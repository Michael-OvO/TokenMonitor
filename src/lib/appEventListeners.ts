/**
 * Pure-DOM event listener wiring for the main App window.
 * No Svelte or Tauri imports — only browser APIs.
 */

export interface AppEventCallbacks {
  onResize?: () => void;
  onFocus?: () => void;
  onBlur?: () => void;
  onError?: (event: ErrorEvent) => void;
  onUnhandledRejection?: (event: PromiseRejectionEvent) => void;
  onVisibilityChange?: () => void;
  onColorSchemeChange?: (matchesLight: boolean) => void;
}

/**
 * Attaches window/document/media-query event listeners and returns a
 * cleanup function that removes all of them.
 */
export function setupAppEventListeners(
  callbacks: AppEventCallbacks,
): () => void {
  const colorScheme = window.matchMedia("(prefers-color-scheme: light)");

  const onResize = () => callbacks.onResize?.();
  const onFocus = () => callbacks.onFocus?.();
  const onBlur = () => callbacks.onBlur?.();
  const onError = (e: ErrorEvent) => callbacks.onError?.(e);
  const onRejection = (e: PromiseRejectionEvent) => callbacks.onUnhandledRejection?.(e);
  const onVisibility = () => callbacks.onVisibilityChange?.();
  const onScheme = () => callbacks.onColorSchemeChange?.(colorScheme.matches);

  window.addEventListener("resize", onResize);
  window.addEventListener("focus", onFocus);
  window.addEventListener("blur", onBlur);
  window.addEventListener("error", onError);
  window.addEventListener("unhandledrejection", onRejection);
  document.addEventListener("visibilitychange", onVisibility);

  if (typeof colorScheme.addEventListener === "function") {
    colorScheme.addEventListener("change", onScheme);
  } else {
    colorScheme.addListener(onScheme);
  }

  return () => {
    window.removeEventListener("resize", onResize);
    window.removeEventListener("focus", onFocus);
    window.removeEventListener("blur", onBlur);
    window.removeEventListener("error", onError);
    window.removeEventListener("unhandledrejection", onRejection);
    document.removeEventListener("visibilitychange", onVisibility);

    if (typeof colorScheme.removeEventListener === "function") {
      colorScheme.removeEventListener("change", onScheme);
    } else {
      colorScheme.removeListener(onScheme);
    }
  };
}
