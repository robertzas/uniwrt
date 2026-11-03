/**
 * The spectrum chart's geometry: the band's channels laid out left to right in segments of
 * contiguous channels (5 GHz has gaps, such as 64 → 100, that would waste the width), signals
 * bottom to top, and each network drawn as a rounded hill over the frequencies it occupies.
 */
import type { BandReport } from "../../api/gen/BandReport";
import type { Neighbor } from "../../api/gen/Neighbor";

export const WIDTH = 1000;
export const HEIGHT = 300;
/** Room for the signal labels on the left, the channel axis and DFS strip below. */
export const PAD = { left: 40, right: 8, top: 16, bottom: 46 };
export const FLOOR_DBM = -95;
export const CEIL_DBM = -25;
/** The width of the break between two segments, in pixels. */
const GAP_PX = 14;

/** A run of contiguous frequencies (MHz) and where it starts on the chart. */
interface Segment {
  low: number;
  high: number;
  x: number;
}

export interface Scale {
  /** The pixel of a frequency (MHz); one outside every segment lands at the nearest edge. */
  x: (mhz: number) => number;
  y: (dbm: number) => number;
  base: number;
  /** Where each segment ends, for the breaks between them. */
  breaks: number[];
}

/** The frequencies a channel (or network) occupies: its center ± half its width. */
export function span(center: number, width: number): [number, number] {
  return [center - width / 2, center + width / 2];
}

export function scale(band: BandReport): Scale {
  const freqs = new Set(band.channels.map((c) => c.frequency_mhz));
  for (const n of band.networks) freqs.add(n.bss.frequency_mhz);
  const sorted = [...freqs].sort((a, b) => a - b);
  const runs: { low: number; high: number }[] = [];
  for (const f of sorted) {
    const [low, high] = span(f, 20);
    const last = runs.at(-1);
    if (last && low <= last.high) last.high = Math.max(last.high, high);
    else runs.push({ low, high });
  }
  if (runs.length === 0) runs.push({ low: 0, high: 1 });
  const plot = WIDTH - PAD.left - PAD.right - GAP_PX * (runs.length - 1);
  const total = runs.reduce((sum, r) => sum + (r.high - r.low), 0);
  const perMhz = plot / total;
  const segments: Segment[] = [];
  let x = PAD.left;
  for (const run of runs) {
    segments.push({ ...run, x });
    x += (run.high - run.low) * perMhz + GAP_PX;
  }
  const base = HEIGHT - PAD.bottom;
  return {
    x: (mhz) => {
      let best = segments[0] as Segment;
      for (const s of segments) {
        if (mhz >= s.low && mhz <= s.high) return s.x + (mhz - s.low) * perMhz;
        if (mhz > s.high) best = s;
      }
      const clamped = Math.min(Math.max(mhz, best.low), best.high);
      return best.x + (clamped - best.low) * perMhz;
    },
    y: (dbm) => {
      const clamped = Math.min(Math.max(dbm, FLOOR_DBM), CEIL_DBM);
      return base - ((clamped - FLOOR_DBM) / (CEIL_DBM - FLOOR_DBM)) * (base - PAD.top);
    },
    base,
    breaks: segments.slice(0, -1).map((s) => s.x + (s.high - s.low) * perMhz + GAP_PX / 2),
  };
}

/** The hill of a network: steep flanks, a flat top at its signal. */
export function hill(network: Neighbor, s: Scale): string {
  const [low, high] = span(network.bss.center_frequency_mhz, network.bss.width);
  const x0 = s.x(low);
  const x1 = s.x(high);
  const y = s.y(network.bss.signal_dbm);
  const flank = Math.min((x1 - x0) * 0.18, 18);
  const r = (n: number) => Math.round(n * 10) / 10;
  return [
    `M${r(x0)},${s.base}`,
    `C${r(x0 + flank * 0.6)},${s.base} ${r(x0 + flank * 0.4)},${r(y)} ${r(x0 + flank)},${r(y)}`,
    `L${r(x1 - flank)},${r(y)}`,
    `C${r(x1 - flank * 0.4)},${r(y)} ${r(x1 - flank * 0.6)},${s.base} ${r(x1)},${s.base}`,
    "Z",
  ].join(" ");
}

/** The signal grid's lines, every 10 dB. */
export const GRID_DBM = [-90, -80, -70, -60, -50, -40, -30];

/** The networks to label: the site's own and the strongest others, at most `count`. */
export function labelled(networks: readonly Neighbor[], count: number): Set<string> {
  const ranked = [...networks].sort(
    (a, b) =>
      Number(b.kind === "own") - Number(a.kind === "own") || b.bss.signal_dbm - a.bss.signal_dbm,
  );
  return new Set(ranked.slice(0, count).map((n) => n.bss.bssid));
}
