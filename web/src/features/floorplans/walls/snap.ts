/**
 * Where a click lands when drawing walls (M6.2c): on a wall's end, on a wall's line, or along
 * 0°, 45° or 90° from the corner drawn last; the Alt key draws freely. Tolerances are in plan
 * pixels (the editor turns a fixed screen distance into them at the current zoom).
 */
import type { Wall } from "../../../api/gen/Wall";
import type { PlanPoint } from "../../../viz/floorplan/PlanCanvas";

export type SnapKind = "end" | "line" | "angle" | "none";

export interface Snapped {
  point: PlanPoint;
  kind: SnapKind;
}

/** A wall's end: the wall's index and which end. */
export interface End {
  wall: number;
  end: "from" | "to";
}

const dist = (a: PlanPoint, b: PlanPoint) => Math.hypot(a.x - b.x, a.y - b.y);

/** The point of segment a–b nearest `p`. */
export function nearestOn(p: PlanPoint, a: PlanPoint, b: PlanPoint): PlanPoint {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const len = dx * dx + dy * dy;
  const t = len === 0 ? 0 : Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / len));
  return { x: a.x + dx * t, y: a.y + dy * t };
}

/** The wall end nearest `p` within `tolerance`, skipping the ends in `skip`. */
export function nearestEnd(
  p: PlanPoint,
  walls: readonly Wall[],
  tolerance: number,
  skip: readonly End[] = [],
): PlanPoint | undefined {
  let top: PlanPoint | undefined;
  let gap = tolerance;
  walls.forEach((wall, i) => {
    for (const end of ["from", "to"] as const) {
      if (skip.some((s) => s.wall === i && s.end === end)) continue;
      const d = dist(p, wall[end]);
      if (d <= gap) {
        gap = d;
        top = wall[end];
      }
    }
  });
  return top;
}

/** The index of the wall nearest `p` within `tolerance`, skipping `skip`. */
export function hitWall(
  p: PlanPoint,
  walls: readonly Wall[],
  tolerance: number,
  skip: ReadonlySet<number> = new Set(),
): number | undefined {
  let top: number | undefined;
  let gap = tolerance;
  walls.forEach((wall, i) => {
    if (skip.has(i)) return;
    const d = dist(p, nearestOn(p, wall.from, wall.to));
    if (d <= gap) {
      gap = d;
      top = i;
    }
  });
  return top;
}

/** Where the line through `a` along `dir` meets wall `w`'s segment, if it does. */
function meet(a: PlanPoint, dir: PlanPoint, w: Wall): PlanPoint | undefined {
  const sx = w.to.x - w.from.x;
  const sy = w.to.y - w.from.y;
  const d = dir.x * sy - dir.y * sx;
  if (Math.abs(d) < 1e-9) return undefined;
  const apx = w.from.x - a.x;
  const apy = w.from.y - a.y;
  const t = (apx * sy - apy * sx) / d;
  const u = (apx * dir.y - apy * dir.x) / d;
  return u >= 0 && u <= 1 ? { x: a.x + dir.x * t, y: a.y + dir.y * t } : undefined;
}

/**
 * Where `p` snaps: a wall's end first, then (with an `anchor`, the corner drawn last) the
 * nearest 45° direction from it, meeting a wall's line where one is near; without an anchor,
 * onto a wall's line. `skip` leaves out the ends being dragged and their walls.
 */
export function snap(
  p: PlanPoint,
  anchor: PlanPoint | undefined,
  walls: readonly Wall[],
  tolerance: number,
  free: boolean,
  skip: readonly End[] = [],
): Snapped {
  if (free) return { point: p, kind: "none" };
  const end = nearestEnd(p, walls, tolerance, skip);
  if (end) return { point: end, kind: "end" };
  const moving = new Set(skip.map((s) => s.wall));
  if (anchor) {
    const dx = p.x - anchor.x;
    const dy = p.y - anchor.y;
    if (Math.hypot(dx, dy) < 1e-9) return { point: anchor, kind: "angle" };
    const angle = Math.round(Math.atan2(dy, dx) / (Math.PI / 4)) * (Math.PI / 4);
    const dir = { x: Math.cos(angle), y: Math.sin(angle) };
    const along = dx * dir.x + dy * dir.y;
    const ruled = { x: anchor.x + dir.x * along, y: anchor.y + dir.y * along };
    let best: PlanPoint | undefined;
    let gap = tolerance;
    walls.forEach((wall, i) => {
      if (moving.has(i)) return;
      const at = meet(anchor, dir, wall);
      if (at && dist(at, ruled) <= gap && dist(at, anchor) > 1e-6) {
        gap = dist(at, ruled);
        best = at;
      }
    });
    return best ? { point: best, kind: "line" } : { point: ruled, kind: "angle" };
  }
  const hit = hitWall(p, walls, tolerance, moving);
  if (hit !== undefined) {
    const wall = walls[hit] as Wall;
    return { point: nearestOn(p, wall.from, wall.to), kind: "line" };
  }
  return { point: p, kind: "none" };
}

/** The ends that sit on `point` (a corner shared by several walls), so they move together. */
export function endsAt(point: PlanPoint, walls: readonly Wall[], tolerance = 0.5): End[] {
  const out: End[] = [];
  walls.forEach((wall, i) => {
    for (const end of ["from", "to"] as const) {
      if (dist(point, wall[end]) <= tolerance) out.push({ wall: i, end });
    }
  });
  return out;
}

/**
 * `walls` with the ends in `ends` moved to `to`, dropping any wall that shrinks under 1 px
 * (unless `keep`: a preview keeps indices steady).
 */
export function moveEnds(
  walls: readonly Wall[],
  ends: readonly End[],
  to: PlanPoint,
  keep = false,
): Wall[] {
  const moved = walls.map((wall, i) => {
    let next = wall;
    for (const e of ends) if (e.wall === i) next = { ...next, [e.end]: to };
    return next;
  });
  return keep ? moved : moved.filter((wall) => dist(wall.from, wall.to) >= 1);
}
