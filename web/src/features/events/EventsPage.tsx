import { useSearchParams } from "@solidjs/router";
import { BellRing, Clock, RotateCw } from "lucide";
import { createEffect, createMemo, createSignal, For, onSettled, Show, useContext } from "solid-js";
import type { EventRecord } from "../../api/gen/EventRecord";
import { api } from "../../lib/api";
import { createClientsView } from "../../lib/clientList";
import { clientName } from "../../lib/clients";
import { describeEvent, type Names } from "../../lib/events";
import { LiveContext } from "../../lib/live";
import { strings } from "../../lib/strings";
import { StatusDot } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Card } from "../../ui/Card";
import { EmptyState } from "../../ui/EmptyState";
import { Icon } from "../../ui/Icon";
import { EventRow } from "./EventRow";
import { createEventFeed, type EventFeed, MAX_EVENTS } from "./feed";
import {
  EMPTY_FILTER,
  type EventParams,
  type EventRange,
  type Filter,
  filterParams,
  isFiltered,
  parseParams,
  rangeParams,
} from "./query";
import { Toolbar } from "./Toolbar";

const t = strings.events;

const dayFormat = new Intl.DateTimeFormat(undefined, {
  weekday: "long",
  month: "long",
  day: "numeric",
});

/** "Today", "Yesterday" or the date, for a day's heading. */
export function dayLabel(ms: number, now = Date.now()): string {
  const day = new Date(ms).toDateString();
  if (day === new Date(now).toDateString()) return t.today;
  if (day === new Date(now - 86_400_000).toDateString()) return t.yesterday;
  return dayFormat.format(new Date(ms));
}

/** Events newest first, in runs of one day. */
export function byDay(events: readonly EventRecord[]): { day: string; events: EventRecord[] }[] {
  const days: { day: string; events: EventRecord[] }[] = [];
  for (const event of events) {
    const day = new Date(event.at).toDateString();
    const last = days.at(-1);
    if (last?.day === day) last.events.push(event);
    else days.push({ day, events: [event] });
  }
  return days;
}

/** The ids of events that raised a condition nothing cleared since (among those shown). */
export function ongoing(events: readonly EventRecord[]): Set<number> {
  const newest = new Map<string, EventRecord>();
  for (const event of events) {
    const key = event.condition?.key;
    if (key === undefined) continue;
    const seen = newest.get(key);
    if (seen === undefined || event.at > seen.at || (event.at === seen.at && event.id > seen.id)) {
      newest.set(key, event);
    }
  }
  return new Set(
    [...newest.values()].filter((event) => event.condition?.raised).map((event) => event.id),
  );
}

/**
 * `/insights/events`: what happened across the network, newest first and live (features/
 * observability.md, Events). The filters and the range live in the URL
 * (`?categories=wan&min=warning&nodes=3&range=7d`), so the device and client pages link here.
 */
