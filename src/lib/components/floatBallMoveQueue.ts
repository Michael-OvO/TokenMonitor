export interface MoveQueueDeps {
  requestAnimationFrame: (cb: FrameRequestCallback) => number;
  cancelAnimationFrame: (id: number) => void;
  sendMove: (x: number, y: number, interactionId: string | null | undefined) => Promise<void>;
}

export interface QueuedMove {
  x: number;
  y: number;
  interactionId: string | null | undefined;
}

export class MoveQueue {
  private queued: QueuedMove | null = null;
  private flushRaf: number | null = null;
  private inFlight = false;
  private sequence = 0;

  constructor(private deps: MoveQueueDeps) {}

  /** Public entry point: coalesce the latest target and schedule a RAF flush. */
  queue(x: number, y: number, interactionId: string | null | undefined): void {
    this.queued = { x, y, interactionId };
    if (!this.inFlight) {
      this.requestFlush();
    }
  }

  /** Cancel any pending RAF without clearing in-flight state. */
  cancel(): void {
    this.queued = null;
    if (this.flushRaf !== null) {
      this.deps.cancelAnimationFrame(this.flushRaf);
      this.flushRaf = null;
    }
  }

  /** Cancel + full teardown. Call on unmount. */
  destroy(): void {
    this.cancel();
    this.inFlight = false;
  }

  /** Bump and return the move sequence counter (used by callers for IPC ordering). */
  nextSequence(): number {
    return ++this.sequence;
  }

  private requestFlush(): void {
    if (this.flushRaf !== null) return;
    this.flushRaf = this.deps.requestAnimationFrame(() => {
      this.flushRaf = null;
      void this.flush();
    });
  }

  private async flush(): Promise<void> {
    if (this.inFlight || !this.queued) return;
    const next = this.queued;
    this.queued = null;
    this.inFlight = true;

    try {
      await this.deps.sendMove(next.x, next.y, next.interactionId);
    } catch {
      // Caller's sendMove is expected to handle its own logging.
    } finally {
      this.inFlight = false;
      if (this.queued) {
        this.requestFlush();
      }
    }
  }
}
