import { describe, expect, it } from "vitest";
import type { Wall } from "../../../../api/gen/Wall";
import type { Pixels } from "../../coverage/footprint";
import { type TracedWall, trace, workSize } from "./trace";

/** A white plan to draw on in black (or `shade`). */
function plan(width: number, height: number) {
  const data = new Uint8ClampedArray(width * height * 4).fill(255);
  const dot = (x: number, y: number, shade = 0) => {
    if (x < 0 || y < 0 || x >= width || y >= height) return;
    const i = (Math.round(y) * width + Math.round(x)) * 4;
    data[i] = data[i + 1] = data[i + 2] = shade;
  };
  return {
    pixels: { width, height, data } as Pixels,
    /** A filled rectangle, ends inclusive of `x0`, `y0`, exclusive of `x1`, `y1`. */
    rect(x0: number, y0: number, x1: number, y1: number, shade = 0) {
      for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) dot(x, y, shade);
    },
    /** A stroke `w` pixels thick from (x0, y0) to (x1, y1). */
    line(x0: number, y0: number, x1: number, y1: number, w = 1) {
      const n = Math.ceil(Math.hypot(x1 - x0, y1 - y0) * 2);
      for (let s = 0; s <= n; s++) {
        const x = x0 + ((x1 - x0) * s) / n;
        const y = y0 + ((y1 - y0) * s) / n;
        for (let a = -(w - 1) / 2; a <= (w - 1) / 2; a += 0.5) {
          for (let b = -(w - 1) / 2; b <= (w - 1) / 2; b += 0.5) dot(x + a, y + b);
        }
      }
    },
  };
}

const near = (w: TracedWall, from: [number, number], to: [number, number], tol = 3) => {
  const ok = (p: { x: number; y: number }, q: [number, number]) =>
    Math.abs(p.x - q[0]) <= tol && Math.abs(p.y - q[1]) <= tol;
  return (ok(w.from, from) && ok(w.to, to)) || (ok(w.from, to) && ok(w.to, from));
};
const find = (walls: TracedWall[], from: [number, number], to: [number, number], tol = 3) =>
  walls.find((w) => near(w, from, to, tol));

/**
 * A 20 m × 12 m flat at 50 px/m: a 32 cm brick shell, a 16 cm wall with a doorway, a 45° wall,
 * and a wall drawn as two thin lines 14 cm apart; then what isn't a wall: room labels, a
 * dimension line with ticks, diagonal hatching, a bed, a stair and a filled column.
 */
function flat() {
  const p = plan(1000, 600);
  p.rect(100, 100, 900, 116);
  p.rect(100, 484, 900, 500);
  p.rect(100, 100, 116, 500);
  p.rect(884, 100, 900, 500);
  p.rect(496, 116, 504, 200);
  p.rect(496, 250, 504, 484);
  p.line(200, 400, 350, 250, 8);
  p.line(504, 300, 884, 300);
  p.line(504, 307, 884, 307);
  // Labels: 2 px strokes, letters 12 px tall, words up to 1.5 m.
  for (let i = 0; i < 12; i++) {
    p.rect(150 + i * 6, 150, 152 + i * 6, 162);
    p.rect(150 + i * 6, 150, 155 + i * 6, 152);
  }
  for (let i = 0; i < 8; i++) p.rect(620 + i * 7, 330, 622 + i * 7, 342);
  // A dimension line above the flat, with ticks.
  p.line(100, 60, 900, 60);
  for (const x of [100, 496, 900]) p.line(x, 52, x, 68);
  // Hatching: 1 px diagonals 24 px apart.
  for (let x = 600; x < 800; x += 24) p.line(x, 470, x + 100, 370);
  // A bed (1.6 m × 2 m) and a stair (1 m treads, 28 cm apart).
  p.line(620, 150, 700, 150);
  p.line(700, 150, 700, 250);
  p.line(700, 250, 620, 250);
  p.line(620, 250, 620, 150);
  for (let y = 140; y < 280; y += 14) p.line(300, y, 350, y);
  // A filled column, 1.2 m square.
  p.rect(780, 160, 840, 220);
  return p;
}

