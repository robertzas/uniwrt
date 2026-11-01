import type { JSX } from "@solidjs/web";
import { For, Show, useContext } from "solid-js";
import type { ClientDetail } from "../../api/gen/ClientDetail";
import type { ClientSample } from "../../api/gen/ClientSample";
import type { ExperiencePart } from "../../api/gen/ExperiencePart";
import { scoreTone, upstreamName } from "../../lib/clients";
import {
  formatBitrate,
  formatDateTime,
  formatDbm,
  formatDuration,
  formatPercent,
  formatRate,
  NONE,
} from "../../lib/format";
import { LiveContext } from "../../lib/live";
import { strings } from "../../lib/strings";
import { Badge } from "../../ui/Badge";
import { Card, CardHeader } from "../../ui/Card";
import { Sparkline } from "../../ui/Sparkline";
import { Stat } from "../../ui/Stat";
import { createNow } from "./columns";

const d = strings.client;
const radios = strings.device.radios;

/** The Overview tab: headline numbers with their last hour, then connection, score and identity. */
export function ClientOverview(props: { detail: ClientDetail }) {
  const live = useContext(LiveContext);
  const now = createNow();
  const nodeName = (id: number) =>
    live.fleet.nodes.find((node) => node.id === id)?.name ?? `#${id}`;
  return (
    <div class="flex flex-col gap-4">
      <Stats detail={props.detail} now={now()} />
      <div class="grid gap-4 lg:grid-cols-3">
        <Connection detail={props.detail} nodeName={nodeName} />
        <Experience detail={props.detail} />
        <Details detail={props.detail} />
      </div>
    </div>
  );
}

function Stats(props: { detail: ClientDetail; now: number }) {
  const s = d.stat;
  const client = () => props.detail.client;
  const connection = () => (client().online ? client().connection : null);
  const series = (pick: (sample: ClientSample) => number | null) =>
    props.detail.samples.map(pick).filter((value): value is number => value != null);
  // dBm as height above −100 dBm, so a stronger signal draws higher.
  const signal = () =>
    series((sample) => (sample.signal_dbm == null ? null : sample.signal_dbm + 100));
  const scores = () => series((sample) => sample.score);
  const down = () => series((sample) => (sample.down_bps == null ? null : Number(sample.down_bps)));
  const snr = () => {
    const c = connection();
    return c?.signal_dbm != null && c.noise_dbm != null ? c.signal_dbm - c.noise_dbm : null;
  };
  return (
    <div class="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-5">
      <Stat label={s.experience} value={client().experience?.score ?? NONE}>
        <Sparkline
          values={scores()}
          max={100}
          label={d.signal.chart(s.experience)}
          class="text-accent"
        />
      </Stat>
      <Stat label={s.signal} value={formatDbm(connection()?.signal_dbm)}>
        <Show when={signal().length > 1} fallback={snr() != null ? s.snr(snr() ?? 0) : null}>
          <Sparkline
            values={signal()}
            max={70}
            label={d.signal.chart(s.signal)}
            class="text-info"
          />
        </Show>
      </Stat>
      <Stat label={s.rate} value={formatRate(connection()?.tx_rate?.kbps)}>
        <Show when={connection()?.rx_rate}>{(rate) => s.up(formatRate(rate().kbps))}</Show>
      </Stat>
      <Stat label={s.traffic} value={formatBitrate(connection()?.traffic?.down_bps)}>
        <Show when={connection()?.traffic?.up_bps != null}>
          <span class="flex items-center gap-2">
            <span class="shrink-0">{s.up(formatBitrate(connection()?.traffic?.up_bps))}</span>
            <Sparkline
              values={down()}
              label={d.signal.chart(d.signal.series.down)}
              class="text-ok"
            />
          </span>
        </Show>
      </Stat>
      <Show
        when={connection()}
        fallback={
          <Stat
            label={s.lastSeen}
            value={strings.clients.ago(formatDuration((props.now - client().last_seen) / 1000))}
          />
        }
      >
        {(c) => <Stat label={s.connected} value={formatDuration((props.now - c().since) / 1000)} />}
      </Show>
    </div>
  );
}

function Rows(props: { rows: [string, JSX.Element][] }) {
  return (
    <dl class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-2 px-4 pb-4 text-sm">
      <For each={props.rows}>
        {([label, value]) => (
          <>
            <dt class="text-fg-subtle">{label}</dt>
            <dd class="min-w-0 truncate text-right font-medium">{value}</dd>
          </>
        )}
      </For>
    </dl>
  );
}

