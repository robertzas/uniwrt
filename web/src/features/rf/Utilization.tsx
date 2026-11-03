import { createSignal } from "solid-js";
import type { BandReport } from "../../api/gen/BandReport";
import { strings } from "../../lib/strings";
import { Card, CardHeader } from "../../ui/Card";
import { ChartToolbar } from "../../viz/timeseries/ChartToolbar";
import { SERIES_COLORS } from "../../viz/timeseries/data";
import { createMetricsFeed } from "../../viz/timeseries/feed";
import { MetricChart, type MetricLine } from "../../viz/timeseries/MetricChart";
import type { Preset } from "../../viz/timeseries/ranges";

const t = strings.rf.utilization;

/** How busy each of the band's radios' channels was over a range (the metric `busy`). */
export function Utilization(props: { band: BandReport; nodeName: (id: number) => string }) {
  const [range, setRange] = createSignal<Preset>("24h");
  const entities = () => props.band.radios.map((r) => `node:${r.node}/radio:${r.radio}`);
  const feed = createMetricsFeed(() => ({
    queries: entities().map((entity) => ({ entity, metrics: ["busy" as const] })),
    range: { preset: range() },
    width: 600,
  }));
  const lines = (): MetricLine[] =>
    props.band.radios.map((r, i) => ({
      entity: `node:${r.node}/radio:${r.radio}`,
      metric: "busy",
      label: `${props.nodeName(r.node)} · ${r.radio}`,
      color: SERIES_COLORS[i % SERIES_COLORS.length] ?? "accent",
    }));
  return (
    <Card class="min-w-0" aria-label={t.title}>
      <CardHeader title={t.title} description={t.description} />
      <div class="min-w-0 px-4 pb-4">
        <MetricChart
          title={t.chart}
          controls={<ChartToolbar range={range()} onRange={setRange} source={feed.resolution()} />}
          feed={feed}
          lines={lines()}
          height={200}
          empty={t.empty}
        />
      </div>
    </Card>
  );
}
