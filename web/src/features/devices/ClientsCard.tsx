import { createMemo, Show } from "solid-js";
import type { LeaseFacts } from "../../api/gen/LeaseFacts";
import type { NodeFacts } from "../../api/gen/NodeFacts";
import type { StationFacts } from "../../api/gen/StationFacts";
import { formatBytes, formatDbm, formatDuration, formatRate, NONE } from "../../lib/format";
import { strings } from "../../lib/strings";
import { StatusDot, type Tone } from "../../ui/Badge";
import { Card, CardHeader } from "../../ui/Card";
import { type Column, Table } from "../../ui/Table";

const t = strings.device.clients;

/** A Wi-Fi client, named and addressed from DHCP leases where they know it. */
interface ClientRow {
  station: StationFacts;
  hostname: string | null;
  ip: string | null;
  network: string;
}

function signalTone(dbm: number | null): Tone {
  if (dbm == null) return "neutral";
  if (dbm >= -65) return "ok";
  return dbm >= -75 ? "warn" : "crit";
}

/**
 * The node's clients (mesh peers excluded), named from its own DHCP leases and `leases` from
 * elsewhere (an access point's clients get theirs from the gateway).
 */
export function clientRows(facts: NodeFacts, leases: readonly LeaseFacts[] = []): ClientRow[] {
  const byMac = new Map<string, LeaseFacts>();
  for (const lease of [...leases, ...facts.leases]) {
    // IPv4 leases name the client; DHCPv6 ones without a MAC can't be matched.
    if (lease.mac && (!byMac.has(lease.mac) || !lease.ip.includes(":")))
      byMac.set(lease.mac, lease);
  }
  const networks = new Map<string, string>();
  for (const radio of facts.radios) {
    for (const iface of radio.ifaces) {
      if (iface.name) {
        const band = strings.device.radios.band[radio.band];
        networks.set(iface.name, iface.ssid ? `${iface.ssid} · ${band}` : band);
      }
    }
  }
  return facts.stations
    .filter((station) => station.peer_link === null)
    .map((station) => ({
      station,
      hostname: byMac.get(station.mac)?.hostname ?? null,
      ip: byMac.get(station.mac)?.ip ?? null,
      network: networks.get(station.iface) ?? station.iface,
    }));
}

const columns: Column<ClientRow>[] = [
  {
    id: "client",
    header: t.columns.client,
    cell: (row) => (
      <span
        class={["block max-w-56 truncate font-medium", { "font-mono text-xs": !row.hostname }]}
        title={row.station.mac}
      >
        {row.hostname ?? row.station.mac}
      </span>
    ),
  },
  {
    id: "ip",
    header: t.columns.ip,
    cell: (row) => <span class="font-mono text-xs">{row.ip ?? NONE}</span>,
  },
  { id: "network", header: t.columns.network, cell: (row) => row.network },
  {
    id: "signal",
    header: t.columns.signal,
    cell: (row) => (
      <span class="inline-flex items-center gap-2">
        <StatusDot tone={signalTone(row.station.signal_dbm)} />
        {formatDbm(row.station.signal_dbm)}
      </span>
    ),
    align: "end",
  },
  {
    id: "rate",
    header: t.columns.rate,
    // Directions are the node's: it transmits what the client downloads.
    cell: (row) =>
      `${formatRate(row.station.tx_rate?.kbps)} / ${formatRate(row.station.rx_rate?.kbps)}`,
    align: "end",
  },
  {
    id: "connected",
    header: t.columns.connected,
    cell: (row) => formatDuration(row.station.connected_s),
    align: "end",
  },
  {
    id: "traffic",
    header: t.columns.traffic,
    cell: (row) =>
      row.station.rx_bytes + row.station.tx_bytes > 0
        ? formatBytes(row.station.rx_bytes + row.station.tx_bytes)
        : NONE,
    align: "end",
  },
];

/** The node's Wi-Fi clients, live. */
export function ClientsCard(props: { facts: NodeFacts; leases: readonly LeaseFacts[] }) {
  const rows = createMemo(() => clientRows(props.facts, props.leases));
  return (
    <Card class="min-w-0 overflow-hidden">
      <CardHeader title={t.title} actions={<span class="tabular-nums">{rows().length}</span>} />
      <Show
        when={rows().length > 0}
        fallback={<p class="px-4 pb-4 text-fg-subtle text-sm">{t.empty}</p>}
      >
        <Table label={t.title} columns={columns} rows={rows()} rowKey={(row) => row.station.mac} />
      </Show>
    </Card>
  );
}
