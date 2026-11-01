import type { Metric } from "../api/gen/Metric";
import type { MetricsResponse } from "../api/gen/MetricsResponse";
import { METRIC_UNITS } from "../viz/timeseries/units";

/** A made-up answer of `GET v1/metrics`: three points a minute apart, each metric at `value`. */
export function metricsAnswer(url: string, value = 42): MetricsResponse {
  const query = new URL(url, "http://test").searchParams;
  const to = Number(query.get("to"));
  const metrics = (query.get("metrics") ?? "").split(",") as Metric[];
  const at = [to - 120_000, to - 60_000, to];
  return {
    entity: query.get("entity") ?? "",
    from: Number(query.get("from")),
    to,
    resolution: "raw",
    series: metrics.map((metric) => ({
      metric,
      unit: METRIC_UNITS[metric],
      at,
      avg: [value, null, value],
      min: null,
      max: null,
    })),
  };
}
