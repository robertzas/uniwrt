/**
 * What the walls a signal crosses take from it (M6.2c), counted as the controller counts them
 * (`crates/rf/src/coverage/walls.rs`): a path within a floor crosses that floor's walls; a path
 * between floors crosses each storey's walls on the part of it within the storey's height, both
 * ends `ANTENNA_M` above their floors. Walls sit in a grid of buckets, so a path tests only the
 * walls near it: the heatmap follows a dragged device live with hundreds of walls.
 */
import type { BandModel } from "../../../api/gen/BandModel";
import type { Floor } from "../../../api/gen/Floor";
import type { Material } from "../../../api/gen/Material";

/** How high above its floor a path ends, in meters. */
export const ANTENNA_M = 1;
/** The index's buckets on its longest side, at most. */
const BUCKETS = 48;
const EPS = 1e-9;

/** A point of a path, in meters on its floor's level. */
export interface At {
  level: number;
  x: number;
  y: number;
  z: number;
}

/** What crossing each material once takes on the band, in dB. */
export type Losses = ReadonlyMap<Material, number>;

export function lossesOf(model: BandModel): Losses {
  return new Map(model.wall_losses.map((w) => [w.material, w.loss_db]));
}

/** Whether the segments p–q and a–b meet, ends included (parallel ones never do). */
export function crosses(
  px: number,
  py: number,
  qx: number,
  qy: number,
  ax: number,
  ay: number,
  bx: number,
  by: number,
): boolean {
  const rx = qx - px;
  const ry = qy - py;
  const sx = bx - ax;
  const sy = by - ay;
  const d = rx * sy - ry * sx;
  if (Math.abs(d) < 1e-12) return false;
  const apx = ax - px;
  const apy = ay - py;
  const t = (apx * sy - apy * sx) / d;
  const u = (apx * ry - apy * rx) / d;
  return t >= 0 && t <= 1 && u >= 0 && u <= 1;
}

/** One floor's walls in meters, bucketed. */
export class WallIndex {
  /** Each wall's ends, four numbers apiece. */
  private readonly ends: Float64Array;
  private readonly materials: Material[];
  private readonly buckets: number[][];
  private readonly seen: Uint32Array;
  private stamp = 0;
  private readonly minX: number;
  private readonly minY: number;
  private readonly maxX: number;
  private readonly maxY: number;
  private readonly size: number;
  private readonly cols: number;
  private readonly rows: number;

  constructor(
    walls: readonly { ax: number; ay: number; bx: number; by: number; material: Material }[],
  ) {
    this.ends = new Float64Array(walls.length * 4);
    this.materials = walls.map((w) => w.material);
    this.seen = new Uint32Array(walls.length);
    let minX = Number.POSITIVE_INFINITY;
    let minY = Number.POSITIVE_INFINITY;
    let maxX = Number.NEGATIVE_INFINITY;
    let maxY = Number.NEGATIVE_INFINITY;
    walls.forEach((w, i) => {
      this.ends.set([w.ax, w.ay, w.bx, w.by], i * 4);
      minX = Math.min(minX, w.ax, w.bx);
      minY = Math.min(minY, w.ay, w.by);
      maxX = Math.max(maxX, w.ax, w.bx);
      maxY = Math.max(maxY, w.ay, w.by);
    });
    const span = Math.max(maxX - minX, maxY - minY, 1e-3);
    this.size = Math.max(span / BUCKETS, 0.25);
    // A margin keeps walls on the box's edge inside it.
    this.minX = minX - this.size / 2;
    this.minY = minY - this.size / 2;
    this.cols = Math.max(1, Math.ceil((maxX - this.minX) / this.size + 0.5));
    this.rows = Math.max(1, Math.ceil((maxY - this.minY) / this.size + 0.5));
    this.maxX = this.minX + this.cols * this.size;
    this.maxY = this.minY + this.rows * this.size;
    this.buckets = Array.from({ length: this.cols * this.rows }, () => []);
    walls.forEach((w, i) => {
      // The wall's box, padded: a path through a bucket's corner still meets it.
      const pad = this.size * 1e-6;
      const c0 = this.col(Math.min(w.ax, w.bx) - pad);
      const c1 = this.col(Math.max(w.ax, w.bx) + pad);
      const r0 = this.row(Math.min(w.ay, w.by) - pad);
      const r1 = this.row(Math.max(w.ay, w.by) + pad);
      for (let r = r0; r <= r1; r++) {
        for (let c = c0; c <= c1; c++) this.buckets[r * this.cols + c]?.push(i);
      }
    });
  }

  get length(): number {
    return this.materials.length;
  }

  private col(x: number): number {
    return Math.min(this.cols - 1, Math.max(0, Math.floor((x - this.minX) / this.size)));
  }

  private row(y: number): number {
    return Math.min(this.rows - 1, Math.max(0, Math.floor((y - this.minY) / this.size)));
  }

