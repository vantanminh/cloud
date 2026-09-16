import { describe, expect, it } from "vitest"

import {
  defaultServicePosition,
  mergePositions,
  moveNodePosition,
  pointerDeltaPercent,
} from "@/lib/topology-layout"

describe("topology layout", () => {
  it("updates a dragged node position inside the canvas", () => {
    const next = moveNodePosition({ left: 50, top: 40 }, { left: 10, top: -5 })
    expect(next).toEqual({ left: 60, top: 35 })
  })

  it("clamps drag so nodes stay on the board", () => {
    expect(
      moveNodePosition({ left: 90, top: 10 }, { left: 40, top: -40 })
    ).toEqual({ left: 92, top: 6 })
  })

  it("converts pointer movement into percent of the canvas", () => {
    expect(pointerDeltaPercent(100, 50, 1000, 500)).toEqual({
      left: 10,
      top: 10,
    })
    expect(pointerDeltaPercent(10, 10, 0, 0)).toEqual({ left: 0, top: 0 })
  })

  it("keeps persisted positions over defaults", () => {
    const defaults = {
      postgres: { left: 22, top: 12 },
      app: defaultServicePosition(0, 1, true),
    }
    const merged = mergePositions(defaults, {
      postgres: { left: 30, top: 18 },
    })
    expect(merged.postgres).toEqual({ left: 30, top: 18 })
    expect(merged.app).toEqual({ left: 50, top: 42 })
  })
})
