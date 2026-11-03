import { describe, expect, it } from "vitest";
import { busyTone, crowding, linkTone, relative } from "./rf";

describe("RF numbers in words", () => {
  it("rates interference, busy shares and links", () => {
    expect([0, 0.29, 0.3, 0.99, 1, 3].map(crowding)).toEqual([
      "clean",
      "clean",
      "some",
      "some",
      "crowded",
      "crowded",
    ]);
    expect([null, 10, 50, 74.9, 75].map(busyTone)).toEqual([
      "neutral",
      "ok",
      "warn",
      "warn",
      "crit",
    ]);
    expect([-50, -65, -66, -75, -76].map(linkTone)).toEqual(["ok", "ok", "warn", "warn", "crit"]);
  });

  it("tells times relative to now", () => {
    const now = 1_000_000_000;
    expect(relative(now - 30_000, now)).toBe("just now");
    expect(relative(now + 59_000, now)).toBe("just now");
    expect(relative(now - 2 * 3_600_000, now)).toMatch(/^2h.* ago$/);
    expect(relative(now + 40 * 60_000, now)).toMatch(/^in 40m/);
  });
});
