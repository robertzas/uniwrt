/**
 * The coverage model in the browser (features/wifi.md, Coverage & floor plans; D-118): the
 * controller fits each band's path loss (`BandModel`); this places the nodes in meters the way
 * it does (`crates/rf/src/coverage/geometry.rs`) and predicts the signal from them over a floor,
 * its dead zones and where another node would cover the most of them. Walls drawn on the plans
 * take their materials' losses from every path that crosses them (M6.2c, `walls.ts`).
 */
import type { BandModel } from "../../../api/gen/BandModel";
import type { Floor } from "../../../api/gen/Floor";
import type { PlanPoint } from "../../../viz/floorplan/PlanCanvas";
import { type Losses, lossesOf, type Storey, storeys, wallLoss } from "./walls";

/** Good enough for voice and video. */
export const GOOD_DBM = -67;
/** Below this a spot is a dead zone. */
export const DEAD_DBM = -75;
/** A suggested node must hear the mesh at least this well (5 GHz). */
export const MESH_DBM = -70;
/** The grid's cells on the plan's longest side. */
export const CELLS = 160;
const DEFAULT_HEIGHT_M = 3;

/** A node sending on the band, in meters. */
export interface Source {
  node: number;
  level: number;
  x: number;
  y: number;
  z: number;
  tx: number;
}

/** The site's walls on one band: each scaled floor's, and what each material takes. */
export interface Walls {
  storeys: readonly Storey[];
  losses: Losses;
}

/** A floor's signal, row by row over cells of `cellPx` plan pixels (`NaN` where none reaches). */
export interface Grid {
  cols: number;
  rows: number;
  cellPx: number;
  dbm: Float32Array;
}

/** The plan's footprint on the grid: 1 where a cell counts. `detected` false means all count. */
export interface Mask {
  cols: number;
  rows: number;
  inside: Uint8Array;
  detected: boolean;
}

export function metersPerPx(floor: Floor): number | undefined {
  const scale = floor.scale;
  if (!scale) return undefined;
  const px = Math.hypot(scale.to.x - scale.from.x, scale.to.y - scale.from.y);
  const per = scale.length_m / px;
  return Number.isFinite(per) && per > 0 ? per : undefined;
}

/** Level `level`'s height above the ground floor: the floors between, a missing one 3 m. */
export function elevation(floors: readonly Floor[], level: number): number {
  const height = (l: number) => floors.find((f) => f.level === l)?.height_m ?? DEFAULT_HEIGHT_M;
  let z = 0;
  if (level >= 0) for (let l = 0; l < level; l++) z += height(l);
  else for (let l = level; l < 0; l++) z -= height(l);
  return z;
}

/**
 * The band's transmitters placed on scaled floors, in meters. `moving` overrides positions on
 * floor `floor` (a node being dragged).
 */
export function sources(
  model: BandModel,
  floors: readonly Floor[],
  moving?: { floor: number; at: ReadonlyMap<number, PlanPoint> },
): Source[] {
  const out: Source[] = [];
  for (const floor of floors) {
    const per = metersPerPx(floor);
    if (!per) continue;
    const z = elevation(floors, floor.level);
    for (const placement of floor.nodes) {
      const tx = model.transmitters.find((t) => t.node === placement.node);
      if (!tx) continue;
      const at = (moving?.floor === floor.id && moving.at.get(placement.node)) || placement.at;
      out.push({
        node: placement.node,
        level: floor.level,
        x: at.x * per,
        y: at.y * per,
        z,
        tx: tx.tx_power_dbm,
      });
    }
  }
  return out;
}

/** The band's walls on `floors`, or undefined where none is drawn on a scaled floor. */
export function wallsOf(model: BandModel, floors: readonly Floor[]): Walls | undefined {
  const list = storeys(floors, metersPerPx, (level) => elevation(floors, level));
  return list.length > 0 ? { storeys: list, losses: lossesOf(model) } : undefined;
}

/** The signal from `source` at a point without walls, in dBm. */
function open(model: BandModel, source: Source, x: number, y: number, z: number, level: number) {
  const d = Math.max(1, Math.hypot(source.x - x, source.y - y, source.z - z));
  return (
    source.tx -
    model.loss_at_1m_db -
    10 * model.exponent * Math.log10(d) -
    model.floor_loss_db * Math.abs(source.level - level)
  );
}

const through = (
  walls: Walls | undefined,
  s: Source,
  x: number,
  y: number,
  z: number,
  level: number,
) => (walls ? wallLoss(walls.storeys, walls.losses, s, { level, x, y, z }) : 0);

/** The signal from `source` at a point, in dBm. */
export function predict(
  model: BandModel,
  source: Source,
  x: number,
  y: number,
  z: number,
  level: number,
  walls?: Walls,
): number {
  return open(model, source, x, y, z, level) - through(walls, source, x, y, z, level);
}

/** Whether `source` reaches a point with at least `dbm`; walls are traced only when it could. */
function reaches(
  model: BandModel,
  source: Source,
  x: number,
  y: number,
  z: number,
  level: number,
  walls: Walls | undefined,
  dbm: number,
): boolean {
  const free = open(model, source, x, y, z, level);
  return free >= dbm && free - through(walls, source, x, y, z, level) >= dbm;
}

