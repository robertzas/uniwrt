import type { JSX } from "@solidjs/web";
import { keepPreviousData, useQuery } from "@tanstack/solid-query";
import { ListFilter, ScrollText } from "lucide";
import { createMemo, createSignal, For, Show } from "solid-js";
import type { Blocking } from "../../../api/gen/Blocking";
import type { Client } from "../../../api/gen/Client";
import type { DnsClientStats } from "../../../api/gen/DnsClientStats";
import type { DnsRange } from "../../../api/gen/DnsRange";
import type { DnsStats } from "../../../api/gen/DnsStats";
import type { DomainCount } from "../../../api/gen/DomainCount";
import type { SiteView } from "../../../api/gen/SiteView";
import { api, type DnsQueryFilter } from "../../../lib/api";
import { clientName } from "../../../lib/clients";
import { formatCount, NONE } from "../../../lib/format";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { RequestError } from "../../../ui/JobOutcome";
import { SavedRevision } from "../../../ui/SavedRevision";
import { Segmented } from "../../../ui/Segmented";
import { Stat } from "../../../ui/Stat";
import { createListing, DomainAction, type Listing } from "./DomainAction";
import { barsOf, blockedShare, HOUR } from "./dnsStats";
import { QueryBars } from "./QueryBars";
import { QueryLog } from "./QueryLog";

const t = strings.networks.queries;
/** How often the statistics are read again: the controller stores them every few seconds. */
const POLL_MS = 60_000;
/** The clients listed on the card; the query log filters by any. */
const TOP_CLIENTS = 8;
/** The top domains listed before "Show more" (the API sends up to 20). */
const TOP_DOMAINS = 10;
const RANGES: readonly DnsRange[] = ["24h", "7d", "30d"];

/**
 * DNS query statistics (network.md, Query statistics; D-133): the range's totals, its queries
 * over time, the busiest clients and the top answered and blocked domains of the last 24 h. A
 * domain is allowed or blocked from there (a revision each, for admins), and the query log opens
 * filtered by a client or a domain.
 */
export function DnsQueriesCard(props: {
  view: SiteView;
  admin: boolean;
  clients: readonly Client[];
}) {
  const [range, setRange] = createSignal<DnsRange>("24h");
  const stats = useQuery(() => ({
    queryKey: ["dns-stats", range()],
    queryFn: ({ signal }) => api.services.dnsStats(range(), signal),
    refetchInterval: POLL_MS,
    placeholderData: keepPreviousData,
  }));
  const [log, setLog] = createSignal<DnsQueryFilter>();
  const listing = createListing();
  const blocking = () => props.view.config.services.blocking;
  const byMac = createMemo(() => new Map(props.clients.map((c) => [c.mac, c])));
  const byKey = createMemo(
    () => new Map((stats.data?.clients ?? []).map((c) => [c.client, c.name] as const)),
  );
  /** As `/clients` names it: the statistics' name, the live client's, else its MAC or address. */
  const nameOf = (key: string, mac: string | null) => {
    const named = byKey().get(key);
    if (named) return named;
    const live = mac === null ? undefined : byMac().get(mac);
    return live ? clientName(live) : (mac ?? key);
  };

  return (
    <Card aria-label={t.title}>
      <CardHeader
        title={t.title}
        description={t.description}
        actions={
          <div class="flex flex-wrap items-center gap-2">
            <Segmented
              label={t.range}
              value={range()}
              options={RANGES.map((value) => ({ value, label: t.ranges[value] }))}
              onChange={setRange}
            />
            <Button size="sm" variant="secondary" icon={ScrollText} onClick={() => setLog({})}>
              {t.log}
            </Button>
          </div>
        }
      />
      <div class="flex flex-col gap-4 px-4 pb-4">
        <Show
          when={stats.data}
          fallback={
            <p role="status" class="text-fg-muted text-sm">
              {stats.isError ? "" : t.loading}
            </p>
          }
        >
          {(data) => (
            <Body
              data={data()}
              blocking={blocking()}
              admin={props.admin}
              listing={listing}
              nameOf={nameOf}
              onLog={setLog}
            />
          )}
        </Show>
        <RequestError text={stats.error?.message ?? (log() ? undefined : listing.failure())} />
        <Show when={listing.saved()}>{(revision) => <SavedRevision revision={revision()} />}</Show>
      </div>
      <Show when={log()}>
        {(filter) => (
          <QueryLog
            initial={filter()}
            clients={stats.data?.clients ?? []}
            nameOf={nameOf}
            blocking={blocking()}
            admin={props.admin}
            listing={listing}
            onClose={() => setLog(undefined)}
          />
        )}
      </Show>
    </Card>
  );
}

