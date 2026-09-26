import { get, writable } from "svelte/store";

/** Whether the popover is on screen. WebView2 is not told when its window
 * hides and keeps painting, so the page pauses its animations and timers and
 * spaces out its refetches while this is false. Only the popover tracks it:
 * the float ball's page never leaves `true`. */
export const popoverVisible = writable(true);

/** Follow Rust's `popover-visibility` event, sent from every show and hide,
 * seeded from the window itself until the first one arrives (the popover
 * starts hidden). Resolves to the unlisten function. */
export async function trackPopoverVisibility(
  onEvent: (handler: (visible: boolean) => void) => Promise<() => void>,
  isVisible: () => Promise<boolean>,
): Promise<() => void> {
  let told = false;
  const unlisten = await onEvent((visible) => {
    told = true;
    popoverVisible.set(visible);
  });
  const visible = await isVisible().catch(() => true);
  if (!told) popoverVisible.set(visible);
  return unlisten;
}

/** Runs `refresh` at once while the popover shows. While it is hidden, a
 * request runs only `hiddenEveryMs` after the last run, so a show never paints
 * data older than that; the others collapse into one run at the next show. */
export function refreshWhenVisible(
  refresh: () => void,
  hiddenEveryMs = Infinity,
): { request: () => void; stop: () => void } {
  let pending = false;
  let last = Date.now();
  const run = () => {
    pending = false;
    last = Date.now();
    refresh();
  };
  const stop = popoverVisible.subscribe((visible) => {
    if (visible && pending) run();
  });
  return {
    request: () => {
      if (get(popoverVisible) || Date.now() - last >= hiddenEveryMs) run();
      else pending = true;
    },
    stop,
  };
}
