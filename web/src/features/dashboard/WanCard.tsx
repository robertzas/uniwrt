import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import type { NodeDetail } from "../../api/gen/NodeDetail";
import type { NodeId } from "../../api/gen/NodeId";
import type { SpeedTest } from "../../api/gen/SpeedTest";
import { api } from "../../lib/api";
import { formatBitrate, formatDuration, formatRate, NONE } from "../../lib/format";
import { strings } from "../../lib/strings";
import { Badge } from "../../ui/Badge";
import { Card, CardHeader } from "../../ui/Card";
import { ChartToolbar, exploreHref } from "../../viz/timeseries/ChartToolbar";
import type { Line } from "../../viz/timeseries/data";
import { createMetricsFeed } from "../../viz/timeseries/feed";
import type { Preset } from "../../viz/timeseries/ranges";
import { TimeChart } from "../../viz/timeseries/TimeChart";
import { type InternetHealth, wanInterfaces } from "./health";

const t = strings.dashboard.wan;
const RANGES: readonly Preset[] = ["1h", "24h", "7d"];
/** The WAN's addresses change rarely: the gateway's record is read this often, not followed. */
const DETAIL_EVERY_MS = 30_000;

/**
 * The gateway's record, read every 30 s while the page is visible. Following `node:<id>` would
 * make the engine collect the gateway more often for as long as the dashboard is open (D-041).
 * Polled into a plain signal, so a read never holds a navigation (D-048).
 */
function createGatewayDetail(id: () => NodeId | undefined) {
  const [detail, setDetail] = createSignal<NodeDetail>();
  createEffect(id, (nodeId) => {
    setDetail(undefined);
    if (nodeId === undefined) return;
    const controller = new AbortController();
    const read = () => {
      if (document.hidden) return;
      api.nodes.get(nodeId, controller.signal).then(setDetail, () => {});
    };
    read();
    const timer = setInterval(read, DETAIL_EVERY_MS);
    document.addEventListener("visibilitychange", read);
    return () => {
      controller.abort();
      clearInterval(timer);
      document.removeEventListener("visibilitychange", read);
    };
  });
  return detail;
}

/** The newest speed test's grade and rates, read once per gateway (wan.md, SQM; D-076). */
function createLatestSpeed(id: () => NodeId | undefined) {
  const [latest, setLatest] = createSignal<SpeedTest>();
  createEffect(id, (nodeId) => {
    setLatest(undefined);
    if (nodeId === undefined) return;
    const controller = new AbortController();
    api.wan.speedTests(controller.signal, 1).then(
      (view) => setLatest(view.tests?.[0]),
      () => {},
    );
    return () => controller.abort();
  });
  return latest;
}

/**
 * The internet connection: its state and what it carries now (from the live topology), the
 * site's throughput over a range (stored series), and the gateway's WAN addresses.
 */
export function WanCard(props: { gateway: NodeId | undefined; internet: InternetHealth }) {
  const [range, setRange] = createSignal<Preset>("24h");
  const detail = createGatewayDetail(() => props.gateway);
  const speed = createLatestSpeed(() => props.gateway);
  const feed = createMetricsFeed(() =>
    props.gateway === undefined
      ? undefined
      : {
          queries: [{ entity: "site", metrics: ["down_bps", "up_bps"] }],
          range: { preset: range() },
          width: 600,
        },
  );
  const lines = createMemo((): Line[] => {
    const stored = (metric: "down_bps" | "up_bps", label: string, color: "ok" | "info"): Line => {
      const series = feed.series("site", metric);
      return { label, color, at: series?.at ?? [], values: series?.avg ?? [], area: true };
    };
    return [stored("down_bps", t.download, "ok"), stored("up_bps", t.upload, "info")];
  });
  const wan = createMemo(() => wanInterfaces(detail()?.facts?.interfaces ?? []));
  const rows = createMemo((): [string, string][] => {
    const list = wan();
    const first = list[0];
    if (!first) return [];
    const join = (values: string[]) => (values.length > 0 ? values.join(", ") : NONE);
    const all = (pick: (iface: (typeof list)[number]) => string[]) => [
      ...new Set(list.flatMap(pick)),
    ];
    return [
      [t.interface, list.map((iface) => `${iface.name} · ${iface.proto}`).join(", ")],
      [t.ipv4, join(all((iface) => iface.ipv4))],
      [t.ipv6, join(all((iface) => iface.ipv6))],
      [t.prefix, join(all((iface) => iface.delegated_prefixes))],
      [
        t.gateway,
        join(all((iface) => [iface.ipv4_gateway, iface.ipv6_gateway].filter((g) => g !== null))),
      ],
      [t.dns, join(all((iface) => iface.dns_servers))],
      [t.uptime, formatDuration(first.uptime_s)],
      ...((): [string, string][] => {
        const test = speed();
        return test
          ? [
              [
                t.bufferbloat,
                t.speed(test.grade, formatRate(test.download_kbps), formatRate(test.upload_kbps)),
              ],
            ]
          : [];
      })(),
    ];
  });
  const tone = () => props.internet.tone;

  return (
    <Card class="flex min-w-0 flex-col" aria-label={t.title}>
      <CardHeader
        title={t.title}
        actions={
          <Badge tone={tone()}>{strings.dashboard.health.state[props.internet.state]}</Badge>
        }
      />
      <Show
        when={props.gateway !== undefined}
        fallback={<p class="px-4 pb-4 text-fg-subtle text-sm">{t.noGateway}</p>}
      >
        <div class="grid grid-cols-2 gap-3 px-4 pb-1">
          <Rate label={t.download} value={props.internet.down} class="text-ok" />
          <Rate label={t.upload} value={props.internet.up} class="text-info" />
        </div>
        <div class="px-4 pt-2 pb-3">
          <TimeChart
            title={t.throughput}
            controls={
              <ChartToolbar
                range={range()}
                onRange={setRange}
                options={RANGES}
                explore={exploreHref("site", range())}
              />
            }
            lines={lines()}
            format={(v) => formatBitrate(v)}
            min={0}
            from={feed.window()?.from}
            to={feed.window()?.to}
            loading={feed.loading()}
            height={150}
            empty={t.empty}
          />
        </div>
        <Show
          when={rows().length > 0}
          fallback={
            <p class="border-edge border-t px-4 py-3 text-fg-subtle text-xs">
              {detail() ? NONE : t.waiting}
            </p>
          }
        >
          <dl class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1.5 border-edge border-t px-4 py-3 text-sm">
            <For each={rows()}>
              {([label, value]) => (
                <>
                  <dt class="text-fg-subtle">{label}</dt>
                  <dd class="truncate text-right font-medium tabular-nums" title={value}>
                    {value}
                  </dd>
                </>
              )}
            </For>
          </dl>
        </Show>
      </Show>
    </Card>
  );
}

function Rate(props: { label: string; value: number | null; class: string }) {
  return (
    <div class="flex min-w-0 flex-col">
      <span class="flex items-center gap-1.5 font-medium text-fg-subtle text-xs">
        <span class={["size-2 rounded-full bg-current", props.class]} aria-hidden="true" />
        {props.label} · {t.now}
      </span>
      <span class="truncate font-semibold text-2xl tabular-nums leading-tight tracking-tight">
        {formatBitrate(props.value)}
      </span>
    </div>
  );
}
