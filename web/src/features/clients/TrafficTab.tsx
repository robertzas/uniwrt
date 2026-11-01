import { createMemo, createSignal } from "solid-js";
import type { ClientSample } from "../../api/gen/ClientSample";
import { formatBitrate, formatBytes } from "../../lib/format";
import { strings } from "../../lib/strings";
import { Card } from "../../ui/Card";
import { Stat } from "../../ui/Stat";
import { ChartToolbar, exploreHref } from "../../viz/timeseries/ChartToolbar";
import { integrate, type Line } from "../../viz/timeseries/data";
import { createMetricsFeed } from "../../viz/timeseries/feed";
import type { Preset } from "../../viz/timeseries/ranges";
import { TimeChart } from "../../viz/timeseries/TimeChart";
import { CLIENT_RANGES, liveWindow, sampleLine } from "./history";

const t = strings.client.traffic;
const series = strings.client.signal.series;

/**
 * The Traffic tab: the client's download and upload over a range (the last hour live from memory,
 * longer from the stored series), and about how much it moved.
 */
export function TrafficTab(props: { mac: string; samples: readonly ClientSample[] }) {
  const [range, setRange] = createSignal<Preset>("1h");
  const entity = () => `client:${props.mac}`;
  const feed = createMetricsFeed(() =>
    range() === "1h"
      ? undefined
      : {
          queries: [{ entity: entity(), metrics: ["down_bps", "up_bps"] }],
          range: { preset: range() },
          width: 800,
        },
  );
  const lines = createMemo((): Line[] => {
    if (range() === "1h")
      return [
        sampleLine(props.samples, series.down, "ok", "down_bps", true),
        sampleLine(props.samples, series.up, "info", "up_bps", true),
      ];
    const stored = (metric: "down_bps" | "up_bps", label: string, color: "ok" | "info"): Line => {
      const s = feed.series(entity(), metric);
      return { label, color, at: s?.at ?? [], values: s?.avg ?? [], area: true };
    };
    return [stored("down_bps", series.down, "ok"), stored("up_bps", series.up, "info")];
  });
  const window = () => {
    if (range() === "1h") return liveWindow(props.samples);
    const w = feed.window();
    return w ? { from: w.from, to: w.to } : {};
  };
  const total = (line: Line | undefined) =>
    line?.values.some((v) => v != null) ? formatBytes(integrate(line.at, line.values) / 8) : "—";
  return (
    <div class="flex flex-col gap-4">
      <ChartToolbar
        range={range()}
        onRange={setRange}
        options={CLIENT_RANGES}
        source={range() === "1h" ? "live" : feed.resolution()}
        explore={exploreHref(entity(), range())}
      />
      <div class="grid grid-cols-2 gap-3 sm:max-w-md">
        <Stat label={t.downloaded} value={total(lines()[0])}>
          <span title={t.estimate}>{strings.chart.rangeNames[range()]}</span>
        </Stat>
        <Stat label={t.uploaded} value={total(lines()[1])}>
          <span title={t.estimate}>{strings.chart.rangeNames[range()]}</span>
        </Stat>
      </div>
      <Card class="p-4">
        <TimeChart
          title={t.title}
          lines={lines()}
          format={(v) => formatBitrate(v)}
          min={0}
          {...window()}
          loading={range() !== "1h" && feed.loading()}
          height={220}
          empty={range() === "1h" ? strings.client.signal.empty : t.empty}
        />
      </Card>
    </div>
  );
}
