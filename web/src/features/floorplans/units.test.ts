import { describe, expect, it } from "vitest";
import { fitWithin, formatLength, fromMeters, scaleBar, toMeters } from "./units";

describe("floor plan lengths", () => {
  it("converts and shows feet and meters", () => {
    expect(toMeters(10, "ft")).toBeCloseTo(3.048);
    expect(fromMeters(3.048, "ft")).toBeCloseTo(10);
    expect(formatLength(2.345, "m")).toBe("2.3 m");
    expect(formatLength(30.48, "ft")).toBe("100 ft");
  });

  it("picks a round scale bar that fits", () => {
    // 2 cm per plan pixel at 1:1: 120 px is 2.4 m, so the bar is 2 m over 100 px.
    expect(scaleBar(0.02, 1, "m")).toEqual({ label: "2 m", px: 100 });
    // Zoomed in 4×: 120 px is 0.6 m, so 0.5 m.
    expect(scaleBar(0.02, 4, "m").label).toBe("0.5 m");
    const feet = scaleBar(0.3048, 1, "ft");
    expect(feet.label).toBe("100 ft");
    expect(feet.px).toBeCloseTo(100);
  });

  it("brings large images within the limit and leaves small ones", () => {
    expect(fitWithin(8192, 4096, 4096)).toBe(0.5);
    expect(fitWithin(800, 600, 4096)).toBe(1);
  });
});
