import { useQuery } from "@tanstack/solid-query";
import { createMemo, createSignal } from "solid-js";
import type { Client } from "../../../api/gen/Client";
import type { NeighborEntry } from "../../../api/gen/NeighborEntry";
import type { NodeId } from "../../../api/gen/NodeId";
import { api } from "../../../lib/api";
import { clientName } from "../../../lib/clients";
import { strings } from "../../../lib/strings";
import { Badge } from "../../../ui/Badge";
import { type Column, Table } from "../../../ui/Table";
import { ClientLink, Mono, TableView } from "./TableView";
import {
  type ClientIndex,
  compareAddresses,
  compareText,
  type FamilyFilter,
  filterWords,
  matchesWords,
  neighborClient,
  neighborTone,
} from "./tables";

const t = strings.device.tools.tables;
const n = t.neighbors;

interface Row {
  key: string;
  entry: NeighborEntry;
  client: Client | undefined;
}

const name = (row: Row) => (row.client ? clientName(row.client) : null);

const columns: Column<Row>[] = [
  {
    id: "address",
    header: n.address,
    cell: (row) => (
      <span class="inline-flex items-center gap-2">
        <Mono>{row.entry.address}</Mono>
        {row.entry.router ? <Badge tone="info">{t.router}</Badge> : null}
      </span>
    ),
    sort: (a, b) => compareAddresses(a.entry.address, b.entry.address),
  },
  {
    id: "mac",
    header: n.mac,
    cell: (row) =>
      row.entry.mac ? (
        <Mono>{row.entry.mac}</Mono>
      ) : (
        <span class="text-fg-subtle">{n.unresolved}</span>
      ),
    sort: (a, b) => compareText(a.entry.mac, b.entry.mac),
  },
  {
    id: "client",
    header: n.client,
    cell: (row) => <ClientLink client={row.client} />,
    sort: (a, b) => compareText(name(a), name(b)),
  },
  {
    id: "device",
    header: n.device,
    cell: (row) => row.entry.device,
    sort: (a, b) => compareText(a.entry.device, b.entry.device),
  },
  {
    id: "state",
    header: n.state,
    cell: (row) =>
      row.entry.state ? (
        <Badge tone={neighborTone(row.entry.state)}>{row.entry.state}</Badge>
      ) : null,
    sort: (a, b) => compareText(a.entry.state, b.entry.state),
  },
];

/** The neighbor cache (ARP and NDP), each entry linked to the client it is. */
export function NeighborsPanel(props: { node: NodeId; clients: ClientIndex }) {
  const query = useQuery(() => ({
    queryKey: ["diag-table", props.node, "neighbors"],
    queryFn: ({ signal }) => api.diag.neighbors(props.node, signal),
    retry: false,
    refetchOnWindowFocus: false,
  }));
  const [search, setSearch] = createSignal("");
  const [family, setFamily] = createSignal<FamilyFilter>("all");
  const all = createMemo<Row[]>(() =>
    (query.data ?? []).map((entry) => ({
      key: `${entry.device}|${entry.address}`,
      entry,
      client: neighborClient(entry, props.clients),
    })),
  );
  const rows = createMemo(() => {
    const words = filterWords(search());
    const want = family();
    return all().filter(
      (row) =>
        (want === "all" || (want === "ipv6") === row.entry.address.includes(":")) &&
        matchesWords(
          [row.entry.address, row.entry.mac, name(row), row.entry.device, row.entry.state],
          words,
        ),
    );
  });
  return (
    <TableView
      query={query}
      search={search()}
      onSearch={setSearch}
      family={family()}
      onFamily={setFamily}
      counts={query.data ? { shown: rows().length, all: all().length } : undefined}
    >
      {() => (
        <Table
          label={n.label}
          columns={columns}
          rows={rows()}
          rowKey={(row) => row.key}
          sortBy={{ column: "address" }}
          empty={all().length === 0 ? n.empty : t.noMatch}
        />
      )}
    </TableView>
  );
}
