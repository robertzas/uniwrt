import { createSignal, For } from "solid-js";
import { strings } from "../../../lib/strings";
import { SELECT } from "../../../ui/FormField";
import type { Line } from "../../../viz/timeseries/data";
import { createMetricsFeed } from "../../../viz/timeseries/feed";
import { MetricChart } from "../../../viz/timeseries/MetricChart";
import { RangePicker } from "../../../viz/timeseries/RangePicker";
import type { Preset } from "../../../viz/timeseries/ranges";
import { TimeChart } from "../../../viz/timeseries/TimeChart";
import { formatUnit, unitScale } from "../../../viz/timeseries/units";
import type { ProbeRow } from "./monitor";

const t = strings.internet.monitor.chart;
const RANGES: readonly Preset[] = ["1h", "6h", "24h", "7d", "30d"];
const SYNC = "wan-probe";

/**
 * One series' stored rounds, smokeping-style: the median latency inside the band from the
 * fastest to the slowest answer, and the loss below on the same time axis.
 */
export function ProbeChart(props: {
  rows: readonly ProbeRow[];
  chosen: ProbeRow;
  onChoose: (key: string) => void;
}) {
  const [range, setRange] = createSignal<Preset>("24h");
  const entity = () => `probe:${props.chosen.key}`;
  const feed = createMetricsFeed(() => ({
    queries: [{ entity: entity(), metrics: ["rtt", "rtt_min", "rtt_max", "loss"] }],
    range: { preset: range() },
    width: 800,
  }));
  /** The median, banded by the fastest's lows and the slowest's highs where they line up. */
  const latency = (): Line[] => {
    const rtt = feed.series(entity(), "rtt");
    const low = feed.series(entity(), "rtt_min");
    const high = feed.series(entity(), "rtt_max");
    const aligned = (other: typeof low) =>
      other !== undefined && other.at.length === (rtt?.at.length ?? -1);
    return [
      {
        label: t.median,
        color: "accent",
        at: rtt?.at ?? [],
        values: rtt?.avg ?? [],
        min: low && aligned(low) ? (low.min ?? low.avg) : null,
        max: high && aligned(high) ? (high.max ?? high.avg) : null,
      },
    ];
  };
  return (
    <div class="flex flex-col gap-3">
      <div class="flex flex-wrap items-center justify-between gap-2">
        <select
          aria-label={t.series}
          class={[SELECT, "w-auto max-w-72"]}
          value={props.chosen.key}
          onChange={(event) => props.onChoose(event.currentTarget.value)}
        >
          <For each={props.rows}>{(row) => <option value={row.key}>{row.name}</option>}</For>
        </select>
        <RangePicker value={range()} onChange={setRange} options={RANGES} />
      </div>
      <TimeChart
        title={t.latency}
        lines={latency()}
        format={formatUnit("milliseconds")}
        {...unitScale("milliseconds")}
        from={feed.window()?.from}
        to={feed.window()?.to}
        loading={feed.loading()}
        sync={SYNC}
        empty={t.empty}
      />
      <MetricChart
        title={t.loss}
        feed={feed}
        lines={[{ entity: entity(), metric: "loss", color: "crit" }]}
        height={120}
        sync={SYNC}
        empty={t.empty}
      />
    </div>
  );
}
