/** Time ranges for charts: the presets, a zoomed-in window, and how often each refreshes. */

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** The preset ranges, each ending now. */
export const PRESETS = {
  "1h": HOUR,
  "6h": 6 * HOUR,
  "24h": DAY,
  "7d": 7 * DAY,
  "30d": 30 * DAY,
  "1y": 365 * DAY,
} as const;

export type Preset = keyof typeof PRESETS;

export const presets = Object.keys(PRESETS) as Preset[];

export function isPreset(value: string | undefined): value is Preset {
  return value !== undefined && Object.hasOwn(PRESETS, value);
}

/** A preset ending now, or a fixed window (a zoom). */
export type TimeRange = { preset: Preset } | { from: number; to: number };

export interface Window {
  from: number;
  to: number;
}

/** The window a range covers at `now`. */
export function resolveRange(range: TimeRange, now: number): Window {
  return "preset" in range
    ? { from: now - PRESETS[range.preset], to: now }
    : { from: range.from, to: range.to };
}

/**
 * How often a range's data is read again, about as often as a point of it changes; `null` for
 * a fixed window, which stays as it is.
 */
export function refreshEvery(range: TimeRange): number | null {
  if (!("preset" in range)) return null;
  const span = PRESETS[range.preset];
  if (span <= HOUR) return 15_000;
  if (span <= 6 * HOUR) return 30_000;
  if (span <= DAY) return MINUTE;
  if (span <= 7 * DAY) return 5 * MINUTE;
  return 15 * MINUTE;
}