export function EventsPage() {
  const live = useContext(LiveContext);
  const [params, setParams] = useSearchParams<EventParams>();
  const query = createMemo(() =>
    parseParams({
      categories: params.categories,
      min: params.min,
      nodes: params.nodes,
      clients: params.clients,
      range: params.range,
      from: params.from,
      to: params.to,
    }),
  );
  const filter = () => query().filter;
  const feed = createEventFeed(live, query);
  const clients = createClientsView(live);
  const patterns = createPatterns();
  const [expanded, setExpanded] = createSignal<ReadonlySet<number>>(new Set());

  const nodeNames = createMemo(() => new Map(live.fleet.nodes.map((n) => [n.id, n.name])));
  const clientNames = createMemo(
    () => new Map(clients.clients.map((client) => [client.mac, clientName(client)])),
  );
  const names: Names = {
    node: (id) => nodeNames().get(id) ?? t.node(id),
    client: (mac) => clientNames().get(mac) ?? mac,
    pattern: (id) => patterns().get(id),
  };
  const open = createMemo(() => ongoing(feed.events()));
  const days = createMemo(() => byDay(feed.events()));

  const setFilter = (next: Filter) => setParams(filterParams(next));
  const setRange = (range: EventRange) => setParams(rangeParams(range));
  const toggle = (id: number) =>
    setExpanded((now) => {
      const next = new Set(now);
      if (!next.delete(id)) next.add(id);
      return next;
    });

  createEffect(
    () => t.title,
    (title) => {
      document.title = `${title} · ${strings.appName}`;
    },
  );

  const row = (event: EventRecord) => (
    <EventRow
      event={event}
      described={describeEvent(event, names)}
      device={
        event.node === null
          ? undefined
          : { name: names.node(event.node), href: `/devices/${event.node}` }
      }
      client={
        event.client === null
          ? undefined
          : {
              name: names.client(event.client),
              href: `/clients/${encodeURIComponent(event.client)}`,
            }
      }
      ongoing={feed.live() && open().has(event.id)}
      expanded={expanded().has(event.id)}
      onToggle={() => toggle(event.id)}
      onOnlyDevice={() =>
        setFilter({ ...filter(), nodes: event.node === null ? [] : [event.node] })
      }
      onOnlyCategory={() => setFilter({ ...filter(), categories: [event.category] })}
    />
  );

  return (
    <div class="mx-auto flex max-w-7xl flex-col gap-4">
      <header>
        <h1 class="font-semibold text-2xl tracking-tight">{t.title}</h1>
        <p class="mt-1 text-fg-muted text-sm">{strings.nav.events.description}</p>
      </header>
      <Card class="p-4">
        <Toolbar
          filter={filter()}
          range={query().range}
          nodes={live.fleet.nodes}
          clientName={names.client}
          onFilter={setFilter}
          onRange={setRange}
        />
      </Card>
      <Card class="overflow-hidden" aria-label={t.list}>
        <div class="flex items-center gap-3 border-edge border-b px-4 py-2.5">
          <p role="status" class="flex items-center gap-2 text-sm">
            <Show
              when={feed.live()}
              fallback={<Icon icon={Clock} class="size-3.5 text-fg-subtle" />}
            >
              <StatusDot
                tone={live.socket.state() === "open" ? "ok" : "warn"}
                pulse={live.socket.state() === "open"}
              />
            </Show>
            <span class="font-medium text-fg">
              {feed.live()
                ? live.socket.state() === "open"
                  ? t.state.live
                  : t.state.reconnecting
                : t.state.window}
            </span>
            <span class="text-fg-subtle tabular-nums">· {t.count(feed.events().length)}</span>
          </p>
        </div>
        <Show
          when={feed.events().length > 0}
          fallback={<Empty feed={feed} filter={filter()} onClear={() => setFilter(EMPTY_FILTER)} />}
        >
          <For each={days()}>
            {(group) => (
              <section aria-label={dayLabel(group.events[0]?.at ?? 0)}>
                <h2 class="sticky top-0 z-10 border-edge/60 border-b bg-surface/95 px-4 py-1.5 font-medium text-fg-subtle text-xs uppercase tracking-wide backdrop-blur">
                  {dayLabel(group.events[0]?.at ?? 0)}
                </h2>
                <ul>
                  <For each={group.events}>{(event) => row(event)}</For>
                </ul>
              </section>
            )}
          </For>
          <Footer
            older={feed.olderStatus()}
            hasOlder={feed.hasOlder()}
            capped={feed.capped()}
            onMore={() => feed.loadOlder()}
          />
        </Show>
      </Card>
    </div>
  );
}

function Empty(props: { feed: EventFeed; filter: Filter; onClear: () => void }) {
  return (
    <Show
      when={props.feed.status() !== "loading"}
      fallback={
        <p role="status" class="py-20 text-center text-fg-subtle text-sm">
          {t.loading}…
        </p>
      }
    >
      <Show
        when={props.feed.status() !== "failed"}
        fallback={
          <div role="alert" class="flex flex-col items-center gap-3 py-16 text-sm">
            <p class="text-fg-muted">{t.failed}</p>
            <Button size="sm" icon={RotateCw} onClick={() => props.feed.retry()}>
              {t.retry}
            </Button>
          </div>
        }
      >
        <div class="py-6">
          <EmptyState
            icon={BellRing}
            title={t.empty.title}
            description={isFiltered(props.filter) ? t.empty.filtered : t.empty.live}
          />
          <Show when={isFiltered(props.filter)}>
            <div class="mt-2 flex justify-center">
              <Button size="sm" onClick={() => props.onClear()}>
                {t.clear}
              </Button>
            </div>
          </Show>
        </div>
      </Show>
    </Show>
  );
}

function Footer(props: {
  older: "idle" | "loading" | "failed";
  hasOlder: boolean;
  capped: boolean;
  onMore: () => void;
}) {
  return (
    <div class="flex justify-center px-4 py-3 text-fg-subtle text-xs">
      <Show
        when={props.older !== "loading"}
        fallback={<span role="status">{t.loadingOlder}…</span>}
      >
        <Show
          when={props.hasOlder}
          fallback={<span>{props.capped ? t.capped(MAX_EVENTS) : t.end}</span>}
        >
          <div class="flex items-center gap-2">
            <Show when={props.older === "failed"}>
              <span role="alert">{t.olderFailed}</span>
            </Show>
            <Button size="sm" variant="ghost" onClick={() => props.onMore()}>
              {t.loadOlder}
            </Button>
          </div>
        </Show>
      </Show>
    </div>
  );
}

/** What each log pattern means, by id; read once when the page opens. */
function createPatterns(): () => Map<string, string> {
  const [patterns, setPatterns] = createSignal(new Map<string, string>());
  onSettled(() => {
    const controller = new AbortController();
    api.logs
      .patterns(controller.signal)
      .then((answer) => setPatterns(new Map(answer.patterns.map((p) => [p.id, p.meaning]))))
      .catch(() => {
        // Events from the logs show their pattern's id instead.
      });
    return () => controller.abort();
  });
  return patterns;
}