const traceAt = (pixels: Pixels, metersPerPx: number | undefined, existing: Wall[] = []) =>
  trace({
    pixels,
    width: pixels.width,
    height: pixels.height,
    metersPerPx,
    existing,
    limit: 1000,
  });

describe("trace", () => {
  it("finds the walls and leaves labels, dimensions, hatching, furniture and fills", () => {
    const walls = traceAt(flat().pixels, 0.02);
    // The shell, its corners shared on the walls' center lines.
    expect(find(walls, [108, 108], [892, 108])?.material).toBe("brick");
    expect(find(walls, [108, 492], [892, 492])?.material).toBe("brick");
    expect(find(walls, [108, 108], [108, 492])?.material).toBe("brick");
    expect(find(walls, [892, 108], [892, 492])?.material).toBe("brick");
    // The inner wall, met with the shell and open at the doorway.
    expect(find(walls, [500, 108], [500, 200])?.material).toBe("drywall");
    expect(find(walls, [500, 250], [500, 492])?.material).toBe("drywall");
    // The 45° wall and the hollow one (met with the inner wall and the shell).
    expect(find(walls, [200, 400], [350, 250], 6)?.material).toBe("drywall");
    expect(find(walls, [500, 303.5], [892, 303.5])?.material).toBe("drywall");
    expect(walls).toHaveLength(8);
  });

  it("drops what's drawn already and keeps to the limit, longest first", () => {
    const { pixels } = flat();
    const drawn: Wall[] = [
      { from: { x: 110, y: 107 }, to: { x: 890, y: 109 }, material: "concrete" },
    ];
    const walls = traceAt(pixels, 0.02, drawn);
    expect(walls).toHaveLength(7);
    expect(find(walls, [108, 108], [892, 108])).toBeUndefined();
    const two = trace({
      pixels,
      width: 1000,
      height: 600,
      metersPerPx: 0.02,
      existing: [],
      limit: 2,
    });
    expect(two.map((w) => Math.round(Math.hypot(w.to.x - w.from.x, w.to.y - w.from.y)))).toEqual([
      784, 784,
    ]);
  });

  it("maps the traced pixels back to a larger plan, and assumes a size without a scale", () => {
    const { pixels } = flat();
    // The same drawing as the reduced copy of a 2000 × 1200 plan, without a scale.
    const walls = trace({
      pixels,
      width: 2000,
      height: 1200,
      metersPerPx: undefined,
      existing: [],
      limit: 1000,
    });
    const top = walls.find((w) => near(w, [216, 216], [1784, 216], 6));
    expect(top?.thickness).toBeCloseTo(32, 0);
    expect(workSize(4096, 2048)).toEqual({ width: 1400, height: 700 });
    expect(workSize(800, 600)).toEqual({ width: 800, height: 600 });
  });

  it("traces single-line plans by their long lines", () => {
    const p = plan(600, 400);
    p.line(50, 50, 550, 50);
    p.line(550, 50, 550, 350);
    p.line(550, 350, 50, 350);
    p.line(50, 350, 50, 50);
    p.line(300, 50, 300, 350);
    const walls = traceAt(p.pixels, 0.02);
    expect(walls).toHaveLength(5);
    expect(walls.every((w) => w.material === "drywall")).toBe(true);
  });

  it("finds nothing on a blank plan or a photo-like fill", () => {
    expect(traceAt(plan(300, 200).pixels, 0.02)).toEqual([]);
    const p = plan(300, 200);
    p.rect(0, 0, 300, 200, 60);
    expect(traceAt(p.pixels, 0.02)).toEqual([]);
  });

  it("traces a full-size plan quickly", () => {
    const p = plan(1400, 1000);
    for (let i = 0; i < 6; i++) {
      p.rect(100 + i * 200, 100, 108 + i * 200, 900);
      p.rect(100, 100 + i * 150, 1300, 108 + i * 150);
    }
    for (let x = 0; x < 1400; x += 9) p.line(x, 950, x + 40, 990);
    const start = performance.now();
    const walls = traceAt(p.pixels, 0.02);
    const ms = performance.now() - start;
    expect(walls.length).toBeGreaterThanOrEqual(12);
    expect(ms).toBeLessThan(1500);
  });
});
