import { createSignal, Show } from "solid-js";
import type { ClientSample } from "../../api/gen/ClientSample";
import { formatDbm, formatRate } from "../../lib/format";
import { strings } from "../../lib/strings";
import { Card } from "../../ui/Card";
import { ChartToolbar, exploreHref } from "../../viz/timeseries/ChartToolbar";
import { createMetricsFeed } from "../../viz/timeseries/feed";
import { MetricChart } from "../../viz/timeseries/MetricChart";
import type { Preset } from "../../viz/timeseries/ranges";
import { TimeChart } from "../../viz/timeseries/TimeChart";
import { CLIENT_RANGES, liveWindow, sampleLine } from "./history";

const s = strings.client.signal;

/**
 * The Signal tab: the client's signal and PHY rates. The last hour comes from the samples the
 * controller keeps in memory (every few seconds, live); longer ranges from the stored series.
 */
export function SignalTab(props: { mac: string; samples: readonly ClientSample[] }) {
  const [range, setRange] = createSignal<Preset>("1h");
  const entity = () => `client:${props.mac}`;
  const feed = createMetricsFeed(() =>
    range() === "1h"
      ? undefined
      : {
          queries: [{ entity: entity(), metrics: ["signal", "tx_rate", "rx_rate"] }],
          range: { preset: range() },
          width: 800,
        },
  );
  const sync = () => `client-signal:${props.mac}`;
  return (
    <div class="flex flex-col gap-4">
      <ChartToolbar
        range={range()}
        onRange={setRange}
        options={CLIENT_RANGES}
        source={range() === "1h" ? "live" : feed.resolution()}
        explore={exploreHref(entity(), range())}
      />
      <Show
        when={range() !== "1h"}
        fallback={
          <div class="grid gap-4 xl:grid-cols-2">
            <Card class="p-4 xl:col-span-2">
              <TimeChart
                title={s.title}
                lines={[sampleLine(props.samples, s.series.signal, "info", "signal_dbm")]}
                format={(v) => formatDbm(v)}
                minSpan={20}
                {...liveWindow(props.samples)}
                sync={sync()}
                empty={s.empty}
              />
            </Card>
            <Card class="p-4 xl:col-span-2">
              <TimeChart
                title={s.rates}
                lines={[
                  sampleLine(props.samples, s.series.tx, "accent", "tx_kbps"),
                  sampleLine(props.samples, s.series.rx, "warn", "rx_kbps"),
                ]}
                format={(v) => formatRate(v)}
                min={0}
                {...liveWindow(props.samples)}
                sync={sync()}
                empty={s.empty}
              />
            </Card>
          </div>
        }
      >
        <Card class="p-4">
          <MetricChart
            title={s.title}
            feed={feed}
            lines={[{ entity: entity(), metric: "signal", label: s.series.signal }]}
            sync={sync()}
            empty={s.stored}
          />
        </Card>
        <Card class="p-4">
          <MetricChart
            title={s.rates}
            feed={feed}
            lines={[
              { entity: entity(), metric: "tx_rate", label: s.series.tx },
              { entity: entity(), metric: "rx_rate", label: s.series.rx },
            ]}
            sync={sync()}
            empty={s.stored}
          />
        </Card>
      </Show>
    </div>
  );
}
