import { useQuery } from "@tanstack/solid-query";
import { createMemo, createSignal, For, Show } from "solid-js";
import type { NodeId } from "../../../api/gen/NodeId";
import type { Route } from "../../../api/gen/Route";
import { api } from "../../../lib/api";
import { NONE } from "../../../lib/format";
import { strings } from "../../../lib/strings";
import { Badge } from "../../../ui/Badge";
import { SELECT } from "../../../ui/FormField";
import { type Column, Table } from "../../../ui/Table";
import { Mono, TableView } from "./TableView";
import {
  compareAddresses,
  compareNumbers,
  compareText,
  type FamilyFilter,
  filterWords,
  matchesWords,
} from "./tables";

const t = strings.device.tools.tables;
const r = t.routes;
const ALL = "*";

interface Row {
  key: string;
  route: Route;
}

const devices = (route: Route) => [
  ...new Set(route.hops.map((hop) => hop.device).filter((device) => device !== null)),
];

const columns: Column<Row>[] = [
  {
    id: "destination",
    header: r.destination,
    cell: (row) => (
      <span class="inline-flex items-center gap-2">
        <Mono>{row.route.destination}</Mono>
        {row.route.kind !== "unicast" ? <Badge>{row.route.kind}</Badge> : null}
      </span>
    ),
    sort: (a, b) => compareAddresses(a.route.destination, b.route.destination),
  },
  {
    id: "via",
    header: r.via,
    cell: (row) => (
      <Show
        when={row.route.hops.length > 1}
        fallback={
          <Show
            when={row.route.hops[0]?.via}
            fallback={<span class="text-fg-subtle">{row.route.hops.length ? r.direct : NONE}</span>}
          >
            {(via) => <Mono>{via()}</Mono>}
          </Show>
        }
      >
        <span class="flex flex-col py-1">
          <For each={row.route.hops}>
            {(hop) => (
              <Mono>
                {hop.via ?? r.direct}
                {hop.device ? ` (${hop.device})` : ""}
                {hop.weight !== null ? ` ×${hop.weight}` : ""}
              </Mono>
            )}
          </For>
        </span>
      </Show>
    ),
    sort: (a, b) => compareAddresses(a.route.hops[0]?.via ?? "", b.route.hops[0]?.via ?? ""),
  },
  {
    id: "device",
    header: r.device,
    cell: (row) => devices(row.route).join(", ") || NONE,
    sort: (a, b) => compareText(devices(a.route)[0], devices(b.route)[0]),
  },
  {
    id: "table",
    header: r.table,
    cell: (row) => row.route.table,
    sort: (a, b) => compareText(a.route.table, b.route.table),
  },
  {
    id: "protocol",
    header: r.protocol,
    cell: (row) => row.route.protocol ?? NONE,
    sort: (a, b) => compareText(a.route.protocol, b.route.protocol),
  },
  {
    id: "source",
    header: r.source,
    cell: (row) => (row.route.source ? <Mono>{row.route.source}</Mono> : NONE),
    sort: (a, b) => compareAddresses(a.route.source ?? "", b.route.source ?? ""),
  },
  {
    id: "scope",
    header: r.scope,
    cell: (row) => row.route.scope ?? NONE,
    sort: (a, b) => compareText(a.route.scope, b.route.scope),
  },
  {
    id: "metric",
    header: r.metric,
    cell: (row) => row.route.metric ?? NONE,
    align: "end",
    sort: (a, b) => compareNumbers(a.route.metric, b.route.metric),
  },
];

/** The device's routes, of one table (`main` at first) or all of them, in the kernel's order. */
export function RoutesPanel(props: { node: NodeId }) {
  const query = useQuery(() => ({
    queryKey: ["diag-table", props.node, "routes"],
    queryFn: ({ signal }) => api.diag.routes(props.node, signal),
    retry: false,
    refetchOnWindowFocus: false,
  }));
  const [search, setSearch] = createSignal("");
  const [family, setFamily] = createSignal<FamilyFilter>("all");
  const [table, setTable] = createSignal("main");
  const all = createMemo<Row[]>(() =>
    (query.data ?? []).map((route, index) => ({ key: String(index), route })),
  );
  const tables = createMemo(() => {
    const names = [...new Set(all().map((row) => row.route.table))];
    return names.sort((a, b) => Number(b === "main") - Number(a === "main") || a.localeCompare(b));
  });
  const rows = createMemo(() => {
    const words = filterWords(search());
    const want = family();
    const only = table();
    return all().filter(
      ({ route }) =>
        (want === "all" || route.family === want) &&
        (only === ALL || route.table === only) &&
        matchesWords(
          [
            route.destination,
            route.kind,
            route.table,
            route.protocol,
            route.scope,
            route.source,
            ...route.hops.flatMap((hop) => [hop.via, hop.device]),
          ],
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
      filters={
        <label class="flex items-center gap-2 text-fg-subtle text-xs">
          {r.table}
          <select
            value={table()}
            onChange={(event) => setTable(event.currentTarget.value)}
            class={SELECT.replace("h-9 w-full", "h-8 w-auto")}
          >
            <option value={ALL}>{r.allTables}</option>
            <For each={tables()}>{(name) => <option value={name}>{name}</option>}</For>
          </select>
        </label>
      }
    >
      {() => (
        <Table
          label={r.label}
          columns={columns}
          rows={rows()}
          rowKey={(row) => row.key}
          empty={all().length === 0 ? r.empty : t.noMatch}
        />
      )}
    </TableView>
  );
}
