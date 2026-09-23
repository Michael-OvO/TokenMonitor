/**
 * Hover tracking that survives the gap between two adjacent targets.
 *
 * Neighbouring targets (donut slices, list rows) are separated by a sliver
 * nobody owns, so a pointer sliding from one to the next fires `mouseleave`
 * on the first, rests in the gap for a frame or two, and only then fires
 * `mouseenter` on the second. Clearing the hover on that `mouseleave` paints
 * the "nothing hovered" state in between. Here a leave only schedules the
 * clear; entering a target (the neighbour or the same one again) within
 * `leaveDelayMs` cancels it, so the hover hands over directly.
 */
export interface StickyHoverOptions {
  /** How long the last target stays hovered after the pointer leaves it. */
  leaveDelayMs: number;
  /** Called with the hovered index, or -1 once nothing is hovered. */
  onChange: (index: number) => void;
}

export interface StickyHover {
  /** Currently hovered index, -1 for none. */
  readonly current: number;
  /** The pointer entered target `index`. */
  enter(index: number): void;
  /** The pointer left the current target; the clear waits `leaveDelayMs`. */
  leave(): void;
  /** Drop the target at once (the pointer left the whole group). */
  clear(): void;
  /** Cancel a pending clear without reporting; for unmount. */
  dispose(): void;
}

export function createStickyHover({ leaveDelayMs, onChange }: StickyHoverOptions): StickyHover {
  let current = -1;
  let leaveTimer: ReturnType<typeof setTimeout> | null = null;

  function cancelLeave() {
    if (leaveTimer === null) return;
    clearTimeout(leaveTimer);
    leaveTimer = null;
  }

  function set(index: number) {
    if (index === current) return;
    current = index;
    onChange(index);
  }

  return {
    get current() {
      return current;
    },
    enter(index) {
      cancelLeave();
      set(index);
    },
    leave() {
      if (current === -1) return;
      cancelLeave();
      if (leaveDelayMs <= 0) {
        set(-1);
        return;
      }
      leaveTimer = setTimeout(() => {
        leaveTimer = null;
        set(-1);
      }, leaveDelayMs);
    },
    clear() {
      cancelLeave();
      set(-1);
    },
    dispose: cancelLeave,
  };
}
