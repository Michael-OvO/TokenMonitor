/**
 * A trailing-edge debounce: `fn` runs once calls have stopped for `ms`,
 * e.g. once typing settles instead of on every keystroke.
 */
export interface Debounced {
  /** (Re)start the wait; `fn` runs when it ends. */
  schedule(): void;
  /** Run a pending call now (for unmount); nothing if none is pending. */
  flush(): void;
  /** Drop a pending call. */
  cancel(): void;
}

export function debounce(fn: () => void, ms: number): Debounced {
  let timer: ReturnType<typeof setTimeout> | null = null;

  function cancel() {
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
  }

  return {
    schedule() {
      cancel();
      timer = setTimeout(() => {
        timer = null;
        fn();
      }, ms);
    },
    flush() {
      if (timer === null) return;
      cancel();
      fn();
    },
    cancel,
  };
}
