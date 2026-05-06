import type { FloatBallExpandDirection } from "../types/index.js";

// ── Constants ──────────────────────────────────────────────────────────

export const BALL_SIZE_CSS_PX = 56;
export const EXPANDED_WIDTH_CSS_PX = 152;
export const EXPAND_MARGIN_CSS_PX = 8;

// ── Formatting helpers ─────────────────────────────────────────────────

export function percent(value: number | null): string {
  return value == null ? "N/A" : `${Math.round(value)}%`;
}

export function fillWidth(value: number | null): string {
  const safe = value == null ? 0 : Math.max(0, Math.min(value, 100));
  return `${safe}%`;
}

export function formatBallCost(cost: number): string {
  if (cost <= 0) return "$0";
  if (cost < 1) return `$${cost.toFixed(2)}`;
  if (cost < 10) return `$${cost.toFixed(1)}`;
  return `$${Math.round(cost)}`;
}

export function formatPoint(point: { x: number; y: number } | null): string {
  return point ? `(${point.x}, ${point.y})` : "n/a";
}

export function formatMonitor(
  monitor: {
    position: { x: number; y: number };
    size: { width: number; height: number };
  } | null,
): string {
  if (!monitor) return "none";
  return `pos=${formatPoint(monitor.position)} size=${monitor.size.width}x${monitor.size.height}`;
}

export function formatError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function formatInteraction(interactionId: string | null | undefined): string {
  return `interaction=${interactionId ?? "n/a"}`;
}

// ── Expand direction resolution ────────────────────────────────────────

export type FloatBallPositionPayload = {
  x: number;
  y: number;
  anchor?: "top" | "left" | "right" | "bottom" | null;
  expanded?: boolean;
  ballOffsetX?: number;
};

export type MonitorInfo = {
  position: { x: number; y: number };
  size: { width: number; height: number };
  workArea?: {
    position: { x: number; y: number };
    size: { width: number; height: number };
  };
  scaleFactor?: number;
};

export function resolveExpandDirection(
  position: FloatBallPositionPayload,
  monitor: MonitorInfo | null,
  currentDirection: FloatBallExpandDirection,
): FloatBallExpandDirection {
  if (position.anchor === "left") return "right";
  if (position.anchor === "right") return "left";
  if (!monitor) return currentDirection;

  const workArea = monitor.workArea ?? {
    position: monitor.position,
    size: monitor.size,
  };
  const scale = monitor.scaleFactor ?? 1;
  const ballWidth = Math.round(BALL_SIZE_CSS_PX * scale);
  const expandedWidth = Math.round(EXPANDED_WIDTH_CSS_PX * scale);
  const expandMargin = Math.round(EXPAND_MARGIN_CSS_PX * scale);
  const innerLeft = workArea.position.x + expandMargin;
  const innerRight = workArea.position.x + workArea.size.width - expandMargin;
  const roomRight = innerRight - (position.x + expandedWidth);
  const roomLeft = position.x - (expandedWidth - ballWidth) - innerLeft;

  if (currentDirection === "right") {
    if (roomRight >= 0) return "right";
    if (roomLeft >= 0) return "left";
  } else {
    if (roomLeft >= 0) return "left";
    if (roomRight >= 0) return "right";
  }

  if (roomRight > roomLeft) return "right";
  if (roomLeft > roomRight) return "left";
  return currentDirection;
}