function Body(props: {
  data: DnsStats;
  blocking: Blocking;
  admin: boolean;
  listing: Listing;
  nameOf: (key: string, mac: string | null) => string;
  onLog: (filter: DnsQueryFilter) => void;
}) {
  const bars = createMemo(() => barsOf(props.data, Date.now()));
  const hours = () => Math.max(1, (Date.now() - props.data.since) / HOUR);
  return (
    <>
      <div class="grid grid-cols-2 gap-3 md:grid-cols-4">
        <Stat label={t.total} value={formatCount(props.data.queries)}>
          {t.perHour(formatCount(Math.round(props.data.queries / hours())))}
        </Stat>
        <Stat label={t.blocked} value={formatCount(props.data.blocked)} />
        <Stat
          label={t.share}
          value={blockedShare(props.data.queries, props.data.blocked) ?? NONE}
        />
        <Stat label={t.clients} value={formatCount(props.data.clients.length)} />
      </div>
      <Show
        when={props.data.queries > 0}
        fallback={<p class="text-fg-subtle text-sm">{t.empty}</p>}
      >
        <QueryBars
          bars={bars()}
          range={props.data.range}
          label={t.chart(
            t.rangeText[props.data.range],
            formatCount(props.data.queries),
            formatCount(props.data.blocked),
          )}
        />
      </Show>
      <div class="grid gap-4 lg:grid-cols-3">
        <TopClients
          clients={props.data.clients.slice(0, TOP_CLIENTS)}
          nameOf={props.nameOf}
          onLog={props.onLog}
        />
        <For each={[false, true]}>
          {(blocked) => (
            <TopDomains
              title={blocked ? t.topBlocked : t.topDomains}
              domains={blocked ? props.data.top_blocked : props.data.top_domains}
              blocked={blocked}
              blocking={props.blocking}
              admin={props.admin}
              listing={props.listing}
              onLog={props.onLog}
            />
          )}
        </For>
      </div>
    </>
  );
}

function Section(props: { title: string; note?: string; children: JSX.Element }) {
  return (
    <section aria-label={props.title} class="flex min-w-0 flex-col gap-2">
      <h3 class="flex items-baseline justify-between gap-2 font-medium text-sm">
        {props.title}
        <Show when={props.note}>
          <span class="font-normal text-fg-subtle text-xs">{props.note}</span>
        </Show>
      </h3>
      {props.children}
    </section>
  );
}

function TopClients(props: {
  clients: readonly DnsClientStats[];
  nameOf: (key: string, mac: string | null) => string;
  onLog: (filter: DnsQueryFilter) => void;
}) {
  const top = () => Math.max(1, props.clients[0]?.queries ?? 1);
  const width = (count: number) => `${((100 * count) / top()).toFixed(1)}%`;
  return (
    <Section title={t.topClients}>
      <ul class="flex flex-col gap-2">
        <For each={props.clients}>
          {(client) => {
            const name = () => props.nameOf(client.client, client.mac);
            return (
              <li class="flex min-w-0 items-center gap-2">
                <div class="flex min-w-0 flex-1 flex-col gap-1">
                  <div class="flex min-w-0 items-baseline justify-between gap-2 text-sm">
                    <Show when={client.mac} fallback={<span class="truncate">{name()}</span>}>
                      {(mac) => (
                        <a
                          href={`/clients/${encodeURIComponent(mac())}`}
                          class="truncate text-accent hover:underline"
                        >
                          {name()}
                        </a>
                      )}
                    </Show>
                    <span class="shrink-0 text-fg-subtle text-xs tabular-nums">
                      {t.clientLine(formatCount(client.queries), formatCount(client.blocked))}
                    </span>
                  </div>
                  <span
                    aria-hidden="true"
                    class="flex h-1.5 overflow-hidden rounded-full bg-raised"
                  >
                    <span class="bg-crit" style={{ width: width(client.blocked) }} />
                    <span
                      class="bg-accent/70"
                      style={{ width: width(client.queries - client.blocked) }}
                    />
                  </span>
                </div>
                <Button
                  size="sm"
                  variant="ghost"
                  icon={ListFilter}
                  aria-label={t.showQueries(name())}
                  onClick={() => props.onLog({ client: client.client })}
                />
              </li>
            );
          }}
        </For>
      </ul>
    </Section>
  );
}

function TopDomains(props: {
  title: string;
  domains: readonly DomainCount[];
  blocked: boolean;
  blocking: Blocking;
  admin: boolean;
  listing: Listing;
  onLog: (filter: DnsQueryFilter) => void;
}) {
  const [all, setAll] = createSignal(false);
  const shown = () => (all() ? props.domains : props.domains.slice(0, TOP_DOMAINS));
  const hidden = () => props.domains.length - TOP_DOMAINS;
  return (
    <Section title={props.title} note={t.lastDay}>
      <Show
        when={props.domains.length > 0}
        fallback={<p class="text-fg-subtle text-sm">{t.noDomains}</p>}
      >
        <ol class="flex flex-col">
          <For each={shown()}>
            {(entry) => (
              <li class="group flex min-h-8 min-w-0 items-center gap-2 text-sm">
                <button
                  type="button"
                  title={entry.domain}
                  class="min-w-0 flex-1 truncate text-left hover:text-accent hover:underline"
                  onClick={() =>
                    props.onLog({
                      domain: entry.domain,
                      outcome: props.blocked ? "blocked" : undefined,
                    })
                  }
                >
                  {entry.domain}
                </button>
                <span
                  class="shrink-0 text-fg-subtle text-xs tabular-nums"
                  title={t.count(formatCount(entry.queries))}
                >
                  {formatCount(entry.queries)}
                </span>
                <DomainAction
                  domain={entry.domain}
                  blocked={props.blocked}
                  blocking={props.blocking}
                  admin={props.admin}
                  listing={props.listing}
                />
              </li>
            )}
          </For>
        </ol>
        <Show when={hidden() > 0}>
          <Button size="sm" variant="ghost" class="self-start" onClick={() => setAll(!all())}>
            {all() ? t.fewer : t.more(hidden())}
          </Button>
        </Show>
      </Show>
    </Section>
  );
}
