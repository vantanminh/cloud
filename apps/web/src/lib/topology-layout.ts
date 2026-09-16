export type NodePosition = {
  left: number
  top: number
}

export const TOPOLOGY_BOUNDS = {
  minLeft: 8,
  maxLeft: 92,
  minTop: 6,
  maxTop: 82,
}

export function clampPercent(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, Number(value.toFixed(2))))
}

export function moveNodePosition(
  current: NodePosition,
  delta: NodePosition,
  bounds = TOPOLOGY_BOUNDS
): NodePosition {
  return {
    left: clampPercent(
      current.left + delta.left,
      bounds.minLeft,
      bounds.maxLeft
    ),
    top: clampPercent(current.top + delta.top, bounds.minTop, bounds.maxTop),
  }
}

export function pointerDeltaPercent(
  movementX: number,
  movementY: number,
  containerWidth: number,
  containerHeight: number
): NodePosition {
  if (containerWidth <= 0 || containerHeight <= 0) {
    return { left: 0, top: 0 }
  }
  return {
    left: (movementX / containerWidth) * 100,
    top: (movementY / containerHeight) * 100,
  }
}

export function defaultPostgresPosition(): NodePosition {
  return { left: 22, top: 12 }
}

export function defaultRedisPosition(hasPostgres: boolean): NodePosition {
  return { left: hasPostgres ? 78 : 50, top: 12 }
}

export function defaultServicePosition(
  index: number,
  count: number,
  hasDatabase: boolean
): NodePosition {
  const column = index % 3
  const row = Math.floor(index / 3)
  return {
    left: count === 1 ? 50 : 20 + column * 30,
    top: (hasDatabase ? 42 : 28) + row * 30,
  }
}

export function mergePositions(
  defaults: Record<string, NodePosition>,
  stored: Record<string, NodePosition> | undefined
): Record<string, NodePosition> {
  return { ...defaults, ...(stored ?? {}) }
}
