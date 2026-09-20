/** Animate a disclosure's measured height without recalculating grid tracks
 * on every frame. Content stays mounted and returns to natural height when
 * open, so inputs and asynchronous content keep their state. */

export interface DisclosureMotionOptions {
  open: boolean;
  /** Expand duration in milliseconds. */
  openMs?: number;
  /** Collapse duration in milliseconds. */
  closeMs?: number;
  /** Called once the node rests at its target height: when the animation
   * finishes, or at once when no animation runs (reduced motion, no height
   * change). Not called for motion cut short by a reversal or by destroying
   * the action — the state that interrupted it reports its own settle. */
  onSettled?: (open: boolean) => void;
}

export type DisclosureMotionParam = boolean | DisclosureMotionOptions;

const DEFAULT_OPEN_MS = 180;
const DEFAULT_CLOSE_MS = 140;
const EASING = "cubic-bezier(0.2, 0, 0, 1)";

function normalizeParam(param: DisclosureMotionParam) {
  const options = typeof param === "boolean" ? { open: param } : param;
  return {
    open: options.open,
    openMs: options.openMs ?? DEFAULT_OPEN_MS,
    closeMs: options.closeMs ?? DEFAULT_CLOSE_MS,
    onSettled: options.onSettled,
  };
}

export function disclosureMotion(node: HTMLElement, param: DisclosureMotionParam) {
  const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
  let options = normalizeParam(param);
  let open = options.open;
  let animation: Animation | null = null;

  function applyHeight() {
    node.style.height = open ? "auto" : "0px";
  }

  function stopAnimation() {
    if (!animation) return;
    // Detach the finish handler first: cancelling must never report a settle.
    animation.onfinish = null;
    animation.cancel();
    animation = null;
  }

  function settle(settledOpen: boolean) {
    options.onSettled?.(settledOpen);
  }

  function onMotionPreferenceChange() {
    if (!reducedMotion.matches || !animation) return;
    // applyHeight already put the node at its target height; dropping the
    // overlay animation reveals it, which is the settled state.
    stopAnimation();
    settle(open);
  }

  applyHeight();
  reducedMotion.addEventListener("change", onMotionPreferenceChange);

  return {
    update(nextParam: DisclosureMotionParam) {
      options = normalizeParam(nextParam);
      if (options.open === open) return;
      open = options.open;

      // Read the in-flight position before cancelling so quick reversals
      // continue from the visible height instead of snapping to an endpoint.
      const from = node.getBoundingClientRect().height;
      const to = open ? (node.firstElementChild?.getBoundingClientRect().height ?? 0) : 0;
      stopAnimation();
      applyHeight();
      if (reducedMotion.matches || Math.abs(to - from) < 0.5) {
        settle(open);
        return;
      }

      const settledOpen = open;
      animation = node.animate(
        [{ height: `${from}px` }, { height: `${to}px` }],
        {
          duration: open ? options.openMs : options.closeMs,
          easing: EASING,
        },
      );
      animation.onfinish = () => {
        animation = null;
        settle(settledOpen);
      };
    },
    destroy() {
      stopAnimation();
      reducedMotion.removeEventListener("change", onMotionPreferenceChange);
    },
  };
}
