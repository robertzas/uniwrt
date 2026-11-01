import { describe, expect, it } from "vitest";
import { align, hasData, integrate, type Line, latest, valueAt, valueRange } from "./data";
import { refreshEvery, resolveRange } from "./ranges";
import { formatUnit, METRIC_UNITS } from "./units";

const line = (at: number[], values: (number | null)[], extra: Partial<Line> = {}): Line => ({
  label: "x",
  color: "accent",
  at,
  values,
  ...extra,
});

describe("align", () => {
  it("merges the times; a line spans the times it lacks and keeps its breaks", () => {
    const a = align([
      line([0, 20, 40], [1, null, 3]),
      line([10, 20], [5, 6], { min: [4, 5], max: [6, 7] }),
    ]);
    expect(a.times).toEqual([0, 10, 20, 40]);
    expect(a.values[0]).toEqual([1, undefined, null, 3]);
    expect(a.values[1]).toEqual([undefined, 5, 6, undefined]);
    expect(a.bands[0]).toBeNull();
    expect(a.bands[1]).toEqual({
      min: [undefined, 4, 5, undefined],
      max: [undefined, 6, 7, undefined],
    });
    expect(hasData(a)).toBe(true);
    expect(hasData(align([line([0], [null])]))).toBe(false);
  });
});

describe("valueAt and latest", () => {
  it("read a line's own value, else its nearest point, but not across a break", () => {
    expect(valueAt([1, undefined, undefined, 4], 1)).toBe(1);
    expect(valueAt([undefined, undefined, 4], 0)).toBe(4);
    expect(valueAt([1, null, undefined, 4], 2)).toBe(4);
    expect(valueAt([null, undefined, null], 1)).toBeNull();
    expect(valueAt([undefined], 0)).toBeNull();
    expect(latest([1, 2, null, undefined])).toBe(2);
    expect(latest([undefined, null])).toBeNull();
  });
});

describe("valueRange", () => {
  it("leaves room around the data, honours bounds and a least span", () => {
    expect(valueRange(0, 100, { min: 0, max: 100 })).toEqual([0, 100]);
    const [low, high] = valueRange(10, 20, {});
    expect(low).toBeCloseTo(9);
    expect(high).toBeCloseTo(21);
    // 2 dB of wobble spans at least 20 dB, centered.
    const [s0, s1] = valueRange(-61, -59, { minSpan: 20 });
    expect(s1 - s0).toBeCloseTo(24);
    expect((s0 + s1) / 2).toBeCloseTo(-60);
    // Nothing yet, or all zeros, from 0.
    expect(valueRange(null, null, { min: 0 })).toEqual([0, 1]);
    expect(valueRange(0, 0, { min: 0 })[0]).toBe(0);
    expect(valueRange(0, 0, { min: 0 })[1]).toBeGreaterThan(0);
  });
});

describe("integrate", () => {
  it("sums the trapezoids between points, none across a break", () => {
    // 8 kb/s for 10 s, then a break, then 8 kb/s → 0 over 10 s.
    expect(integrate([0, 10_000, 20_000, 30_000], [8000, 8000, null, 0])).toBe(80_000);
    expect(integrate([0, 10_000, 20_000], [0, 8000, 0])).toBe(80_000);
    expect(integrate([], [])).toBe(0);
  });
});

describe("ranges", () => {
  it("end presets now, keep zooms, and refresh about as often as a point changes", () => {
    expect(resolveRange({ preset: "1h" }, 7_200_000)).toEqual({ from: 3_600_000, to: 7_200_000 });
    expect(resolveRange({ from: 1, to: 2 }, 9)).toEqual({ from: 1, to: 2 });
    expect(refreshEvery({ preset: "1h" })).toBe(15_000);
    expect(refreshEvery({ preset: "24h" })).toBe(60_000);
    expect(refreshEvery({ preset: "1y" })).toBe(900_000);
    expect(refreshEvery({ from: 1, to: 2 })).toBeNull();
  });
});

describe("units", () => {
  it("format each unit as the rest of the UI does", () => {
    expect(formatUnit(METRIC_UNITS.cpu)(42.4)).toBe("42 %");
    expect(formatUnit(METRIC_UNITS.down_bps)(2_400_000)).toBe("2.4 Mb/s");
    expect(formatUnit(METRIC_UNITS.tx_rate)(866_700)).toBe("867 Mb/s");
    expect(formatUnit(METRIC_UNITS.capacity)(1200)).toBe("1.2 Gb/s");
    expect(formatUnit(METRIC_UNITS.signal)(-58)).toBe("-58 dBm");
    expect(formatUnit(METRIC_UNITS.snr)(37)).toBe("37 dB");
    expect(formatUnit(METRIC_UNITS.stations)(3.4)).toBe("3.4");
  });
});
