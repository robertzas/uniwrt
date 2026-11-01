import { useNavigate } from "@solidjs/router";
import { Search, Users } from "lucide";
import { createEffect, createMemo, createSignal, For, Show, useContext } from "solid-js";
import type { Client } from "../../api/gen/Client";
import { scoreTone } from "../../lib/clients";
import { NONE } from "../../lib/format";
import { LiveContext } from "../../lib/live";
import { strings } from "../../lib/strings";
import { Badge } from "../../ui/Badge";
import { Card, CardHeader } from "../../ui/Card";
import { EmptyState } from "../../ui/EmptyState";
import { Icon } from "../../ui/Icon";
import { Stat } from "../../ui/Stat";
import { Table } from "../../ui/Table";
import { clientColumns, createNow } from "./columns";
import {
  averageScore,
  type ClientRow,
  createClientsView,
  groupClients,
  scoresByNode,
} from "./store";

const t = strings.clients;

type Filter = "all" | "wifi" | "wired" | "offline";
const filters: readonly Filter[] = ["all", "wifi", "wired", "offline"];

/** The row's client is in the filter, and any of the device's addresses has every word. */
function matches(row: ClientRow, filter: Filter, words: readonly string[]): boolean {
  const client = row.client;
  const medium = client.connection?.medium;
  if (filter === "offline" && client.online) return false;
  if (filter === "wifi" && !(client.online && medium === "wifi")) return false;
  if (filter === "wired" && !(client.online && medium === "wired")) return false;
  if (words.length === 0) return true;
  return [client, ...row.others].some((member) => {
    const text = [member.alias, member.hostname, member.mac, member.vendor, ...member.ips]
      .filter(Boolean)
      .join(" ")
      .toLowerCase();
    return words.every((word) => text.includes(word));
  });
}

/**
 * `/clients`: every client the controller knows, live: who is online and how well, by access
 * point, then the list (search and filter; randomized addresses of a device share a row).
 */
export function ClientsPage() {
  const live = useContext(LiveContext);
  const navigate = useNavigate();
  const view = createClientsView(live);
  const now = createNow();
  const [query, setQuery] = createSignal("");
  const [filter, setFilter] = createSignal<Filter>("all");
  const nodeName = (id: number) =>
    live.fleet.nodes.find((node) => node.id === id)?.name ?? `#${id}`;
  const columns = clientColumns({ nodeName, now });
  const rows = createMemo(() => {
    const words = query().toLowerCase().split(/\s+/).filter(Boolean);
    return groupClients(view.clients).filter((row) => matches(row, filter(), words));
  });
  createEffect(
    () => t.title,
    (title) => {
      document.title = `${title} · ${strings.appName}`;
    },
  );

  return (
    <Show
      when={view.ready}
      fallback={
        <p role="status" class="py-20 text-center text-fg-subtle text-sm">
          {t.loading}…
        </p>
      }
    >
      <Show
        when={view.clients.length > 0}
        fallback={
          <EmptyState icon={Users} title={t.empty.title} description={t.empty.description} />
        }
      >
        <div class="mx-auto flex max-w-7xl flex-col gap-4">
          <Summary clients={view.clients} nodeName={nodeName} />
          <Card class="overflow-hidden">
            <CardHeader
              title={t.title}
              description={t.count(
                view.clients.filter((client) => client.online).length,
                view.clients.length,
              )}
              actions={
                <div class="flex flex-wrap items-center gap-2">
                  <label class="relative">
                    <span class="sr-only">{t.search}</span>
                    <Icon
                      icon={Search}
                      class="-translate-y-1/2 pointer-events-none absolute top-1/2 left-2.5 size-4 text-fg-subtle"
                    />
                    <input
                      type="search"
                      value={query()}
                      onInput={(event) => setQuery(event.currentTarget.value)}
                      placeholder={t.searchPlaceholder}
                      class="h-8 w-56 rounded-control border border-edge bg-canvas pr-3 pl-8 text-sm outline-none placeholder:text-fg-subtle focus:border-accent focus:ring-2 focus:ring-accent/30"
                    />
                  </label>
                  <fieldset class="flex rounded-control border border-edge p-0.5">
                    <legend class="sr-only">{t.filter.label}</legend>
                    <For each={filters}>
                      {(option) => (
                        <button
                          type="button"
                          aria-pressed={filter() === option ? "true" : "false"}
                          onClick={() => setFilter(option)}
                          class={[
                            "h-7 rounded-control px-2.5 font-medium text-xs transition-colors",
                            filter() === option
                              ? "bg-raised text-fg"
                              : "text-fg-subtle hover:text-fg",
                          ]}
                        >
                          {t.filter[option]}
                        </button>
                      )}
                    </For>
                  </fieldset>
                </div>
              }
            />
            <Table
              label={t.title}
              columns={columns}
              rows={rows()}
              rowKey={(row) => row.client.mac}
              onRowClick={(row) => navigate(`/clients/${row.client.mac}`)}
              empty={t.noMatch}
            />
          </Card>
        </div>
      </Show>
    </Show>
  );
}

/** The headline numbers: who is online, the site's Wi-Fi experience, and each AP's. */
function Summary(props: { clients: readonly Client[]; nodeName: (id: number) => string }) {
  const s = t.stat;
  const online = () => props.clients.filter((client) => client.online);
  const wifi = () => online().filter((client) => client.connection?.medium === "wifi").length;
  const weak = () => online().filter((client) => (client.connection?.signal_dbm ?? 0) < -75).length;
  const site = createMemo(() => averageScore(props.clients));
  const byNode = createMemo(() => scoresByNode(props.clients));
  return (
    <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
      <Stat label={s.online} value={online().length}>
        {s.onlineDetail(wifi(), online().length - wifi())}
      </Stat>
      <Stat label={s.experience} value={site().score ?? "—"}>
        <Show when={site().count > 0}>{s.experienceDetail(site().count)}</Show>
      </Stat>
      <Stat label={s.weak} value={weak()}>
        {s.weakDetail}
      </Stat>
      <Card class="flex min-w-0 flex-col gap-1.5 px-3.5 py-3">
        <span class="truncate font-medium text-fg-subtle text-xs">{s.byAp}</span>
        <ul class="flex flex-col gap-1 text-sm">
          <For each={byNode()} fallback={<li class="text-fg-subtle">{NONE}</li>}>
            {(entry) => (
              <li class="flex items-center justify-between gap-2">
                <span class="truncate">{props.nodeName(entry.node)}</span>
                <span class="flex shrink-0 items-center gap-2 text-fg-subtle text-xs tabular-nums">
                  {entry.count}
                  <Badge tone={scoreTone(entry.score)}>{entry.score}</Badge>
                </span>
              </li>
            )}
          </For>
        </ul>
      </Card>
    </div>
  );
}
