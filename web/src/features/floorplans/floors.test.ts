import { describe, expect, it } from "vitest";
import type { Floor } from "../../api/gen/Floor";
import { summary } from "../../test/fixtures";
import { groupDevices, nextLevel, withFloor, withoutNode } from "./floors";

function floor(id: number, level: number, nodes: number[]): Floor {
  return {
    id,
    name: `F${id}`,
    level,
    height_m: 3,
    image: null,
    scale: null,
    nodes: nodes.map((node) => ({ node, at: { x: node, y: node } })),
    walls: [],
    updated_at: 0,
  };
}

describe("the floor list", () => {
  it("takes a saved floor's devices off the others and keeps the levels in order", () => {
    const list = [floor(1, 0, [1, 2]), floor(2, 1, [3])];
    const saved = floor(2, 1, [3, 2]);
    const next = withFloor(list, saved);
    expect(next.map((f) => [f.id, f.nodes.map((p) => p.node)])).toEqual([
      [1, [1]],
      [2, [3, 2]],
    ]);
    expect(withFloor(next, floor(3, -1, [])).map((f) => f.id)).toEqual([3, 1, 2]);
    expect(withoutNode(next, 1, 1)[0]?.nodes).toEqual([]);
  });

  it("groups the devices by where they are", () => {
    const floors = [floor(1, 0, [1]), floor(2, 1, [2])];
    const nodes = [summary(1, "Gateway"), summary(2, "Attic"), summary(3, "Garage")];
    const groups = groupDevices(nodes, floors, floors[0] as Floor);
    expect(groups.here.map((n) => n.name)).toEqual(["Gateway"]);
    expect(groups.elsewhere.map((e) => [e.node.name, e.floor.name])).toEqual([["Attic", "F2"]]);
    expect(groups.unplaced.map((n) => n.name)).toEqual(["Garage"]);
  });

  it("puts a new floor above the highest", () => {
    expect(nextLevel([])).toBe(0);
    expect(nextLevel([floor(1, -1, []), floor(2, 2, [])])).toBe(3);
  });
});
