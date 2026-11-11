import { Ban, Shuffle } from "lucide";
import { createSignal, onSettled, Show } from "solid-js";
import type { Client } from "../../api/gen/Client";
import { clientName, kindIcons, scoreTone, signalTone, upstreamName } from "../../lib/clients";
import { formatBitrate, formatDbm, formatDuration, NONE } from "../../lib/format";
import { strings } from "../../lib/strings";
import { Badge, StatusDot } from "../../ui/Badge";
import { Icon } from "../../ui/Icon";
import type { Column } from "../../ui/Table";
import type { ClientRow } from "./store";

const t = strings.clients;
const bands = strings.device.radios.band;

/** The time now, in ms, moving on every 15 seconds while the calling component lives. */
export function createNow(): () => number {
  const [now, setNow] = createSignal(Date.now());
  onSettled(() => {
    const timer = setInterval(() => setNow(Date.now()), 15_000);
    return () => clearInterval(timer);
  });
  return now;
}

/** Where a client is (or was): the access point with its network and band, or the port. */
export function placeOf(client: Client, nodeName: (id: number) => string): string {
  const connection = client.connection;
  if (!connection) return NONE;
  const node = upstreamName(connection, nodeName);
  if (connection.medium === "wired") {
    return connection.port ? t.via(node, connection.port) : node;
  }
  const network = [connection.ssid, connection.band && bands[connection.band]]
    .filter(Boolean)
    .join(" · ");
  return network ? t.via(node, network) : node;
}

/** How long it has been connected, or since it was last seen. */
export function sinceOf(client: Client, now: number): string {
  if (client.online && client.connection) {
    return formatDuration((now - client.connection.since) / 1000);
  }
  return t.ago(formatDuration((now - client.last_seen) / 1000));
}

/** The list's columns; `nodeName` names an adopted node by ID. */
export function clientColumns(context: {
  nodeName: (id: number) => string;
  now: () => number;
}): Column<ClientRow>[] {
  const c = t.columns;
  return [
    {
      id: "client",
      header: c.client,
      cell: (row) => <ClientCell row={row} />,
      class: "min-w-56",
    },
    {
      id: "ip",
      header: c.ip,
      cell: (row) => <span class="font-mono text-xs">{row.client.ips[0] ?? NONE}</span>,
    },
    {
      id: "connection",
      header: c.connection,
      cell: (row) => (
        <span
          class={["block max-w-56 truncate", { "text-fg-subtle": !row.client.online }]}
          title={placeOf(row.client, context.nodeName)}
        >
          {placeOf(row.client, context.nodeName)}
        </span>
      ),
    },
    {
      id: "signal",
      header: c.signal,
      cell: (row) => (
        <Show when={row.client.online && row.client.connection?.signal_dbm} fallback={NONE}>
          {(dbm) => (
            <span class="inline-flex items-center gap-2">
              <StatusDot tone={signalTone(dbm())} />
              {formatDbm(dbm())}
            </span>
          )}
        </Show>
      ),
      align: "end",
    },
    {
      id: "experience",
      header: c.experience,
      cell: (row) => (
        <Show when={row.client.experience} fallback={NONE}>
          {(experience) => <Badge tone={scoreTone(experience().score)}>{experience().score}</Badge>}
        </Show>
      ),
      align: "end",
    },
    {
      id: "traffic",
      header: c.traffic,
      cell: (row) => {
        const traffic = row.client.online ? row.client.connection?.traffic : null;
        return traffic?.down_bps != null
          ? `${formatBitrate(traffic.down_bps)} / ${formatBitrate(traffic.up_bps)}`
          : NONE;
      },
      align: "end",
      class: "whitespace-nowrap",
    },
    {
      id: "since",
      header: c.since,
      cell: (row) => sinceOf(row.client, context.now()),
      align: "end",
      class: "whitespace-nowrap",
    },
  ];
}

function ClientCell(props: { row: ClientRow }) {
  const client = () => props.row.client;
  const subtitle = () =>
    [client().vendor, client().alias ? client().hostname : null, t.kind[client().kind]]
      .filter(Boolean)
      .join(" · ");
  return (
    <span class="flex min-w-0 items-center gap-2.5">
      <span class="relative shrink-0">
        <Icon icon={kindIcons[client().kind]} class="size-5 text-fg-subtle" />
        <StatusDot
          tone={client().online ? "ok" : "neutral"}
          class="-right-0.5 -bottom-0.5 absolute ring-2 ring-surface"
        />
      </span>
      <span class="flex min-w-0 flex-col">
        <span class="flex min-w-0 items-center gap-1.5">
          <span
            class={[
              "truncate font-medium",
              { "font-mono text-xs": clientName(client()) === client().mac },
            ]}
          >
            {clientName(client())}
          </span>
          <Show when={client().randomized}>
            <span title={t.privateHint} class="shrink-0 text-fg-subtle">
              <Icon icon={Shuffle} class="size-3.5" />
              <span class="sr-only">{t.private}</span>
            </span>
          </Show>
          <Show when={client().blocked !== null}>
            <span title={t.blocked} class="shrink-0 text-crit">
              <Icon icon={Ban} class="size-3.5" />
              <span class="sr-only">{t.blocked}</span>
            </span>
          </Show>
          <Show when={props.row.others.length > 0}>
            <span class="shrink-0 text-fg-subtle text-xs">
              {t.addresses(props.row.others.length)}
            </span>
          </Show>
        </span>
        <span class="truncate text-fg-subtle text-xs">{subtitle()}</span>
      </span>
    </span>
  );
}
