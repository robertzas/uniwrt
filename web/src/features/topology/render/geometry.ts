import type { TopoEdge } from "../../../api/gen/TopoEdge";
import type { Point } from "../layout/tree";
import type { Scene } from "../scene";

/** World to screen: `screen = world × k + (x, y)`, in CSS pixels. */
export interface Camera {
  x: number;
  y: number;
  k: number;
}

export const MIN_ZOOM = 0.15;
export const MAX_ZOOM = 4;

export function toScreen(camera: Camera, point: Point): Point {
  return { x: point.x * camera.k + camera.x, y: point.y * camera.k + camera.y };
}

export function toWorld(camera: Camera, point: Point): Point {
  return { x: (point.x - camera.x) / camera.k, y: (point.y - camera.y) / camera.k };
}

/** Zooms by `factor` around a screen point, which stays where it is. */
export function zoomAt(camera: Camera, factor: number, at: Point): Camera {
  const k = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, camera.k * factor));
  const ratio = k / camera.k;
  return { k, x: at.x - (at.x - camera.x) * ratio, y: at.y - (at.y - camera.y) * ratio };
}

/** Room kept clear at each side of the view, in screen pixels (for overlays). */
export interface Insets {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

/** Clear of the hint above the map and the replay bar below it. */
export const DEFAULT_INSETS: Insets = { top: 64, right: 56, bottom: 96, left: 56 };

/** The camera that shows every point within a `width` × `height` view, at most at 1.5×. */
export function fitCamera(
  points: Iterable<Point>,
  width: number,
  height: number,
  insets: Insets = DEFAULT_INSETS,
): Camera {
  let [minX, minY, maxX, maxY] = [Infinity, Infinity, -Infinity, -Infinity];
  for (const point of points) {
    minX = Math.min(minX, point.x);
    minY = Math.min(minY, point.y);
    maxX = Math.max(maxX, point.x);
    maxY = Math.max(maxY, point.y);
  }
  const innerWidth = Math.max(1, width - insets.left - insets.right);
  const innerHeight = Math.max(1, height - insets.top - insets.bottom);
  const centerX = insets.left + innerWidth / 2;
  const centerY = insets.top + innerHeight / 2;
  if (!Number.isFinite(minX)) return { x: centerX, y: centerY, k: 1 };
  const spanX = Math.max(maxX - minX, 1);
  const spanY = Math.max(maxY - minY, 1);
  const k = Math.min(1.5, Math.max(MIN_ZOOM, Math.min(innerWidth / spanX, innerHeight / spanY)));
  return {
    k,
    x: centerX - ((minX + maxX) / 2) * k,
    y: centerY - ((minY + maxY) / 2) * k,
  };
}

/** The distance from `p` to the segment `a`–`b`. */
export function segmentDistance(p: Point, a: Point, b: Point): number {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const length = dx * dx + dy * dy;
  const t =
    length === 0 ? 0 : Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / length));
  return Math.hypot(p.x - (a.x + t * dx), p.y - (a.y + t * dy));
}

/** Screen pixels around a vertex, and beside a link, that still hit it. */
const VERTEX_SLOP = 4;
const LINK_SLOP = 5;

/**
 * What is at a screen point: the topmost vertex (the last drawn), else the nearest link within
 * reach, else nothing.
 */
export function hitTest(
  scene: Scene,
  positions: ReadonlyMap<string, Point>,
  camera: Camera,
  at: Point,
): { kind: "vertex" | "link"; id: string } | undefined {
  for (let i = scene.vertices.length - 1; i >= 0; i--) {
    const vertex = scene.vertices[i];
    const position = vertex && positions.get(vertex.id);
    if (!vertex || !position) continue;
    const screen = toScreen(camera, position);
    const reach = Math.max(vertex.radius * camera.k, 6) + VERTEX_SLOP;
    if (Math.hypot(at.x - screen.x, at.y - screen.y) <= reach)
      return { kind: "vertex", id: vertex.id };
  }
  let best: { id: string; distance: number } | undefined;
  for (const link of scene.links) {
    const a = positions.get(link.source);
    const b = positions.get(link.target);
    if (!a || !b || !link.edge) continue;
    const distance = segmentDistance(at, toScreen(camera, a), toScreen(camera, b));
    if (distance <= LINK_SLOP && (!best || distance < best.distance))
      best = { id: link.id, distance };
  }
  return best && { kind: "link", id: best.id };
}

/** The link's traffic, both ways, in bits per second. */
export function throughput(edge: TopoEdge | null): number {
  const traffic = edge?.traffic;
  return (traffic?.down_bps ?? 0) + (traffic?.up_bps ?? 0);
}

/** A link's width in screen pixels: 1.5 idle, growing with the log of its traffic up to 9. */
export function linkWidth(edge: TopoEdge | null): number {
  const bps = throughput(edge);
  if (bps <= 0) return 1.5;
  return Math.min(9, 1.5 + 1.6 * Math.log10(1 + bps / 50_000));
}

/**
 * Flow particles on one direction of a link: more and faster with more traffic (from 1 kb/s),
 * `speed` in screen pixels per second.
 */
export function flowOf(
  bps: number | null | undefined,
): { count: number; speed: number } | undefined {
  if (!bps || bps < 1_000) return undefined;
  const scale = Math.log10(bps / 1_000);
  return { count: Math.min(6, 1 + Math.floor(scale)), speed: 28 + 16 * scale };
}

/** Where along a link (0–1) particle `i` of `count` is at `seconds`, `speed` links per second. */
export function particleAt(seconds: number, speed: number, i: number, count: number): number {
  const t = (seconds * speed + i / count) % 1;
  return t < 0 ? t + 1 : t;
}
