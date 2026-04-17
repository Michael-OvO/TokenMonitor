export type DragScaleDetectionInput = {
  scale: number;
  windowX: number;
  windowY: number;
  clientX: number;
  clientY: number;
  screenX: number;
  screenY: number;
};

export type PhysicalDragPositionInput = {
  startScreenX: number;
  startScreenY: number;
  startWindowX: number;
  startWindowY: number;
  screenX: number;
  screenY: number;
  screenToPhysicalScale: number;
};

function transitionError(
  screenX: number,
  screenY: number,
  expectedPhysicalX: number,
  expectedPhysicalY: number,
  screenToPhysicalScale: number,
): number {
  const physicalX = screenX * screenToPhysicalScale;
  const physicalY = screenY * screenToPhysicalScale;
  return Math.abs(physicalX - expectedPhysicalX) + Math.abs(physicalY - expectedPhysicalY);
}

export function shouldHandleFloatBallPointerButton(button: number, isLinux: boolean): boolean {
  return button === 0 || button === 2;
}

export function detectScreenToPhysicalScale({
  scale,
  windowX,
  windowY,
  clientX,
  clientY,
  screenX,
  screenY,
}: DragScaleDetectionInput): number {
  if (!Number.isFinite(scale) || scale <= 1.05) {
    return 1;
  }

  const expectedPhysicalX = windowX + Math.round(clientX * scale);
  const expectedPhysicalY = windowY + Math.round(clientY * scale);
  const physicalError = transitionError(
    screenX,
    screenY,
    expectedPhysicalX,
    expectedPhysicalY,
    1,
  );
  const logicalError = transitionError(
    screenX,
    screenY,
    expectedPhysicalX,
    expectedPhysicalY,
    scale,
  );

  return logicalError < physicalError ? scale : 1;
}

export function getPhysicalWindowPositionFromPointer({
  startScreenX,
  startScreenY,
  startWindowX,
  startWindowY,
  screenX,
  screenY,
  screenToPhysicalScale,
}: PhysicalDragPositionInput): { x: number; y: number } {
  const dx = (screenX - startScreenX) * screenToPhysicalScale;
  const dy = (screenY - startScreenY) * screenToPhysicalScale;

  return {
    x: startWindowX + Math.round(dx),
    y: startWindowY + Math.round(dy),
  };
}
