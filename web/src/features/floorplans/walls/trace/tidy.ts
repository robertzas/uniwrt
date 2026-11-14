/**
 * Traced pieces made into walls (M6.2c2): collinear pieces a label or a short gap split joined,
 * ends that meet another wall moved onto its center line (so corners are shared and drag
 * together), walls drawn as two thin lines found, and the heavier walls told apart by how thick
 * they're drawn.
 */
import type { Piece, Pt } from "./lines";

const sub = (a: Pt, b: Pt): Pt => ({ x: a.x - b.x, y: a.y - b.y });
const dot = (a: Pt, b: Pt) => a.x * b.x + a.y * b.y;
const cross = (a: Pt, b: Pt) => a.x * b.y - a.y * b.x;
export const length = (p: Piece) => Math.hypot(p.b.x - p.a.x, p.b.y - p.a.y);
const unit = (p: Piece): Pt => {
  const l = length(p) || 1;
  return { x: (p.b.x - p.a.x) / l, y: (p.b.y - p.a.y) / l };
};
/** Directions within about 2° of each other. */
const PARALLEL = 0.035;

/** Collinear pieces less than `gap` apart (or overlapping) joined into one, until none are. */
export function join(pieces: readonly Piece[], gap: number): Piece[] {
  const out = pieces.map((p) => ({ ...p }));
  let joined = true;
  while (joined) {
    joined = false;
    for (let i = 0; i < out.length && !joined; i++) {
      for (let j = i + 1; j < out.length && !joined; j++) {
        const [long, short] =
          length(out[i] as Piece) >= length(out[j] as Piece)
            ? [out[i] as Piece, out[j] as Piece]
            : [out[j] as Piece, out[i] as Piece];
        const d = unit(long);
        if (Math.abs(cross(d, unit(short))) > PARALLEL) continue;
        const mid = { x: (short.a.x + short.b.x) / 2, y: (short.a.y + short.b.y) / 2 };
        if (Math.abs(cross(d, sub(mid, long.a))) > Math.max(1, long.thick / 2, short.thick / 2)) {
          continue;
        }
        const l = length(long);
        const s0 = dot(d, sub(short.a, long.a));
        const s1 = dot(d, sub(short.b, long.a));
        const lo = Math.min(s0, s1);
        const hi = Math.max(s0, s1);
        if (lo - l > gap || -hi > gap) continue;
        const from = Math.min(0, lo);
        const to = Math.max(l, hi);
        const ls = length(short);
        const merged: Piece = {
          a: { x: long.a.x + d.x * from, y: long.a.y + d.y * from },
          b: { x: long.a.x + d.x * to, y: long.a.y + d.y * to },
          thick: (long.thick * l + short.thick * ls) / (l + ls),
        };
        out[i] = merged;
        out.splice(j, 1);
        joined = true;
      }
    }
  }
  return out;
}

/** Where the lines through `p` and `q` cross; undefined for near-parallel ones. */
function crossing(p: Piece, q: Piece): Pt | undefined {
  const d = sub(p.b, p.a);
  const e = sub(q.b, q.a);
  const den = cross(d, e);
  if (Math.abs(den) < 0.3 * length(p) * length(q)) return undefined;
  const t = cross(sub(q.a, p.a), e) / den;
  return { x: p.a.x + d.x * t, y: p.a.y + d.y * t };
}

/**
 * Each end within `reach` (beyond both half thicknesses) of where its line crosses another
 * wall's center line, within that wall's length, moved onto that crossing; the nearest wins.
 */
