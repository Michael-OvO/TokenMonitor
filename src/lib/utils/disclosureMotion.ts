/** Animate a disclosure's measured height without recalculating grid tracks
 * on every frame. Content stays mounted and returns to natural height when
 * open, so inputs and asynchronous content keep their state. */
export function disclosureMotion(node: HTMLElement, open: boolean) {
  const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
  let animation: Animation | null = null;

  function applyHeight() {
    node.style.height = open ? "auto" : "0px";
  }

  function stopAnimation() {
    animation?.cancel();
    animation = null;
  }

  function onMotionPreferenceChange() {
    if (reducedMotion.matches) stopAnimation();
  }

  applyHeight();
  reducedMotion.addEventListener("change", onMotionPreferenceChange);

  return {
    update(nextOpen: boolean) {
      if (nextOpen === open) return;
      open = nextOpen;

      // Read the in-flight position before cancelling so quick reversals
      // continue from the visible height instead of snapping to an endpoint.
      const from = node.getBoundingClientRect().height;
      const to = open ? node.firstElementChild!.getBoundingClientRect().height : 0;
      stopAnimation();
      applyHeight();
      if (reducedMotion.matches || Math.abs(to - from) < 0.5) return;

      animation = node.animate(
        [{ height: `${from}px` }, { height: `${to}px` }],
        {
          duration: open ? 180 : 140,
          easing: "cubic-bezier(0.2, 0, 0, 1)",
        },
      );
    },
    destroy() {
      stopAnimation();
      reducedMotion.removeEventListener("change", onMotionPreferenceChange);
    },
  };
}
