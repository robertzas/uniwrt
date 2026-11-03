/**
 * Metrics in words: each metric's name and unit, and how a unit's values read. Shared by the
 * charts (`viz/timeseries`) and the alerts.
 */
import type { Metric } from "../api/gen/Metric";
import type { Unit } from "../api/gen/Unit";
import { formatBitrate, formatCelsius, formatDbm, formatPercent, formatRate } from "./format";
import { strings } from "./strings";

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

export function metricLabel(metric: Metric): string {
  return strings.metrics.metric[metric];
}

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

/** A metric's value with its unit: `92 %`, `−71 dBm`, `48 Mbit/s`. */
export function formatMetric(metric: Metric, value: number): string {
  return formatUnit(METRIC_UNITS[metric])(value);
}
