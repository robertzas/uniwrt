import type { Metric } from "../../api/gen/Metric";
import type { Unit } from "../../api/gen/Unit";
import {
  formatBitrate,
  formatCelsius,
  formatDbm,
  formatPercent,
  formatRate,
} from "../../lib/format";
import { strings } from "../../lib/strings";
import type { ColorToken, ScaleHints } from "./data";

const decimal = new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 });
const integer = new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 });
const load = new Intl.NumberFormat(undefined, { maximumFractionDigits: 2 });

/** How a unit's values read on axes and in legends. */
export function formatUnit(unit: Unit): (value: number) => string {
  switch (unit) {
    case "percent":
      return formatPercent;
    case "load":
      return (v) => load.format(v);
    case "celsius":
      return formatCelsius;
    case "bits_per_second":
      return formatBitrate;
    case "kbps":
      return formatRate;
    case "mbps":
      return (v) => formatRate(v * 1000);
    case "dbm":
      return formatDbm;
    case "db":
      return (v) => `${integer.format(v)} dB`;
    case "count":
      return (v) => decimal.format(v);
    case "score":
    case "tq":
      return (v) => integer.format(v);
  }
}

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

export function metricLabel(metric: Metric): string {
  return strings.metrics.metric[metric];
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

/** Each metric's unit, as `Metric::unit` in the model has it. */
export const METRIC_UNITS: Record<Metric, Unit> = {
  cpu: "percent",
  memory: "percent",
  busy: "percent",
  load: "load",
  temperature: "celsius",
  rx_bps: "bits_per_second",
  tx_bps: "bits_per_second",
  down_bps: "bits_per_second",
  up_bps: "bits_per_second",
  tx_rate: "kbps",
  rx_rate: "kbps",
  capacity: "mbps",
  noise: "dbm",
  signal: "dbm",
  snr: "db",
  stations: "count",
  clients: "count",
  nodes_online: "count",
  score: "score",
  quality: "score",
  tq: "tq",
};
