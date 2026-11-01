import type { JSX } from "@solidjs/web";
import type { Metric } from "../../api/gen/Metric";
import { type ColorToken, type Line, SERIES_COLORS } from "./data";
import type { MetricsFeed } from "./feed";
import { TimeChart } from "./TimeChart";
import { formatUnit, isWhole, METRIC_COLORS, METRIC_UNITS, metricLabel, unitScale } from "./units";

/** A line of a stored series: an entity's metric, its label and color where not the usual. */
export interface MetricLine {
  entity: string;
  metric: Metric;
  label?: string;
  color?: ColorToken;
}

/**
 * Stored series from a feed on one chart, which shares the first metric's unit (its format and
 * scale): bands where points stand for several values, the window the feed read.
 */
export function MetricChart(props: {
  title: string;
  controls?: JSX.Element;
  feed: MetricsFeed;
  lines: readonly MetricLine[];
  height?: number;
  sync?: string | undefined;
  onZoom?: ((from: number, to: number) => void) | undefined;
  empty?: string | undefined;
}) {
  const unit = () => METRIC_UNITS[props.lines[0]?.metric ?? "cpu"];
  const lines = (): Line[] =>
    props.lines.map((line, i) => {
      const series = props.feed.series(line.entity, line.metric);
      return {
        label: line.label ?? metricLabel(line.metric),
        color:
          line.color ??
          METRIC_COLORS[line.metric] ??
          SERIES_COLORS[i % SERIES_COLORS.length] ??
          "accent",
        at: series?.at ?? [],
        values: series?.avg ?? [],
        min: series?.min ?? null,
        max: series?.max ?? null,
      };
    });
  return (
    <TimeChart
      title={props.title}
      controls={props.controls}
      lines={lines()}
      format={formatUnit(unit())}
      {...unitScale(unit())}
      whole={isWhole(unit())}
      from={props.feed.window()?.from}
      to={props.feed.window()?.to}
      loading={props.feed.loading()}
      sync={props.sync}
      onZoom={props.onZoom}
      empty={props.empty}
      height={props.height}
    />
  );
}
