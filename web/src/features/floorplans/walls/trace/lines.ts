/**
 * Straight thick strokes of a mask (M6.2c2): along 0°, 90°, 45° and 135°, the long runs of set
 * pixels on each line of the image, grouped with the overlapping runs of the lines beside them
 * into bands; a band of a wall's thickness is a wall along its middle. Strokes at other angles
 * aren't traced (plans draw walls square, now and then at 45°).
 */
import type { Bits } from "./morph";

export interface Pt {
  x: number;
  y: number;
}

/** A traced wall, in the mask's pixels: its center line and how thick it's drawn. */
export interface Piece {
  a: Pt;
  b: Pt;
  thick: number;
}

/** Lengths in the mask's pixels. */
export interface Limits {
  minLen: number;
  maxThick: number;
}

interface Run {
  u0: number;
  u1: number;
  v: number;
}

const ORIENTATIONS = [
  { dx: 1, dy: 0 },
  { dx: 0, dy: 1 },
  { dx: 1, dy: 1 },
  { dx: 1, dy: -1 },
] as const;

/** The first pixel of each line along (dx, dy), lines in order across them. */
function starts(dx: number, dy: number, w: number, h: number): Pt[] {
  const out: Pt[] = [];
  if (dy === 0) for (let y = 0; y < h; y++) out.push({ x: 0, y });
  else if (dx === 0) for (let x = 0; x < w; x++) out.push({ x, y: 0 });
  else if (dy === 1) {
    for (let x = w - 1; x > 0; x--) out.push({ x, y: 0 });
    for (let y = 0; y < h; y++) out.push({ x: 0, y });
  } else {
    for (let y = 0; y < h; y++) out.push({ x: 0, y });
    for (let x = 1; x < w; x++) out.push({ x, y: h - 1 });
  }
  return out;
}

const median = (values: number[]): number => {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[sorted.length >> 1] as number;
};

/** The walls a mask's straight strokes draw. */
export function lines(mask: Bits, limits: Limits): Piece[] {
  const { width: w, height: h, data } = mask;
  const pieces: Piece[] = [];
  for (const { dx, dy } of ORIENTATIONS) {
    const step = Math.hypot(dx, dy);
    const ux = dx / step;
    const uy = dy / step;
    // Coordinates along the line (u) and across it (v), of pixel centers.
    const u = (x: number, y: number) => (x + 0.5) * ux + (y + 0.5) * uy;
    const v = (x: number, y: number) => -(x + 0.5) * uy + (y + 0.5) * ux;
    const at = (along: number, across: number): Pt => ({
      x: along * ux - across * uy,
      y: along * uy + across * ux,
    });
    const spacing = 1 / step;
    const emit = (runs: Run[]) => {
      const thick = runs.length * spacing;
      if (thick > limits.maxThick) return;
      const u0 = median(runs.map((r) => r.u0));
      const u1 = median(runs.map((r) => r.u1));
      if (u1 - u0 < Math.max(limits.minLen, 3 * thick)) return;
      const mid = runs.reduce((sum, r) => sum + r.v, 0) / runs.length;
      pieces.push({ a: at(u0, mid), b: at(u1, mid), thick });
    };

    let open: { runs: Run[]; last: Run }[] = [];
    for (const start of starts(dx, dy, w, h)) {
      const found: Run[] = [];
      let x = start.x;
      let y = start.y;
      let first: Pt | undefined;
      let last: Pt | undefined;
      const end = () => {
        if (first && last) {
          const u0 = u(first.x, first.y) - step / 2;
          const u1 = u(last.x, last.y) + step / 2;
          if (u1 - u0 >= limits.minLen) found.push({ u0, u1, v: v(first.x, first.y) });
        }
        first = undefined;
      };
      while (x >= 0 && x < w && y >= 0 && y < h) {
        if (data[y * w + x]) {
          if (!first) first = { x, y };
          last = { x, y };
        } else end();
        x += dx;
        y += dy;
      }
      end();

      // Each band takes the run of this line overlapping its last one most.
      const next: { runs: Run[]; last: Run }[] = [];
      const taken = new Set<number>();
      for (const run of found) {
        let best = -1;
        let most = 0;
        open.forEach((band, i) => {
          if (taken.has(i)) return;
          const overlap = Math.min(run.u1, band.last.u1) - Math.max(run.u0, band.last.u0);
          const shorter = Math.min(run.u1 - run.u0, band.last.u1 - band.last.u0);
          if (overlap >= shorter / 2 && overlap > most) {
            most = overlap;
            best = i;
          }
        });
        const band = open[best];
        if (band) {
          taken.add(best);
          band.runs.push(run);
          band.last = run;
          next.push(band);
        } else next.push({ runs: [run], last: run });
      }
      open.forEach((band, i) => {
        if (!taken.has(i)) emit(band.runs);
      });
      open = next;
    }
    for (const band of open) emit(band.runs);
  }
  return pieces;
}
