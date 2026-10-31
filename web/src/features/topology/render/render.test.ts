import { Router, Wifi } from "lucide";
import { describe, expect, it } from "vitest";
import { house } from "../../../test/topology";
import { graphOf } from "../graph";
import type { Point } from "../layout/tree";
import { buildScene } from "../scene";
import {
  fitCamera,
  flowOf,
  hitTest,
  linkWidth,
  particleAt,
  toScreen,
  toWorld,
  zoomAt,
} from "./geometry";
import { iconPathData } from "./icons";
import { qualityStep } from "./palette";

describe("the camera", () => {
  it("fits every point inside the view, centred", () => {
    const insets = { top: 10, right: 20, bottom: 30, left: 20 };
    const points = [
      { x: -100, y: 0 },
      { x: 100, y: 50 },
    ];
    const camera = fitCamera(points, 800, 400, insets);
    const [left, right] = points.map((point) => toScreen(camera, point)) as [Point, Point];
    expect(left.x).toBeGreaterThanOrEqual(20);
    expect(right.x).toBeLessThanOrEqual(800 - 20);
    expect((left.x + right.x) / 2).toBeCloseTo(400);
    expect((left.y + right.y) / 2).toBeCloseTo(10 + 360 / 2);
    expect(fitCamera([], 800, 400, insets)).toEqual({ x: 400, y: 190, k: 1 });
  });

  it("zooms around a point that stays put, within limits", () => {
    const camera = { x: 30, y: -20, k: 1 };
    const at = { x: 200, y: 100 };
    const world = toWorld(camera, at);
    const zoomed = zoomAt(camera, 2, at);
    expect(zoomed.k).toBe(2);
    expect(toScreen(zoomed, world).x).toBeCloseTo(at.x);
    expect(toScreen(zoomed, world).y).toBeCloseTo(at.y);
    expect(zoomAt(camera, 100, at).k).toBe(4);
  });
});

describe("hit testing", () => {
  const scene = buildScene(graphOf(house()), "rf", {
    clustered: () => false,
    nodeName: () => undefined,
  });
  const positions = new Map([
    ["internet", { x: 0, y: 0 }],
    ["node:1", { x: 0, y: 100 }],
    ["node:2", { x: 200, y: 100 }],
    ["peer:02:00:00:00:00:99", { x: 400, y: 100 }],
  ]);
  const camera = { x: 0, y: 0, k: 1 };

  it("finds the vertex under the pointer, else the link beside it", () => {
    expect(hitTest(scene, positions, camera, { x: 3, y: 103 })).toEqual({
      kind: "vertex",
      id: "node:1",
    });
    expect(hitTest(scene, positions, camera, { x: 100, y: 103 })).toEqual({
      kind: "link",
      id: "node:1~node:2",
    });
    expect(hitTest(scene, positions, camera, { x: 100, y: 160 })).toBeUndefined();
  });
});

describe("links", () => {
  it("widen and flow faster with their traffic", () => {
    const mesh = house().edges[1] ?? null;
    expect(linkWidth(null)).toBe(1.5);
    expect(linkWidth(mesh)).toBeGreaterThan(1.5);
    expect(linkWidth(mesh)).toBeLessThanOrEqual(9);
    expect(flowOf(500)).toBeUndefined();
    const slow = flowOf(10_000);
    const fast = flowOf(100_000_000);
    expect(fast?.count).toBeGreaterThan(slow?.count ?? 0);
    expect(fast?.speed).toBeGreaterThan(slow?.speed ?? 0);
    expect(particleAt(1.25, 1, 0, 2)).toBeCloseTo(0.25);
    expect(particleAt(1.25, 1, 1, 2)).toBeCloseTo(0.75);
    expect([0, 19, 20, 99, 100].map(qualityStep)).toEqual([0, 0, 1, 4, 4]);
  });
});

describe("icons", () => {
  it("turn Lucide shapes into one path", () => {
    const d = iconPathData([
      ["circle", { cx: 12, cy: 12, r: 2 }],
      ["rect", { x: 2, y: 4, width: 20, height: 8 }],
      ["line", { x1: 1, y1: 2, x2: 3, y2: 4 }],
      ["polyline", { points: "1 2 3 4" }],
    ]);
    expect(d).toBe("M10 12a2 2 0 1 0 4 0a2 2 0 1 0 -4 0Z M2 4h20v8h-20Z M1 2L3 4 M1 2L3 4");
    expect(iconPathData(Router)).toContain("M");
    expect(iconPathData(Wifi).length).toBeGreaterThan(10);
  });
});
