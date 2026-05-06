import { getPhysicalWindowPositionFromPointer } from "./floatBallInteraction.js";

export type DragState = {
  pointerId: number;
  button: number;
  interactionId: string;
  startScreenX: number;
  startScreenY: number;
  startWindowX: number;
  startWindowY: number;
  screenToPhysicalScale: number;
  initiated: boolean;
};

export type DragMoveResult = {
  physicalX: number;
  physicalY: number;
  shouldInitiate: boolean;
};

const DRAG_THRESHOLD_PX = 5;

export class DragMachine {
  private _interactionCounter = 0;

  nextInteractionId(kind: string): string {
    this._interactionCounter += 1;
    return `${kind}-${Date.now().toString(36)}-${this._interactionCounter}`;
  }

  createDragState(
    pointerId: number,
    button: number,
    interactionId: string,
    screenX: number,
    screenY: number,
    windowX: number,
    windowY: number,
    scale: number,
  ): DragState {
    return {
      pointerId,
      button,
      interactionId,
      startScreenX: screenX,
      startScreenY: screenY,
      startWindowX: windowX,
      startWindowY: windowY,
      screenToPhysicalScale: scale,
      initiated: false,
    };
  }

  isThresholdExceeded(state: DragState, screenX: number, screenY: number): boolean {
    const dx = screenX - state.startScreenX;
    const dy = screenY - state.startScreenY;
    return Math.hypot(dx, dy) >= DRAG_THRESHOLD_PX;
  }

  computeMove(state: DragState, screenX: number, screenY: number): DragMoveResult | null {
    const wasInitiated = state.initiated;
    const exceeded = wasInitiated || this.isThresholdExceeded(state, screenX, screenY);

    if (!exceeded) return null;

    const { x, y } = getPhysicalWindowPositionFromPointer({
      startScreenX: state.startScreenX,
      startScreenY: state.startScreenY,
      startWindowX: state.startWindowX,
      startWindowY: state.startWindowY,
      screenX,
      screenY,
      screenToPhysicalScale: state.screenToPhysicalScale,
    });

    return {
      physicalX: x,
      physicalY: y,
      shouldInitiate: !wasInitiated,
    };
  }
}
