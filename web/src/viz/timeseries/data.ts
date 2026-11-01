/**
 * The pure side of the time-series charts: lines on one time axis, the value scale, the value at a
 * point, and totals. Times are milliseconds since the epoch throughout.
 */

/** The colors a line can take: design tokens (`--color-<token>`). */
export type ColorToken =
  | "accent"
  | "info"
  | "warn"
  | "ok"
  | "crit"
  | "band-2g"
  | "band-5g"
  | "band-6g"
  | "fg-muted";

/** The order lines take colors in when nothing else decides. */
export const SERIES_COLORS: readonly ColorToken[] = [
  "accent",
  "info",
  "warn",
  "ok",
  "crit",
  "band-6g",
  "band-2g",
  "fg-muted",
];

/** One line: its own times and values, `null` where the data stops. */
export interface Line {
  label: string;
  color: ColorToken;
  at: readonly number[];
  values: readonly (number | null)[];
  /** The envelope of what each point stands for, drawn as a band; absent for single values. */
  min?: readonly (number | null)[] | null;
  max?: readonly (number | null)[] | null;
  /** Fills the area under the line. */
  area?: boolean;
}

/**
 * A value on the shared axis: a number, `null` where the line breaks, or `undefined` where the
 * line has no point of its own (another line has one), which the line spans.
 */
export type Cell = number | null | undefined;

export interface Aligned {
  times: number[];
  values: Cell[][];
  /** Per line: its envelope on the shared axis, or `null`. */
  bands: ({ min: Cell[]; max: Cell[] } | null)[];
}

/**
 * Puts lines on one time axis (their times merged), as the chart draws them: each line keeps its
 * own points and gaps, and spans the times that only other lines have.
 */
export function align(lines: readonly Line[]): Aligned {
  const all = new Set<number>();
  for (const line of lines) for (const t of line.at) all.add(t);
  const times = [...all].sort((a, b) => a - b);
  const index = new Map(times.map((t, i) => [t, i]));
  const place = (line: Line, column: readonly (number | null)[]) => {
    const out: Cell[] = new Array(times.length).fill(undefined);
    line.at.forEach((t, i) => {
      out[index.get(t) ?? 0] = column[i] ?? null;
    });
    return out;
  };
  return {
    times,
    values: lines.map((line) => place(line, line.values)),
    bands: lines.map((line) =>
      line.min && line.max ? { min: place(line, line.min), max: place(line, line.max) } : null,
    ),
  };
}

/** Whether any line has a value to draw. */
export function hasData(aligned: Aligned): boolean {
  return aligned.values.some((column) => column.some((v) => typeof v === "number"));
}

/**
 * The value a line shows at `index`: its own, or for a time it only spans, the nearest of its
 * points on either side (a break in between leaves none).
 */
export function valueAt(column: readonly Cell[], index: number): number | null {
  const own = column[index];
  if (own !== undefined) return own;
  for (let step = 1; step < column.length; step += 1) {
    const before = column[index - step];
    const after = column[index + step];
    if (typeof before === "number") return before;
    if (typeof after === "number") return after;
    if (before === null || after === null) return null;
    if (index - step < 0 && index + step >= column.length) break;
  }
  return null;
}

/** The last value of a line, for the legend when the pointer is away. */
export function latest(column: readonly Cell[]): number | null {
  for (let i = column.length - 1; i >= 0; i -= 1) {
    const value = column[i];
    if (typeof value === "number") return value;
  }
  return null;
}

export interface ScaleHints {
  /** Fixed bounds; from the data, with some room, where absent. */
  min?: number | undefined;
  max?: number | undefined;
  /** The least the scale spans, so that small wobbles don't fill the chart (20 dB of signal). */
  minSpan?: number | undefined;
}

/** The value scale for data between `low` and `high` (`null` when there is none). */
export function valueRange(
  low: number | null,
  high: number | null,
  hints: ScaleHints,
): [number, number] {
  let min = hints.min ?? low;
  let max = hints.max ?? high;
  if (min == null || max == null || !Number.isFinite(min) || !Number.isFinite(max)) {
    return [hints.min ?? 0, hints.max ?? Math.max((hints.min ?? 0) + 1, 1)];
  }
  const least = Math.max(hints.minSpan ?? 0, Math.abs(max) * 0.02, 1);
  if (max - min < least) {
    const middle = (min + max) / 2;
    min = hints.min ?? middle - least / 2;
    max = hints.max ?? Math.max(middle + least / 2, min + least);
  }
  const room = (max - min) * 0.1;
  return [hints.min ?? min - room, hints.max ?? max + room];
}

/**
 * The area under a line, in value × seconds (bits per second → bits): the trapezoids between
 * neighbouring points, none across a break.
 */
export function integrate(at: readonly number[], values: readonly (number | null)[]): number {
  let sum = 0;
  for (let i = 1; i < at.length; i += 1) {
    const a = values[i - 1];
    const b = values[i];
    const t0 = at[i - 1];
    const t1 = at[i];
    if (a == null || b == null || t0 === undefined || t1 === undefined) continue;
    sum += ((a + b) / 2) * ((t1 - t0) / 1000);
  }
  return sum;
}
