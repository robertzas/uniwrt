/** Number, size, duration and time formatting for the UI, in the browser's locale. */

const integer = new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 });
const decimal = new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 });

/** A placeholder for a value that is unknown. */
export const NONE = "—";

export function formatPercent(value: number | null | undefined): string {
  return value == null ? NONE : `${integer.format(value)} %`;
}

export function formatCelsius(value: number | null | undefined): string {
  return value == null ? NONE : `${integer.format(value)} °C`;
}

export function formatDbm(value: number | null | undefined): string {
  return value == null ? NONE : `${integer.format(value)} dBm`;
}

const units = ["B", "KB", "MB", "GB", "TB", "PB"];

/** Bytes in powers of 1,000, like the rest of networking: `1.4 GB`. */
export function formatBytes(bytes: number | null | undefined): string {
  if (bytes == null) return NONE;
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit += 1;
  }
  return `${(unit === 0 ? integer : decimal).format(value)} ${units[unit]}`;
}

/** A PHY rate: `866 Mb/s`, or `1.2 Gb/s` from 1,000 Mb/s. */
export function formatRate(kbps: number | null | undefined): string {
  if (kbps == null) return NONE;
  const mbps = kbps / 1000;
  if (mbps >= 1000) return `${decimal.format(mbps / 1000)} Gb/s`;
  return `${(mbps >= 10 ? integer : decimal).format(mbps)} Mb/s`;
}

/** A traffic rate in bits per second: `840 b/s`, `12 kb/s`, `3.4 Mb/s`, `1.2 Gb/s`. */
export function formatBitrate(bps: number | null | undefined): string {
  if (bps == null) return NONE;
  const steps: [number, string][] = [
    [1e9, "Gb/s"],
    [1e6, "Mb/s"],
    [1e3, "kb/s"],
  ];
  for (const [size, unit] of steps) {
    if (bps >= size) {
      const value = bps / size;
      return `${(value >= 10 ? integer : decimal).format(value)} ${unit}`;
    }
  }
  return `${integer.format(bps)} b/s`;
}

/** The two largest units of a duration: `3d 4h`, `5h 12m`, `12m 3s`, `40s`. */
export function formatDuration(seconds: number | null | undefined): string {
  if (seconds == null) return NONE;
  const s = Math.max(0, Math.floor(seconds));
  const parts: [number, string][] = [
    [Math.floor(s / 86_400), "d"],
    [Math.floor((s % 86_400) / 3600), "h"],
    [Math.floor((s % 3600) / 60), "m"],
    [s % 60, "s"],
  ];
  const first = parts.findIndex(([value]) => value > 0);
  if (first < 0) return "0s";
  return parts
    .slice(first, first + 2)
    .filter(([value]) => value > 0)
    .map(([value, unit]) => `${value}${unit}`)
    .join(" ");
}

const clock = new Intl.DateTimeFormat(undefined, { timeStyle: "medium" });

/** A time of day, for timestamps in milliseconds since the epoch. */
export function formatTime(ms: number): string {
  return clock.format(new Date(ms));
}
