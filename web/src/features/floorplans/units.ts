/** Lengths on floor plans: stored in meters, entered and shown in m or ft (D-117). */
import type { LengthUnit } from "../../api/gen/LengthUnit";

const M_PER_FT = 0.3048;

export function toMeters(value: number, unit: LengthUnit): number {
  return unit === "ft" ? value * M_PER_FT : value;
}

export function fromMeters(meters: number, unit: LengthUnit): number {
  return unit === "ft" ? meters / M_PER_FT : meters;
}

/** A length for people: a decimal under 10, whole numbers above. */
export function formatLength(meters: number, unit: LengthUnit): string {
  const value = fromMeters(meters, unit);
  const shown = value < 10 ? Math.round(value * 10) / 10 : Math.round(value);
  return `${shown} ${unit}`;
}

/**
 * The scale bar: the longest round length (1, 2 or 5 × 10ⁿ in `unit`) that fits in `maxPx`
 * screen pixels, given the plan's meters per pixel and the view's screen pixels per plan pixel.
 */
export function scaleBar(
  metersPerPx: number,
  zoom: number,
  unit: LengthUnit,
  maxPx = 120,
): { label: string; px: number } {
  const unitsPerScreenPx = fromMeters(metersPerPx, unit) / zoom;
  const most = maxPx * unitsPerScreenPx;
  const power = 10 ** Math.floor(Math.log10(most));
  const step = [5, 2, 1].find((factor) => factor * power <= most) ?? 1;
  const length = step * power;
  return { label: `${Number(length.toPrecision(3))} ${unit}`, px: length / unitsPerScreenPx };
}

/** The factor that brings an image within `max` pixels on its longest side, never enlarging. */
export function fitWithin(width: number, height: number, max: number): number {
  return Math.min(1, max / Math.max(width, height));
}

/** An area for people, in m² or ft², whole above 10. */
export function formatArea(squareMeters: number, unit: LengthUnit): string {
  const value = unit === "ft" ? squareMeters / (M_PER_FT * M_PER_FT) : squareMeters;
  const shown = value < 10 ? Math.round(value * 10) / 10 : Math.round(value);
  return `${shown} ${unit}²`;
}
