import { describe, expect, it } from "vitest";
import type { Wall } from "../../../api/gen/Wall";
import { endsAt, hitWall, moveEnds, snap } from "./snap";

const wall = (x1: number, y1: number, x2: number, y2: number): Wall => ({
  from: { x: x1, y: y1 },
  to: { x: x2, y: y2 },
  material: "drywall",
});
// A room's corner at (100, 100): a wall along the top and one down the left.
const walls = [wall(100, 100, 300, 100), wall(100, 100, 100, 300)];

describe("snapping walls", () => {
  it("lands on wall ends first", () => {
    expect(snap({ x: 104, y: 97 }, undefined, walls, 10, false)).toEqual({
      point: { x: 100, y: 100 },
      kind: "end",
    });
    expect(snap({ x: 104, y: 97 }, undefined, walls, 10, true).kind).toBe("none");
  });

  it("starts on a wall's line, or where the click is", () => {
    expect(snap({ x: 200, y: 106 }, undefined, walls, 10, false)).toEqual({
      point: { x: 200, y: 100 },
      kind: "line",
    });
    expect(snap({ x: 200, y: 200 }, undefined, walls, 10, false).kind).toBe("none");
  });

  it("rules lines at 45° steps from the last corner, meeting walls on the way", () => {
    // From (200, 300) towards the top wall: straight up, ending on it.
    expect(snap({ x: 205, y: 108 }, { x: 200, y: 300 }, walls, 10, false)).toEqual({
      point: { x: 200, y: 100 },
      kind: "line",
    });
    // Far from any wall: along the diagonal.
    const diagonal = snap({ x: 262, y: 238 }, { x: 200, y: 300 }, walls, 10, false);
    expect(diagonal.kind).toBe("angle");
    expect(diagonal.point.x - 200).toBeCloseTo(300 - diagonal.point.y);
  });

  it("finds the wall under a click", () => {
    expect(hitWall({ x: 150, y: 104 }, walls, 6)).toBe(0);
    expect(hitWall({ x: 97, y: 250 }, walls, 6)).toBe(1);
    expect(hitWall({ x: 150, y: 150 }, walls, 6)).toBeUndefined();
  });

  it("moves a shared corner with every wall on it, dropping walls that vanish", () => {
    const corner = endsAt({ x: 100, y: 100 }, walls);
    expect(corner).toEqual([
      { wall: 0, end: "from" },
      { wall: 1, end: "from" },
    ]);
    const moved = moveEnds(walls, corner, { x: 90, y: 90 });
    expect(moved.map((w) => w.from)).toEqual([
      { x: 90, y: 90 },
      { x: 90, y: 90 },
    ]);
    // Dragging the top wall's far end onto its corner removes it.
    expect(moveEnds(walls, [{ wall: 0, end: "to" }], { x: 100, y: 100 })).toEqual([walls[1]]);
    // Ends being dragged don't snap to themselves.
    expect(snap({ x: 101, y: 101 }, undefined, walls, 10, false, corner).kind).not.toBe("end");
  });
});
