import { describe, expect, it } from "vitest";
import { house } from "../../../test/topology";
import { graphOf } from "../graph";
import { buildScene } from "../scene";
import { ForceRunner, type LayoutPositions, type LayoutRequest } from "./force";
import { treeLayout } from "./tree";

const options = { clustered: () => false, nodeName: () => undefined };

describe("the tree layout", () => {
  it("puts each level below its parent, the parent centred over its children", () => {
    const scene = buildScene(graphOf(house()), "tree", options);
    const at = treeLayout(scene);
    const point = (id: string) => at.get(id) ?? { x: Number.NaN, y: Number.NaN };
    expect(point("internet").y).toBe(0);
    expect(point("node:1").y).toBeGreaterThan(point("internet").y);
    expect(point("node:2").y).toBeGreaterThan(point("node:1").y);
    const phone = point("client:aa:00:00:00:00:01");
    const other = point("client:aa:00:00:00:00:02");
    expect(phone.y).toBe(other.y);
    expect(point("node:2").x).toBeCloseTo((phone.x + other.x) / 2);
    // The peer has no uplink: a root of its own, beside the internet.
    expect(point("peer:02:00:00:00:00:99").y).toBe(0);
    expect(at.size).toBe(scene.vertices.length);
  });
});

describe("the force layout", () => {
  function settle(requests: LayoutRequest[]): LayoutPositions {
    let last: LayoutPositions | undefined;
    const runner = new ForceRunner((message) => {
      last = message;
    });
    for (const request of requests) runner.handle(request);
    runner.settle();
    runner.handle({ type: "stop" });
    if (!last) throw new Error("no positions");
    return last;
  }

  const scene = buildScene(graphOf(house()), "clients", options);
  const request: LayoutRequest = {
    type: "scene",
    seq: 7,
    vertices: scene.vertices.map(({ id, kind, radius }) => ({ id, kind, radius })),
    links: scene.links.map(({ source, target, medium }) => ({ source, target, medium })),
    pins: { "node:1": { x: 10, y: 20 } },
  };

  it("keeps pinned vertices where they are and Wi-Fi clients near their access point", () => {
    const result = settle([request]);
    expect(result.seq).toBe(7);
    expect(result.settled).toBe(true);
    const point = (id: string) => {
      const i = scene.vertices.findIndex((v) => v.id === id);
      return { x: result.positions[2 * i] ?? 0, y: result.positions[2 * i + 1] ?? 0 };
    };
    expect(point("node:1")).toEqual({ x: 10, y: 20 });
    const distance = (a: { x: number; y: number }, b: { x: number; y: number }) =>
      Math.hypot(a.x - b.x, a.y - b.y);
    const phone = point("client:aa:00:00:00:00:01");
    const ap = point("node:2");
    const gateway = point("node:1");
    expect(distance(phone, ap)).toBeLessThan(distance(phone, gateway));
  });

  it("lays out the same scene the same way", () => {
    expect([...settle([request]).positions]).toEqual([...settle([request]).positions]);
  });
});
