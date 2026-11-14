/**
 * Walls traced from a plan's drawing (M6.2c2, D-120): proposed for admins to accept or delete,
 * never stored by themselves. The plan is read at up to `WORK_PX` on its longest side; its dark
 * pixels (darker than 60 % of the background's luminance) are opened so strokes thinner than
 * `MIN_THICK_M` (text, dimension lines, hatching, furniture) go; the straight bands left, at
 * least `MIN_LEN_M` long and at most `MAX_THICK_M` thick, are walls (`lines.ts`). Walls drawn
 * as two thin lines up to `HOLLOW_M` apart are paired from the thin strokes (`tidy.ts`); then
 * pieces are joined and met at corners. A plan with neither (walls drawn as single lines) gets
 * its long thin lines instead. Without a scale the plan is taken to span `ASSUMED_M`.
 */
import type { Material } from "../../../../api/gen/Material";
import type { Wall } from "../../../../api/gen/Wall";
import type { PlanPoint } from "../../../../viz/floorplan/PlanCanvas";
import { background, type Pixels } from "../../coverage/footprint";
import { lines } from "./lines";
import { type Bits, open } from "./morph";
import { heavy, join, length, meet, pairs } from "./tidy";

/** The longest side the plan is traced at. */
export const WORK_PX = 1400;
const MIN_THICK_M = 0.06;
const MAX_THICK_M = 0.6;
/** Shorter strokes are most often a large label's letters. */
const MIN_LEN_M = 0.75;
const HOLLOW_M = 0.45;
/** Collinear pieces this close join (doorways are wider). */
const GAP_M = 0.3;
/** An end this far short of (or past) another wall's edge meets it. */
const REACH_M = 0.15;
const ASSUMED_M = 20;

/** A wall proposed from the drawing, in the plan's pixels. */
export interface TracedWall extends Wall {
  /** How thick it's drawn, in plan pixels. */
  thickness: number;
}

export interface TraceInput {
  /** The plan, read at up to `WORK_PX` on its longest side. */
  pixels: Pixels;
  /** The plan's size in its own pixels. */
  width: number;
  height: number;
  metersPerPx: number | undefined;
  /** The walls already drawn: traced ones on them aren't proposed again. */
  existing: readonly Wall[];
  /** At most this many are proposed, longest first. */
  limit: number;
}

/** The size to read a `width` × `height` plan at for tracing. */
export function workSize(width: number, height: number): { width: number; height: number } {
  const k = Math.min(1, WORK_PX / Math.max(width, height));
  return { width: Math.max(1, Math.round(width * k)), height: Math.max(1, Math.round(height * k)) };
}

function dark(p: Pixels): Bits {
  const [r, g, b] = background(p);
  const limit = 0.6 * (0.299 * r + 0.587 * g + 0.114 * b);
  const data = new Uint8Array(p.width * p.height);
  for (let i = 0; i < data.length; i++) {
    const lum = 0.299 * (p.data[i * 4] ?? 0) + 0.587 * (p.data[i * 4 + 1] ?? 0);
    data[i] = lum + 0.114 * (p.data[i * 4 + 2] ?? 0) < limit ? 1 : 0;
  }
  return { width: p.width, height: p.height, data };
}

/** Whether `t` lies along `w`: nearly parallel, on its line, and mostly within it. */
function covered(t: Wall, w: Wall, tolerance: number): boolean {
  const dx = w.to.x - w.from.x;
  const dy = w.to.y - w.from.y;
  const l = Math.hypot(dx, dy);
  const tl = Math.hypot(t.to.x - t.from.x, t.to.y - t.from.y);
  if (l === 0 || tl === 0) return false;
  const ux = dx / l;
  const uy = dy / l;
  const sin = Math.abs(ux * (t.to.y - t.from.y) - uy * (t.to.x - t.from.x)) / tl;
  if (sin > 0.09) return false;
  const off = (p: PlanPoint) => Math.abs(ux * (p.y - w.from.y) - uy * (p.x - w.from.x));
  if ((off(t.from) + off(t.to)) / 2 > tolerance) return false;
  const along = (p: PlanPoint) => ux * (p.x - w.from.x) + uy * (p.y - w.from.y);
  const a = along(t.from);
  const b = along(t.to);
  const overlap = Math.min(l, Math.max(a, b)) - Math.max(0, Math.min(a, b));
  return overlap >= tl / 2;
}

/** The walls the plan's drawing shows, longest first. */
export function trace(input: TraceInput): TracedWall[] {
  const { pixels } = input;
  const k = pixels.width / input.width;
  const ppm = input.metersPerPx
    ? k / input.metersPerPx
    : Math.max(pixels.width, pixels.height) / ASSUMED_M;
  const ink = dark(pixels);
  const r = Math.max(1, Math.round((MIN_THICK_M / 2) * ppm));
  const minLen = Math.max(8, MIN_LEN_M * ppm);
  const limits = { minLen, maxThick: MAX_THICK_M * ppm };
  // The strokes the opening removes, long enough to be a wall's face.
  const thin = lines(ink, { minLen, maxThick: 2 * r + 1 });
  let pieces = [...lines(open(ink, r), limits), ...pairs(thin, HOLLOW_M * ppm)];
  if (pieces.length === 0) pieces = thin.filter((p) => length(p) >= 2 * minLen);
  pieces = meet(join(pieces, GAP_M * ppm), REACH_M * ppm).filter((p) => length(p) >= minLen);
  const isHeavy = heavy(pieces);
  const toPlan = (p: { x: number; y: number }): PlanPoint => ({
    x: Math.round(Math.min(input.width, Math.max(0, p.x / k)) * 10) / 10,
    y: Math.round(Math.min(input.height, Math.max(0, p.y / k)) * 10) / 10,
  });
  const out: TracedWall[] = [];
  for (const p of pieces.sort((a, b) => length(b) - length(a))) {
    const material: Material = isHeavy(p) ? "brick" : "drywall";
    const wall: TracedWall = {
      from: toPlan(p.a),
      to: toPlan(p.b),
      material,
      thickness: p.thick / k,
    };
    const tolerance = Math.max(wall.thickness / 2, (REACH_M * ppm) / k);
    if (input.existing.some((w) => covered(wall, w, tolerance))) continue;
    if (Math.hypot(wall.to.x - wall.from.x, wall.to.y - wall.from.y) < 1) continue;
    out.push(wall);
    if (out.length >= input.limit) break;
  }
  return out;
}
