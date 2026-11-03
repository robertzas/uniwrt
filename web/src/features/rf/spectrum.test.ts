import { describe, expect, it } from "vitest";
import { bss, channel, neighbor, rfView } from "../../test/rf";
import { HEIGHT, hill, labelled, PAD, scale, WIDTH } from "./spectrum";

const band5 = () => {
  const band = rfView().report.bands[1];
  if (!band) throw new Error("the fixture has 5 GHz");
  return band;
};

describe("the spectrum's geometry", () => {
  it("lays contiguous channels out in segments, with a break over each gap", () => {
    const s = scale(band5());
    // 36, 44, 52 and 149 touch no neighbor: four segments, three breaks.
    expect(s.breaks).toHaveLength(3);
    expect(s.x(5170)).toBe(PAD.left);
    expect(s.x(5755)).toBeCloseTo(WIDTH - PAD.right, 5);
    const [first = 0, , last = 0] = s.breaks;
    expect(s.x(5190)).toBeLessThan(first);
    expect(s.x(5210)).toBeGreaterThan(first);
    expect(s.x(5735)).toBeGreaterThan(last);
    // A frequency in a gap lands at the edge of the segment below it.
    expect(s.x(5200)).toBe(s.x(5190));
    expect(s.x(5500)).toBe(s.x(5270));
  });

  it("joins channels that touch into one segment", () => {
    const band = band5();
    band.channels = [36, 40, 44, 48].map((n) => channel(n));
    band.networks = [];
    expect(scale(band).breaks).toHaveLength(0);
  });

  it("puts signals between the floor and the ceiling, clamped", () => {
    const s = scale(band5());
    expect(s.y(-95)).toBe(HEIGHT - PAD.bottom);
    expect(s.y(-200)).toBe(s.y(-95));
    expect(s.y(-25)).toBe(PAD.top);
    expect(s.y(-60)).toBeLessThan(s.y(-80));
  });

  it("draws a network as a hill over the frequencies it covers", () => {
    const band = band5();
    const s = scale(band);
    const wide = band.networks[0];
    if (!wide) throw new Error("the fixture has a wide network");
    const path = hill(wide, s);
    expect(path.startsWith(`M${s.x(5170)},${s.base}`)).toBe(true);
    expect(path).toContain(`${Math.round(s.y(-58) * 10) / 10}`);
    expect(path.endsWith("Z")).toBe(true);
  });

  it("labels the site's own networks first, then the strongest", () => {
    const band = band5();
    const extra = neighbor(bss({ bssid: "aa:bb:cc:00:00:09", channel: 36, signal_dbm: -90 }));
    const picked = labelled([...band.networks, extra], 2);
    expect([...picked]).toEqual(["02:00:00:00:02:01", "aa:bb:cc:00:00:01"]);
  });
});