function Connection(props: { detail: ClientDetail; nodeName: (id: number) => string }) {
  const c = d.connection;
  const client = () => props.detail.client;
  const node = () => {
    const connection = client().connection;
    if (!connection) return NONE;
    const id = connection.node;
    if (id == null) return upstreamName(connection, props.nodeName);
    return (
      <a href={`/devices/${id}`} class="text-accent hover:underline">
        {props.nodeName(id)}
      </a>
    );
  };
  const rows = (): [string, JSX.Element][] => {
    const connection = client().connection;
    if (!connection) return [];
    if (connection.medium === "wired") {
      return [
        [c.node, node()],
        [c.wiredPort, connection.port ?? NONE],
        [c.since, formatDateTime(connection.since)],
      ];
    }
    // Rows nobody knows (a client behind an unadopted node) are left out.
    const known = ([, value]: [string, JSX.Element]) => value !== NONE && value != null;
    return (
      [
        [c.node, node()],
        [c.ssid, connection.ssid ?? NONE],
        [c.band, connection.band ? radios.band[connection.band] : NONE],
        [c.channel, connection.channel ?? NONE],
        [c.standard, connection.standard ? radios.standard[connection.standard] : NONE],
        [
          c.roaming,
          connection.capabilities.length > 0 ? (
            <span class="flex flex-wrap justify-end gap-1 whitespace-normal">
              <For each={connection.capabilities}>
                {(cap) => (
                  <span class="rounded-control bg-raised px-1.5 font-medium text-xs">
                    {d.capability[cap]}
                  </span>
                )}
              </For>
            </span>
          ) : (
            NONE
          ),
        ],
        [c.capacity, connection.capacity_mbps != null ? c.mbps(connection.capacity_mbps) : NONE],
        [c.port, connection.port ?? NONE],
        [c.since, formatDateTime(connection.since)],
      ] as [string, JSX.Element][]
    ).filter(known);
  };
  return (
    <Card class="min-w-0">
      <CardHeader
        title={c.title}
        description={
          client().online
            ? undefined
            : client().connection
              ? c.lastAt(
                  upstreamName(
                    client().connection ?? { node: null, upstream_name: null },
                    props.nodeName,
                  ),
                )
              : c.none
        }
      />
      <Rows rows={rows()} />
    </Card>
  );
}

function partValue(part: ExperiencePart): string {
  switch (part.factor) {
    case "snr":
      return `${Math.round(part.value)} dB`;
    case "signal":
      return formatDbm(part.value);
    case "roaming":
      return String(Math.round(part.value));
    default:
      return formatPercent(part.value);
  }
}

function Experience(props: { detail: ClientDetail }) {
  const e = d.experience;
  const experience = () => props.detail.client.experience;
  return (
    <Card class="min-w-0">
      <CardHeader
        title={e.title}
        actions={
          <Show when={experience()}>
            {(x) => <Badge tone={scoreTone(x().score)}>{x().score}</Badge>}
          </Show>
        }
      />
      <Show when={experience()} fallback={<p class="px-4 pb-4 text-fg-subtle text-sm">{e.none}</p>}>
        {(x) => (
          <ul class="flex flex-col gap-3 px-4 pb-4 text-sm">
            <For each={x().parts}>
              {(part) => (
                <li class="flex flex-col gap-1">
                  <span class="flex items-baseline justify-between gap-2">
                    <span title={e.weight(part.weight)}>{e.factor[part.factor]}</span>
                    <span class="text-fg-muted tabular-nums">
                      {partValue(part)}
                      <span class="sr-only">, {e.score(part.score)}</span>
                    </span>
                  </span>
                  <span class="h-1.5 overflow-hidden rounded-full bg-raised" aria-hidden="true">
                    <span
                      class={[
                        "block h-full rounded-full",
                        {
                          ok: "bg-ok",
                          warn: "bg-warn",
                          crit: "bg-crit",
                          neutral: "bg-fg-subtle",
                        }[scoreTone(part.score)],
                      ]}
                      style={{ width: `${part.score}%` }}
                    />
                  </span>
                </li>
              )}
            </For>
          </ul>
        )}
      </Show>
    </Card>
  );
}

function Details(props: { detail: ClientDetail }) {
  const t = d.details;
  const client = () => props.detail.client;
  const rows = (): [string, JSX.Element][] => [
    [t.mac, <span class="font-mono text-xs">{client().mac}</span>],
    [t.vendor, client().vendor ?? NONE],
    [t.kind, strings.clients.kind[client().kind]],
    [t.hostname, client().hostname ?? NONE],
    [
      t.addresses,
      client().ips.length > 0 ? (
        <span class="flex flex-col items-end font-mono text-xs">
          <For each={client().ips}>{(ip) => <span class="max-w-full truncate">{ip}</span>}</For>
        </span>
      ) : (
        NONE
      ),
    ],
    [t.firstSeen, formatDateTime(client().first_seen)],
    [t.lastSeen, formatDateTime(client().last_seen)],
    ...(client().group
      ? ([[t.group, client().group?.replace(/^(name|id):/, "") ?? NONE]] as [string, string][])
      : []),
  ];
  return (
    <Card class="min-w-0">
      <CardHeader title={t.title} />
      <Rows rows={rows()} />
    </Card>
  );
}
