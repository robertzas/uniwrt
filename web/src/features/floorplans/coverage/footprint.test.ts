import { describe, expect, it } from "vitest";
import { footprint, type Pixels } from "./footprint";

/** A white image of `w` × `h` with black pixels where `ink` says. */
function image(w: number, h: number, ink: (x: number, y: number) => boolean): Pixels {
  const data = new Uint8ClampedArray(w * h * 4).fill(255);
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      if (ink(x, y)) data.set([0, 0, 0, 255], (y * w + x) * 4);
    }
  }
  return { width: w, height: h, data };
}

const share = (inside: Uint8Array) => inside.reduce((a, v) => a + v, 0) / inside.length;

describe("a plan's footprint", () => {
  // 50 × 40 cells read at 4 px each: a house's outline from cell 10 to 40 (x) and 8 to 32 (y),
  // with a doorway 2 cells wide in its bottom wall.
  const house = image(200, 160, (x, y) => {
    const onX = x >= 40 && x <= 160;
    const onY = y >= 32 && y <= 128;
    const door = x >= 96 && x < 104 && y > 120;
    return !door && ((onX && (y === 32 || y === 128)) || (onY && (x === 40 || x === 160)));
  });

  it("is what the walls enclose, doorways closed", () => {
    const mask = footprint(house, 50, 40, 2);
    expect(mask.detected).toBe(true);
    const at = (c: number, r: number) => mask.inside[r * 50 + c];
    expect(at(25, 20)).toBe(1);
    expect(at(24, 31)).toBe(1); // inside, beside the doorway
    expect(at(2, 2)).toBe(0);
    expect(at(25, 36)).toBe(0); // outside, below the doorway
    expect(share(mask.inside)).toBeCloseTo((31 * 25) / (50 * 40), 1);
  });

  it("counts the whole plan when nothing is enclosed or the drawing fills it", () => {
    const blank = footprint(
      image(200, 160, () => false),
      50,
      40,
      2,
    );
    expect(blank.detected).toBe(false);
    expect(share(blank.inside)).toBe(1);
    const busy = footprint(
      image(200, 160, (x, y) => (x * 7 + y * 13) % 5 === 0),
      50,
      40,
      2,
    );
    expect(busy.detected).toBe(false);
  });

  it("reads a dark plan's background from its border", () => {
    const inverted = { ...house, data: house.data.map((v, i) => (i % 4 === 3 ? v : 255 - v)) };
    expect(footprint(inverted, 50, 40, 2).detected).toBe(true);
  });
});
