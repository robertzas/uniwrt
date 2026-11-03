import type { Metric } from "../../api/gen/Metric";
import type { Unit } from "../../api/gen/Unit";
import type { ColorToken, ScaleHints } from "./data";

export { formatMetric, formatUnit, METRIC_UNITS, metricLabel } from "../../lib/metrics";

/** The value scale a unit suits: percentages from 0 to 100, rates from 0, 20 dB of signal. */
export function unitScale(unit: Unit): ScaleHints {
  switch (unit) {
    case "percent":
    case "score":
      return { min: 0, max: 100 };
    case "tq":
      return { min: 0, max: 255 };
    case "dbm":
    case "db":
      return { minSpan: 20 };
    case "celsius":
      return { minSpan: 10 };
    case "load":
    case "count":
      return { min: 0, minSpan: 1 };
    default:
      return { min: 0 };
  }
}

/** Whether a unit counts things, so that its axis keeps to whole steps. */
export function isWhole(unit: Unit): boolean {
  return unit === "count" || unit === "score" || unit === "tq";
}

/** A metric's own color where it has a usual one (download green, upload blue, as elsewhere). */
export const METRIC_COLORS: Partial<Record<Metric, ColorToken>> = {
  cpu: "accent",
  memory: "info",
  down_bps: "ok",
  up_bps: "info",
  rx_bps: "ok",
  tx_bps: "info",
  tx_rate: "accent",
  rx_rate: "warn",
  signal: "info",
  snr: "accent",
};
