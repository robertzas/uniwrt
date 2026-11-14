import { describe, expect, it } from "vitest";
import type { Floor } from "../../../api/gen/Floor";
import type { Material } from "../../../api/gen/Material";
import { crosses, type Losses, storeys, WallIndex, wallLoss } from "./walls";

const LOSSES: Losses = new Map<Material, number>([
  ["glass", 4],
  ["drywall", 4],
  ["wood", 6],
  ["brick", 12],
  ["concrete", 18],
  ["metal", 32],
]);
const MATERIALS = [...LOSSES.keys()];

/** A seeded generator, so a failure repeats. */
function random(seed: number) {
  let state = seed;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 2 ** 32;
  };
}

type Seg = { ax: number; ay: number; bx: number; by: number; material: Material };

function brute(walls: readonly Seg[], px: number, py: number, qx: number, qy: number) {
  let total = 0;
  for (const w of walls) {
    if (crosses(px, py, qx, qy, w.ax, w.ay, w.bx, w.by)) total += LOSSES.get(w.material) ?? 0;
  }
  return total;
}

/** A floor at 0.1 m per px with walls given in meters. */
function floor(level: number, walls: [number, number, number, number, Material][]): Floor {
  return {
    id: level,
    name: `L${level}`,
    level,
    height_m: 3,
    image: { kind: "webp", width_px: 800, height_px: 600, bytes: 1, version: 1 },
    scale: { from: { x: 0, y: 0 }, to: { x: 100, y: 0 }, length_m: 10, unit: "m" },
    nodes: [],
    walls: walls.map(([ax, ay, bx, by, material]) => ({
      from: { x: ax * 10, y: ay * 10 },
      to: { x: bx * 10, y: by * 10 },
      material,
    })),
    updated_at: 0,
  };
}

describe("walls", () => {
  it("find exactly the walls a segment crosses, as testing every wall does", () => {
    const next = random(7);
    const walls: Seg[] = [];
    for (let i = 0; i < 300; i++) {
      const ax = next() * 40;
      const ay = next() * 30;
      // Mostly axis-aligned rooms, some diagonal walls.
      const kind = next();
      const len = 1 + next() * 10;
      const [bx, by] =
        kind < 0.45 ? [ax + len, ay] : kind < 0.9 ? [ax, ay + len] : [ax + len, ay + len * next()];
      walls.push({ ax, ay, bx, by, material: MATERIALS[i % MATERIALS.length] as Material });
    }
    const index = new WallIndex(walls);
    for (let i = 0; i < 2000; i++) {
      // Ends inside and outside the walls' box.
      const [px, py, qx, qy] = [
        next() * 60 - 10,
        next() * 50 - 10,
        next() * 60 - 10,
        next() * 50 - 10,
      ];
      expect(index.loss(px, py, qx, qy, LOSSES)).toBe(brute(walls, px, py, qx, qy));
    }
    // Along an axis and degenerate.
    expect(index.loss(-5, 10, 50, 10, LOSSES)).toBe(brute(walls, -5, 10, 50, 10));
    expect(index.loss(10, -5, 10, 45, LOSSES)).toBe(brute(walls, 10, -5, 10, 45));
    expect(index.loss(3, 3, 3, 3, LOSSES)).toBe(0);
  });

  it("count each storey's walls on the part of a path within it, as the controller does", () => {
    // The controller's case (walls.rs): the path rises from z 1 to z 4 over x 0 → 12 m and
    // leaves the ground storey at x 8 m, right on the brick wall.
    const floors = [
      floor(0, [[8, -1, 8, 1, "brick"]]),
      floor(1, [
        [2, -1, 2, 1, "metal"],
        [10, -1, 10, 1, "glass"],
      ]),
    ];
    const list = storeys(
      floors,
      () => 0.1,
      (level) => level * 3,
    );
    const low = { level: 0, x: 0, y: 0, z: 0 };
    const high = { level: 1, x: 12, y: 0, z: 3 };
    expect(wallLoss(list, LOSSES, low, high)).toBe(12 + 4);
    expect(wallLoss(list, LOSSES, high, low)).toBe(12 + 4);
    expect(wallLoss(list, LOSSES, { ...low, level: 1, z: 3 }, high)).toBe(32 + 4);
    expect(wallLoss([], LOSSES, low, high)).toBe(0);
    // Unscaled floors and floors without walls index nothing.
    expect(
      storeys(
        [floor(2, [])],
        () => 0.1,
        () => 0,
      ),
    ).toEqual([]);
    expect(
      storeys(
        floors,
        () => undefined,
        () => 0,
      ),
    ).toEqual([]);
  });

  it("index a heatmap's worth of paths quickly", () => {
    const next = random(3);
    const walls: Seg[] = [];
    for (let i = 0; i < 400; i++) {
      const ax = next() * 80;
      const ay = next() * 60;
      walls.push(
        next() < 0.5
          ? { ax, ay, bx: ax + 6, by: ay, material: "drywall" }
          : { ax, ay, bx: ax, by: ay + 6, material: "brick" },
      );
    }
    const index = new WallIndex(walls);
    const start = performance.now();
    let sum = 0;
    // Four sources over a 160 × 120 grid.
    for (let s = 0; s < 4; s++) {
      for (let r = 0; r < 120; r++) {
        for (let c = 0; c < 160; c++) sum += index.loss(20 * s, 30, c * 0.5, r * 0.5, LOSSES);
      }
    }
    const ms = performance.now() - start;
    expect(sum).toBeGreaterThan(0);
    // Generous for slow CI machines; a laptop takes a small fraction of it.
    expect(ms).toBeLessThan(2000);
  });
});