export function meet(pieces: readonly Piece[], reach: number): Piece[] {
  return pieces.map((p, i) => {
    const moved = { ...p };
    for (const end of ["a", "b"] as const) {
      let best: Pt | undefined;
      let gap = Number.POSITIVE_INFINITY;
      pieces.forEach((q, j) => {
        if (i === j) return;
        // The same pair always crosses at the same point, so shared corners match exactly.
        const x = i < j ? crossing(p, q) : crossing(q, p);
        if (!x) return;
        const d = Math.hypot(x.x - p[end].x, x.y - p[end].y);
        if (d > q.thick / 2 + p.thick / 2 + reach || d >= gap) return;
        const along = dot(unit(q), sub(x, q.a));
        const ext = p.thick / 2 + reach;
        if (along < -ext || along > length(q) + ext) return;
        const other = end === "a" ? p.b : p.a;
        if (Math.hypot(x.x - other.x, x.y - other.y) < p.thick) return;
        best = x;
        gap = d;
      });
      if (best) moved[end] = best;
    }
    return moved;
  });
}

/**
 * Whether `p` is drawn heavy: at least half again as thick as the thinnest quarter of the walls
 * (by length), so plans that draw every wall alike have none.
 */
export function heavy(pieces: readonly Piece[]): (p: Piece) => boolean {
  const sorted = [...pieces].sort((a, b) => a.thick - b.thick);
  const total = sorted.reduce((sum, p) => sum + length(p), 0);
  let seen = 0;
  let middle = 0;
  for (const p of sorted) {
    seen += length(p);
    middle = p.thick;
    if (seen >= total / 4) break;
  }
  return (p) => p.thick >= 1.5 * middle;
}

/**
 * Walls drawn as two thin parallel lines (`gap` apart at most, both at least `minLen` long and
 * overlapping by half the shorter): only where each line's sole neighbors within `gap` are on one
 * side at one offset, and the pair are each other's. Lines with neighbors on both sides or at
 * several offsets are hatching, stairs or a window's panes. The wall spans both lines.
 */
export function pairs(thin: readonly Piece[], gap: number): Piece[] {
  const near = thin.map((p) => {
    const d = unit(p);
    const l = length(p);
    const found: { j: number; off: number }[] = [];
    thin.forEach((q, j) => {
      if (q === p || Math.abs(cross(d, unit(q))) > PARALLEL) return;
      const mid = { x: (q.a.x + q.b.x) / 2, y: (q.a.y + q.b.y) / 2 };
      const off = cross(d, sub(mid, p.a));
      if (Math.abs(off) < Math.max(2, p.thick) || Math.abs(off) > gap) return;
      const s0 = dot(d, sub(q.a, p.a));
      const s1 = dot(d, sub(q.b, p.a));
      const overlap = Math.min(l, Math.max(s0, s1)) - Math.max(0, Math.min(s0, s1));
      if (overlap >= Math.min(l, length(q)) / 2) found.push({ j, off });
    });
    // One offset (within the lines' own width), on one side.
    const first = found[0];
    const alone =
      first !== undefined &&
      found.every((f) => Math.abs(f.off - first.off) <= Math.max(2, p.thick));
    return alone ? found.map((f) => f.j) : undefined;
  });
  const out: Piece[] = [];
  near.forEach((partners, i) => {
    const p = thin[i] as Piece;
    for (const j of partners ?? []) {
      if (j < i || !near[j]?.includes(i)) continue;
      const q = thin[j] as Piece;
      const d = unit(p);
      const off = cross(d, sub({ x: (q.a.x + q.b.x) / 2, y: (q.a.y + q.b.y) / 2 }, p.a));
      const s0 = dot(d, sub(q.a, p.a));
      const s1 = dot(d, sub(q.b, p.a));
      const from = Math.min(0, s0, s1);
      const to = Math.max(length(p), s0, s1);
      const n = { x: -d.y * (off / 2), y: d.x * (off / 2) };
      out.push({
        a: { x: p.a.x + d.x * from + n.x, y: p.a.y + d.y * from + n.y },
        b: { x: p.a.x + d.x * to + n.x, y: p.a.y + d.y * to + n.y },
        thick: Math.abs(off) + (p.thick + q.thick) / 2,
      });
    }
  });
  return out;
}
