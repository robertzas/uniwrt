import { describe, expect, it } from "vitest";
import { colorAt, paint, type Stop } from "./Heatmap";

const stops: Stop[] = [
  { value: -80, rgb: [200, 0, 0] },
  { value: -60, rgb: [0, 200, 100] },
];

describe("the heatmap", () => {
  it("blends between stops and clamps beyond them", () => {
    expect(colorAt(stops, -90)).toEqual([200, 0, 0]);
    expect(colorAt(stops, -70)).toEqual([100, 100, 50]);
    expect(colorAt(stops, -40)).toEqual([0, 200, 100]);
  });

  it("paints nothing where the browser has no canvas", () => {
    expect(
      paint({ cols: 1, rows: 1, values: new Float32Array([-70]) }, stops, 0.5),
    ).toBeUndefined();
  });
});