  /** The loss from the walls the segment p–q crosses, in dB. */
  loss(px: number, py: number, qx: number, qy: number, losses: Losses): number {
    if (this.materials.length === 0) return 0;
    // Clip the segment to the index's box (Liang–Barsky).
    const dx = qx - px;
    const dy = qy - py;
    let t0 = 0;
    let t1 = 1;
    const edges: [number, number][] = [
      [-dx, px - this.minX],
      [dx, this.maxX - px],
      [-dy, py - this.minY],
      [dy, this.maxY - py],
    ];
    for (const [p, q] of edges) {
      if (Math.abs(p) < EPS) {
        if (q < 0) return 0;
      } else {
        const t = q / p;
        if (p < 0) t0 = Math.max(t0, t);
        else t1 = Math.min(t1, t);
      }
    }
    if (t0 > t1) return 0;
    const x0 = px + dx * t0;
    const y0 = py + dy * t0;
    const x1 = px + dx * t1;
    const y1 = py + dy * t1;

    this.stamp = (this.stamp + 1) >>> 0;
    if (this.stamp === 0) {
      this.seen.fill(0);
      this.stamp = 1;
    }
    let total = 0;
    const visit = (bucket: readonly number[]) => {
      for (const i of bucket) {
        if (this.seen[i] === this.stamp) continue;
        this.seen[i] = this.stamp;
        const o = i * 4;
        const e = this.ends;
        if (
          crosses(
            px,
            py,
            qx,
            qy,
            e[o] as number,
            e[o + 1] as number,
            e[o + 2] as number,
            e[o + 3] as number,
          )
        ) {
          total += losses.get(this.materials[i] as Material) ?? 0;
        }
      }
    };
    // Walk the buckets the segment passes (Amanatides–Woo).
    let c = this.col(x0);
    let r = this.row(y0);
    const endC = this.col(x1);
    const endR = this.row(y1);
    const sx = x1 > x0 ? 1 : -1;
    const sy = y1 > y0 ? 1 : -1;
    const ddx = x1 - x0;
    const ddy = y1 - y0;
    const nextX = (cc: number) => this.minX + (sx > 0 ? cc + 1 : cc) * this.size;
    const nextY = (rr: number) => this.minY + (sy > 0 ? rr + 1 : rr) * this.size;
    let tMaxX = Math.abs(ddx) < EPS ? Number.POSITIVE_INFINITY : (nextX(c) - x0) / ddx;
    let tMaxY = Math.abs(ddy) < EPS ? Number.POSITIVE_INFINITY : (nextY(r) - y0) / ddy;
    const tDx = Math.abs(ddx) < EPS ? Number.POSITIVE_INFINITY : this.size / Math.abs(ddx);
    const tDy = Math.abs(ddy) < EPS ? Number.POSITIVE_INFINITY : this.size / Math.abs(ddy);
    for (let steps = this.cols + this.rows + 2; steps > 0; steps--) {
      visit(this.buckets[r * this.cols + c] as number[]);
      if (c === endC && r === endR) break;
      if (tMaxX < tMaxY) {
        c += sx;
        tMaxX += tDx;
      } else {
        r += sy;
        tMaxY += tDy;
      }
      if (c < 0 || r < 0 || c >= this.cols || r >= this.rows) break;
    }
    return total;
  }
}

/** A scaled floor's walls between its floor and the next one's. */
export interface Storey {
  level: number;
  bottom: number;
  top: number;
  index: WallIndex;
}

/**
 * Every scaled floor with walls, given each floor's meters per pixel and elevation (the model's
 * own placing of floors).
 */
export function storeys(
  floors: readonly Floor[],
  metersPerPx: (floor: Floor) => number | undefined,
  elevation: (level: number) => number,
): Storey[] {
  const out: Storey[] = [];
  for (const floor of floors) {
    const per = floor.walls.length > 0 ? metersPerPx(floor) : undefined;
    if (!per) continue;
    const bottom = elevation(floor.level);
    out.push({
      level: floor.level,
      bottom,
      top: bottom + floor.height_m,
      index: new WallIndex(
        floor.walls.map((w) => ({
          ax: w.from.x * per,
          ay: w.from.y * per,
          bx: w.to.x * per,
          by: w.to.y * per,
          material: w.material,
        })),
      ),
    });
  }
  return out;
}

/** The loss from the walls between `from` and `to`, in dB. */
export function wallLoss(list: readonly Storey[], losses: Losses, from: At, to: At): number {
  if (list.length === 0) return 0;
  const low = Math.min(from.level, to.level);
  const high = Math.max(from.level, to.level);
  const dz = to.z - from.z;
  let total = 0;
  for (const storey of list) {
    if (storey.level < low || storey.level > high) continue;
    let t0 = 0;
    let t1 = 1;
    if (from.level !== to.level && Math.abs(dz) >= EPS) {
      const start = from.z + ANTENNA_M;
      const a = (storey.bottom - start) / dz;
      const b = (storey.top - start) / dz;
      t0 = Math.max(0, Math.min(a, b));
      t1 = Math.min(1, Math.max(a, b));
    }
    if (t1 < t0) continue;
    const dx = to.x - from.x;
    const dy = to.y - from.y;
    total += storey.index.loss(
      from.x + dx * t0,
      from.y + dy * t0,
      from.x + dx * t1,
      from.y + dy * t1,
      losses,
    );
  }
  return total;
}
