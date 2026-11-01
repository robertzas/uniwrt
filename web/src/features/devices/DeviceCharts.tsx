import { createMemo, createSignal, For, Show } from "solid-js";
import type { Metric } from "../../api/gen/Metric";
import type { NodeFacts } from "../../api/gen/NodeFacts";
import { strings } from "../../lib/strings";
import { Card, CardHeader } from "../../ui/Card";
import { ChartToolbar, exploreHref } from "../../viz/timeseries/ChartToolbar";
import { type ColorToken, SERIES_COLORS } from "../../viz/timeseries/data";
import { createMetricsFeed, createSeriesListing, type FeedQuery } from "../../viz/timeseries/feed";
import { MetricChart, type MetricLine } from "../../viz/timeseries/MetricChart";
import type { Preset } from "../../viz/timeseries/ranges";

const c = strings.device.charts;
const NODE_METRICS: readonly Metric[] = ["cpu", "memory", "load", "temperature"];
const RADIO_METRICS: readonly Metric[] = ["busy", "noise", "stations"];
const BAND_COLORS: Record<string, ColorToken> = {
  "2g": "band-2g",
  "5g": "band-5g",
  "6g": "band-6g",
};

/**
 * A node's history on its page: CPU and memory, load, temperature, one interface's throughput
 * (the WAN's by default) and its radios' utilization, noise and clients, over one range, with
 * cursors shared between the charts.
 */
export function DeviceCharts(props: { id: number; facts: NodeFacts }) {
  const [range, setRange] = createSignal<Preset>("24h");
  const [picked, setPicked] = createSignal<string>();
  const node = () => `node:${props.id}`;
  const listing = createSeriesListing(node());
  const below = (kind: "dev" | "radio") =>
    (listing() ?? [])
      .map((info) => info.entity)
      .filter((entity) => entity.startsWith(`${node()}/${kind}:`))
      .map((entity) => entity.slice(`${node()}/${kind}:`.length))
      .sort((a, b) => a.localeCompare(b));
  const devices = createMemo(() => below("dev"));
  const radios = createMemo(() => below("radio"));
  const has = (metric: Metric) =>
    (listing() ?? []).some((info) => info.entity === node() && info.metrics.includes(metric));
  const device = () => {
    const chosen = picked();
    if (chosen && devices().includes(chosen)) return chosen;
    const interfaces = props.facts.interfaces;
    const wan = interfaces.find((i) => i.up && i.ipv4_gateway !== null)?.device;
    const lan = interfaces.find((i) => i.name === "lan")?.device;
    return [wan, lan].find((d) => d && devices().includes(d)) ?? devices()[0];
  };
  const radioLine = (name: string, metric: Metric, i: number): MetricLine => {
    const band = props.facts.radios.find((r) => r.name === name)?.band;
    return {
      entity: `${node()}/radio:${name}`,
      metric,
      label: band ? c.radio(strings.device.radios.band[band], name) : name,
      color: (band && BAND_COLORS[band]) || SERIES_COLORS[i % SERIES_COLORS.length] || "accent",
    };
  };
  const feed = createMetricsFeed(() => {
    if (listing() === undefined) return undefined;
    const queries: FeedQuery[] = [{ entity: node(), metrics: NODE_METRICS }];
    const dev = device();
    if (dev) queries.push({ entity: `${node()}/dev:${dev}`, metrics: ["rx_bps", "tx_bps"] });
    for (const radio of radios())
      queries.push({ entity: `${node()}/radio:${radio}`, metrics: RADIO_METRICS });
    return { queries, range: { preset: range() }, width: 600 };
  });
  const sync = () => `device:${props.id}`;
  const panel = "min-w-0 p-4";

  return (
    <Card class="min-w-0">
      <CardHeader
        title={c.title}
        actions={
          <ChartToolbar
            range={range()}
            onRange={setRange}
            source={feed.resolution()}
            explore={exploreHref(node(), range())}
          />
        }
      />
      <Show
        when={(listing() ?? []).length > 0}
        fallback={
          <p role="status" class="px-4 pb-6 text-center text-fg-subtle text-sm">
            {listing() === undefined ? `${strings.chart.loading}…` : c.empty}
          </p>
        }
      >
        <div class="grid gap-x-6 gap-y-2 lg:grid-cols-2">
          <div class={panel}>
            <MetricChart
              title={c.cpuMemory}
              feed={feed}
              lines={[
                { entity: node(), metric: "cpu" },
                { entity: node(), metric: "memory" },
              ]}
              sync={sync()}
            />
          </div>
          <div class={panel}>
            <MetricChart
              title={c.load}
              feed={feed}
              lines={[{ entity: node(), metric: "load", color: "warn" }]}
              sync={sync()}
            />
          </div>
          <div class={panel}>
            <MetricChart
              title={c.throughput}
              controls={
                <Show
                  when={devices().length > 1}
                  fallback={<span class="font-mono text-fg-subtle text-xs">{device()}</span>}
                >
                  <label>
                    <span class="sr-only">{c.interface}</span>
                    <select
                      class="h-6 rounded-control border border-edge bg-surface px-1.5 font-mono text-fg text-xs"
                      value={device() ?? ""}
                      onChange={(event) => setPicked(event.currentTarget.value)}
                    >
                      <For each={devices()}>{(name) => <option value={name}>{name}</option>}</For>
                    </select>
                  </label>
                </Show>
              }
              feed={feed}
              lines={[
                { entity: `${node()}/dev:${device() ?? ""}`, metric: "rx_bps" },
                { entity: `${node()}/dev:${device() ?? ""}`, metric: "tx_bps" },
              ]}
              sync={sync()}
            />
          </div>
          <Show when={has("temperature")}>
            <div class={panel}>
              <MetricChart
                title={c.temperature}
                feed={feed}
                lines={[{ entity: node(), metric: "temperature", color: "crit" }]}
                sync={sync()}
              />
            </div>
          </Show>
          <Show when={radios().length > 0}>
            <div class={panel}>
              <MetricChart
                title={c.busy}
                feed={feed}
                lines={radios().map((r, i) => radioLine(r, "busy", i))}
                sync={sync()}
              />
            </div>
            <div class={panel}>
              <MetricChart
                title={c.stations}
                feed={feed}
                lines={radios().map((r, i) => radioLine(r, "stations", i))}
                sync={sync()}
              />
            </div>
            <div class={panel}>
              <MetricChart
                title={c.noise}
                feed={feed}
                lines={radios().map((r, i) => radioLine(r, "noise", i))}
                sync={sync()}
              />
            </div>
          </Show>
        </div>
      </Show>
    </Card>
  );
}