/**
 * The strongest signal at a point, or -Infinity without sources. Walls only take away, so a
 * source is traced through them only while its open-air signal beats the best so far.
 */
export function best(
  model: BandModel,
  list: readonly Source[],
  x: number,
  y: number,
  z: number,
  level: number,
  walls?: Walls,
): number {
  let top = Number.NEGATIVE_INFINITY;
  if (!walls) {
    for (const s of list) top = Math.max(top, open(model, s, x, y, z, level));
    return top;
  }
  const free = list.map((s) => ({ s, v: open(model, s, x, y, z, level) }));
  free.sort((a, b) => b.v - a.v);
  for (const { s, v } of free) {
    if (v <= top) break;
    top = Math.max(top, v - through(walls, s, x, y, z, level));
  }
  return top;
}

/** The grid's shape over a plan: `CELLS` cells on its longest side. */
export function shape(
  width: number,
  height: number,
): { cols: number; rows: number; cellPx: number } {
  const cellPx = Math.max(width, height) / CELLS;
  return { cols: Math.ceil(width / cellPx), rows: Math.ceil(height / cellPx), cellPx };
}

/** The strongest signal at each cell's center over `floor` (which must have a plan and scale). */
export function grid(
  model: BandModel,
  list: readonly Source[],
  floor: Floor,
  floors: readonly Floor[],
  walls?: Walls,
): Grid | undefined {
  const per = metersPerPx(floor);
  const image = floor.image;
  if (!per || !image) return undefined;
  const { cols, rows, cellPx } = shape(image.width_px, image.height_px);
  const z = elevation(floors, floor.level);
  const dbm = new Float32Array(cols * rows);
  for (let r = 0; r < rows; r++) {
    const y = (r + 0.5) * cellPx * per;
    for (let c = 0; c < cols; c++) {
      const v = best(model, list, (c + 0.5) * cellPx * per, y, z, floor.level, walls);
      dbm[r * cols + c] = Number.isFinite(v) ? v : Number.NaN;
    }
  }
  return { cols, rows, cellPx, dbm };
}

const counts = (mask: Mask | undefined, i: number) => !mask || mask.inside[i] === 1;

/** The footprint's area and the part of it below `DEAD_DBM`, in m². */
export function deadZones(
  g: Grid,
  mask: Mask | undefined,
  metersPerPixel: number,
): { area: number; dead: number } {
  const cell = (g.cellPx * metersPerPixel) ** 2;
  let area = 0;
  let dead = 0;
  for (let i = 0; i < g.dbm.length; i++) {
    if (!counts(mask, i)) continue;
    area += cell;
    if (!((g.dbm[i] ?? Number.NaN) >= DEAD_DBM)) dead += cell;
  }
  return { area, dead };
}

/** Where another node would turn the most dead area live, and how much (m²). */
export interface Suggestion {
  at: PlanPoint;
  gain: number;
}

/**
 * The spot on `floor` where a node sending like the band's median one revives the most dead
 * area, among spots that hear the mesh at `MESH_DBM` or better (`backhaul`: the 5 GHz model and
 * its sources; without one any spot qualifies). Candidates every 4th cell, targets every 2nd;
 * walls are traced only for targets the open air would reach.
 */
export function suggest(
  model: BandModel,
  list: readonly Source[],
  floor: Floor,
  floors: readonly Floor[],
  g: Grid,
  mask: Mask | undefined,
  backhaul?: { model: BandModel; sources: readonly Source[]; walls?: Walls },
  walls?: Walls,
): Suggestion | undefined {
  const per = metersPerPx(floor);
  if (!per || list.length === 0) return undefined;
  const z = elevation(floors, floor.level);
  const center = (i: number) => ({
    x: ((i % g.cols) + 0.5) * g.cellPx,
    y: (Math.floor(i / g.cols) + 0.5) * g.cellPx,
  });
  const targets: PlanPoint[] = [];
  for (let r = 0; r < g.rows; r += 2) {
    for (let c = 0; c < g.cols; c += 2) {
      const i = r * g.cols + c;
      if (counts(mask, i) && !((g.dbm[i] ?? Number.NaN) >= DEAD_DBM)) targets.push(center(i));
    }
  }
  if (targets.length === 0) return undefined;
  const powers = list.map((s) => s.tx).sort((a, b) => a - b);
  const tx = powers[Math.floor(powers.length / 2)] ?? 20;
  const target = (2 * g.cellPx * per) ** 2;
  let top: Suggestion | undefined;
  for (let r = 1; r < g.rows; r += 4) {
    for (let c = 1; c < g.cols; c += 4) {
      const i = r * g.cols + c;
      if (!counts(mask, i)) continue;
      const at = center(i);
      const x = at.x * per;
      const y = at.y * per;
      if (
        backhaul &&
        best(backhaul.model, backhaul.sources, x, y, z, floor.level, backhaul.walls) < MESH_DBM
      ) {
        continue;
      }
      const node: Source = { node: 0, level: floor.level, x, y, z, tx };
      let revived = 0;
      for (const t of targets) {
        if (reaches(model, node, t.x * per, t.y * per, z, floor.level, walls, DEAD_DBM)) revived++;
      }
      const gain = revived * target;
      if (revived > 0 && (!top || gain > top.gain)) top = { at, gain };
    }
  }
  return top;
}
