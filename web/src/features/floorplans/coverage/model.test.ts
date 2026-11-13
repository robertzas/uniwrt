import { describe, expect, it } from "vitest";
import type { BandModel } from "../../../api/gen/BandModel";
import type { Floor } from "../../../api/gen/Floor";
import {
  best,
  DEAD_DBM,
  deadZones,
  elevation,
  grid,
  type Mask,
  MESH_DBM,
  predict,
  sources,
  suggest,
} from "./model";

/** A floor of 800 × 600 px at 0.1 m per px (80 × 60 m), with nodes at plan points. */
function floor(
  level: number,
  nodes: [number, number, number][],
  extra: Partial<Floor> = {},
): Floor {
  return {
    id: level + 10,
    name: `L${level}`,
    level,
    height_m: 4,
    image: { kind: "webp", width_px: 800, height_px: 600, bytes: 1, version: 1 },
    scale: { from: { x: 0, y: 0 }, to: { x: 100, y: 0 }, length_m: 10, unit: "m" },
    nodes: nodes.map(([node, x, y]) => ({ node, at: { x, y } })),
    updated_at: 0,
    ...extra,
  };
}

function model(extra: Partial<BandModel> = {}): BandModel {
  return {
    band: "5g",
    loss_at_1m_db: 47.3,
    exponent: 3,
    exponent_fitted: true,
    floor_loss_db: 15,
    floor_loss_fitted: false,
    samples: 2,
    error_db: 1.5,
    transmitters: [
      { node: 1, tx_power_dbm: 20 },
      { node: 2, tx_power_dbm: 23 },
      { node: 3, tx_power_dbm: 20 },
    ],
    ...extra,
  };
}

describe("the coverage model", () => {
  it("stacks floors as the controller does", () => {
    const floors = [floor(-1, [], { height_m: 2.5 }), floor(0, []), floor(2, [])];
    expect(elevation(floors, 1)).toBe(4);
    expect(elevation(floors, 2)).toBe(7);
    expect(elevation(floors, -1)).toBe(-2.5);
    expect(elevation(floors, -2)).toBe(-5.5);
  });

  it("places transmitters in meters, following a dragged one, skipping unscaled floors", () => {
    const floors = [
      floor(0, [[1, 100, 200]]),
      floor(1, [[2, 0, 0]]),
      floor(2, [[3, 5, 5]], { scale: null }),
    ];
    const list = sources(model(), floors, { floor: 10, at: new Map([[1, { x: 300, y: 200 }]]) });
    expect(list).toEqual([
      { node: 1, level: 0, x: 30, y: 20, z: 0, tx: 20 },
      { node: 2, level: 1, x: 0, y: 0, z: 4, tx: 23 },
    ]);
  });

  it("predicts the log-distance loss with the loss per floor crossed", () => {
    const [s] = sources(model(), [floor(1, [[1, 0, 0]])]);
    if (!s) throw new Error("no source");
    // 10 m away on the floor below (3 m down: level 0 isn't listed): 20 − 47.3 − 30·log10(√109) − 15.
    const expected = 20 - 47.3 - 30 * Math.log10(Math.hypot(10, 3)) - 15;
    expect(predict(model(), s, 10, 0, 0, 0)).toBeCloseTo(expected);
    // Within the first meter, the loss stays at 1 m's.
    expect(predict(model(), s, 0, 0, 3, 1)).toBeCloseTo(20 - 47.3);
    expect(best(model(), [], 0, 0, 0, 0)).toBe(Number.NEGATIVE_INFINITY);
  });

  it("finds the dead zones within the footprint", () => {
    const here = floor(0, [[1, 100, 100]]);
    const list = sources(model(), [here]);
    const g = grid(model(), list, here, [here]);
    if (!g) throw new Error("no grid");
    expect([g.cols, g.rows]).toEqual([160, 120]);
    expect(g.cellPx).toBe(5);
    // Cell (20, 20)'s center is 0.35 m from the node, (159, 119) about 85 m.
    expect(g.dbm[20 * 160 + 20]).toBeGreaterThan(-35);
    expect(g.dbm[119 * 160 + 159]).toBeLessThan(DEAD_DBM);
    const all = deadZones(g, undefined, 0.1);
    expect(all.area).toBeCloseTo(80 * 60);
    expect(all.dead).toBeGreaterThan(0.5 * all.area);
    // A footprint of only the 20 × 20 m around the node: nothing there is dead.
    const inside = new Uint8Array(160 * 120);
    for (let r = 0; r < 40; r++) for (let c = 0; c < 40; c++) inside[r * 160 + c] = 1;
    const mask: Mask = { cols: 160, rows: 120, inside, detected: true };
    expect(deadZones(g, mask, 0.1)).toEqual({ area: expect.closeTo(400), dead: 0 });
    expect(suggest(model(), list, here, [here], g, mask)).toBeUndefined();
  });

  it("suggests the spot reviving the most dead area that still hears the mesh", () => {
    const here = floor(0, [[1, 100, 300]]);
    const list = sources(model(), [here]);
    const g = grid(model(), list, here, [here]);
    if (!g) throw new Error("no grid");
    const free = suggest(model(), list, here, [here], g, undefined);
    const meshed = suggest(model(), list, here, [here], g, undefined, {
      model: model(),
      sources: list,
    });
    if (!free || !meshed) throw new Error("no suggestion");
    expect(free.gain).toBeGreaterThan(0);
    // Without a backhaul the best spot is far out; the mesh keeps it within reach of node 1.
    const reach = (at: { x: number; y: number }) =>
      best(model(), list, at.x * 0.1, at.y * 0.1, 0, 0);
    expect(reach(meshed.at)).toBeGreaterThanOrEqual(MESH_DBM);
    expect(reach(free.at)).toBeLessThan(MESH_DBM);
    expect(meshed.gain).toBeLessThanOrEqual(free.gain);
  });
});
