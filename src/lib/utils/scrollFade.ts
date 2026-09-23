/** Mark a scroll container with `data-more-below` while content is hidden
 * past its bottom edge, so CSS can fade that edge instead of showing a
 * scrollbar. The native bar flashes on every remount (each tab switch
 * rebuilds the list), which reads as a glitch beside the numbers. */

// Sub-pixel row heights leave scrollHeight a fraction above clientHeight
// even at the very bottom.
const EPSILON_PX = 1;

export function scrollFade(node: HTMLElement) {
  function sync() {
    const more = node.scrollTop + node.clientHeight < node.scrollHeight - EPSILON_PX;
    node.toggleAttribute("data-more-below", more);
  }

  sync();
  node.addEventListener("scroll", sync, { passive: true });
  const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(sync);
  observer?.observe(node);

  return {
    destroy() {
      node.removeEventListener("scroll", sync);
      observer?.disconnect();
    },
  };
}
