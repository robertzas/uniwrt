import { useQuery } from "@tanstack/solid-query";
import { createMemo, createSignal, Show } from "solid-js";
import type { Client } from "../../../api/gen/Client";
import type { ConntrackTable } from "../../../api/gen/ConntrackTable";
import type { NodeId } from "../../../api/gen/NodeId";
import type { TrackedConnection } from "../../../api/gen/TrackedConnection";
import { api } from "../../../lib/api";
import { clientName } from "../../../lib/clients";
import { formatBytes, formatCount, formatDuration, NONE } from "../../../lib/format";
import { strings } from "../../../lib/strings";
import { Badge } from "../../../ui/Badge";
import { type Column, Table } from "../../../ui/Table";
import { ClientLink, Mono, TableView } from "./TableView";
import {
  type ClientIndex,
  compareAddresses,
  compareNumbers,
  compareText,
  endpoint,
  type FamilyFilter,
  filterWords,
  matchesWords,
  natOf,
  trafficOf,
} from "./tables";

const t = strings.device.tools.tables;
const c = t.conntrack;

interface Row {
  key: string;
  connection: TrackedConnection;
  source: Client | undefined;
  destination: Client | undefined;
  nat: { dnat?: string; snat?: string };
  traffic: number | null;
}

function Endpoint(props: { address: string; port: number | null; client: Client | undefined }) {
  return (
    <span class="inline-flex items-center gap-2">
      <Mono>{endpoint(props.address, props.port)}</Mono>
      <Show when={props.client}>
        <ClientLink client={props.client} />
      </Show>
    </span>
  );
}

const columns: Column<Row>[] = [
  {
    id: "protocol",
    header: c.protocol,
    cell: (row) => row.connection.protocol.toUpperCase(),
    sort: (a, b) => compareText(a.connection.protocol, b.connection.protocol),
  },
  {
    id: "source",
    header: c.source,
    cell: (row) => (
      <Endpoint
        address={row.connection.original.src}
        port={row.connection.original.sport}
        client={row.source}
      />
    ),
    sort: (a, b) => compareAddresses(a.connection.original.src, b.connection.original.src),
  },
  {
    id: "destination",
    header: c.destination,
    cell: (row) => (
      <Endpoint
        address={row.connection.original.dst}
        port={row.connection.original.dport}
        client={row.destination}
      />
    ),
    sort: (a, b) => compareAddresses(a.connection.original.dst, b.connection.original.dst),
  },
  {
    id: "nat",
    header: c.nat,
    cell: (row) => (
      <Show
        when={row.nat.dnat || row.nat.snat}
        fallback={<span class="text-fg-subtle">{NONE}</span>}
      >
        <span class="flex flex-col font-mono text-xs">
          <Show when={row.nat.dnat}>{(to) => <span>{c.dnat(to())}</span>}</Show>
          <Show when={row.nat.snat}>{(as) => <span>{c.snat(as())}</span>}</Show>
        </span>
      </Show>
    ),
  },
  {
    id: "state",
    header: c.state,
    cell: (row) => (
      <span class="inline-flex items-center gap-1.5">
        {row.connection.state ?? ""}
        {row.connection.unreplied ? <Badge tone="warn">{c.unreplied}</Badge> : null}
        {row.connection.offloaded ? (
          <span title={c.offloadedHint}>
            <Badge tone="info">{c.offloaded}</Badge>
          </span>
        ) : null}
      </span>
    ),
    sort: (a, b) => compareText(a.connection.state, b.connection.state),
  },
  {
    id: "traffic",
    header: c.traffic,
    cell: (row) => formatBytes(row.traffic),
    align: "end",
    sort: (a, b) => compareNumbers(a.traffic, b.traffic),
  },
  {
    id: "expires",
    header: c.expires,
    cell: (row) => formatDuration(row.connection.expires_s),
    align: "end",
    sort: (a, b) => compareNumbers(a.connection.expires_s, b.connection.expires_s),
  },
];

/** What the kernel's count says: how full the table is, or why it lists nothing. */
function Notice(props: { table: ConntrackTable }) {
  const text = () => {
    const { count, max, listed, truncated, connections } = props.table;
    if (max === null && count === 0) return c.off;
    if (!listed) return c.unlisted(formatCount(count));
    const share = max ? (count / max) * 100 : null;
    const line =
      max === null
        ? undefined
        : c.count(
            formatCount(count),
            formatCount(max),
            share !== null && share > 0 && share < 1 ? "<1 %" : `${Math.round(share ?? 0)} %`,
          );
    return [line, truncated ? c.truncated(formatCount(connections.length)) : undefined]
      .filter(Boolean)
      .join(" ");
  };
  return (
    <Show when={text()}>
      <p class="px-4 text-fg-muted text-sm">{text()}</p>
    </Show>
  );
}

/**
 * The connections the device tracks (its first 4,096 and the kernel's count), busiest first; each
 * end linked to the client it is, and where NAT moved it.
 */
export function ConntrackPanel(props: { node: NodeId; clients: ClientIndex }) {
  const query = useQuery(() => ({
    queryKey: ["diag-table", props.node, "conntrack"],
    queryFn: ({ signal }) => api.diag.conntrack(props.node, signal),
    retry: false,
    refetchOnWindowFocus: false,
  }));
  const [search, setSearch] = createSignal("");
  const [family, setFamily] = createSignal<FamilyFilter>("all");
  const client = (address: string) => props.clients.byIp.get(address.toLowerCase());
  const all = createMemo<Row[]>(() =>
    (query.data?.connections ?? []).map((connection, index) => ({
      key: String(index),
      connection,
      source: client(connection.original.src),
      destination: client(connection.original.dst),
      nat: natOf(connection),
      traffic: trafficOf(connection),
    })),
  );
  const rows = createMemo(() => {
    const words = filterWords(search());
    const want = family();
    return all().filter((row) => {
      const { connection: conn } = row;
      return (
        (want === "all" || conn.family === want) &&
        matchesWords(
          [
            conn.protocol,
            conn.state,
            endpoint(conn.original.src, conn.original.sport),
            endpoint(conn.original.dst, conn.original.dport),
            row.nat.dnat,
            row.nat.snat,
            row.source && clientName(row.source),
            row.destination && clientName(row.destination),
            conn.unreplied ? c.unreplied : null,
            conn.offloaded ? c.offloaded : null,
          ],
          words,
        )
      );
    });
  });
  return (
    <TableView
      query={query}
      search={search()}
      onSearch={setSearch}
      family={family()}
      onFamily={setFamily}
      counts={query.data ? { shown: rows().length, all: all().length } : undefined}
      notice={
        <Show when={query.data}>
          <Notice table={query.data as ConntrackTable} />
        </Show>
      }
    >
      {() => (
        <Table
          label={c.label}
          columns={columns}
          rows={rows()}
          rowKey={(row) => row.key}
          sortBy={{ column: "traffic", desc: true }}
          empty={all().length === 0 ? c.empty : t.noMatch}
        />
      )}
    </TableView>
  );
}
