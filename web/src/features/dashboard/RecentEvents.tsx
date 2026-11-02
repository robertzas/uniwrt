import { BellRing } from "lucide";
import { createMemo, createSignal, For, onSettled, Show } from "solid-js";
import type { EventRecord } from "../../api/gen/EventRecord";
import type { ServerMessage } from "../../api/gen/ServerMessage";
import { api } from "../../lib/api";
import { categoryIcons, describeEvent, levelTones, type Names } from "../../lib/events";
import type { Live } from "../../lib/live";
import { strings } from "../../lib/strings";
import { Card, CardHeader } from "../../ui/Card";
import { Icon } from "../../ui/Icon";

const t = strings.dashboard.events;
/** The events the card shows, the newest. */
export const COUNT = 6;
/** The WS topic it follows: notices and worse (clients' comings and goings stay on the page). */
export const TOPIC = "events?min=notice";

const TILES = {
  crit: "bg-crit/10 text-crit",
  warn: "bg-warn/10 text-warn",
  info: "bg-info/10 text-info",
  neutral: "bg-raised text-fg-subtle",
} as const;

const clock = new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" });
const day = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" });

/** `14:05` today, else `Oct 31`. */
export function shortTime(ms: number, now = Date.now()): string {
  const date = new Date(ms);
  return date.toDateString() === new Date(now).toDateString()
    ? clock.format(date)
    : day.format(date);
}

/** The newest `count` of `current` and `incoming`, an id once (a fold replaces its record). */
export function newest(
  current: readonly EventRecord[],
  incoming: readonly EventRecord[],
  count = COUNT,
): EventRecord[] {
  const byId = new Map(current.map((event) => [event.id, event]));
  for (const event of incoming) byId.set(event.id, event);
  return [...byId.values()].sort((a, b) => b.at - a.at || b.id - a.id).slice(0, count);
}

/** The latest notable events, live, each linking to the events page around it. */
export function RecentEvents(props: {
  live: Live;
  nodeName: (id: number) => string;
  clientName: (mac: string) => string;
}) {
  const [events, setEvents] = createSignal<EventRecord[] | undefined>();
  const [patterns, setPatterns] = createSignal(new Map<string, string>());
  onSettled(() => {
    const onMessage = (message: ServerMessage) => {
      if (message.type === "events_snapshot") setEvents(newest([], message.events));
      else if (message.type === "events_delta") {
        setEvents((now) => newest(now ?? [], message.events));
      }
    };
    const unsubscribe = props.live.socket.subscribe(TOPIC, onMessage);
    const controller = new AbortController();
    api.logs
      .patterns(controller.signal)
      .then((answer) => setPatterns(new Map(answer.patterns.map((p) => [p.id, p.meaning]))))
      .catch(() => {
        // Events from the logs show their pattern's id instead.
      });
    return () => {
      unsubscribe();
      controller.abort();
    };
  });
  const names: Names = {
    node: (id) => props.nodeName(id),
    client: (mac) => props.clientName(mac),
    pattern: (id) => patterns().get(id),
  };
  const shown = createMemo(() =>
    (events() ?? []).map((event) => ({ event, described: describeEvent(event, names) })),
  );
  return (
    <Card class="flex min-w-0 flex-col" aria-label={t.title}>
      <CardHeader
        title={t.title}
        description={t.description}
        actions={
          <a href="/insights/events" class="font-medium text-accent hover:underline">
            {t.all}
          </a>
        }
      />
      <Show
        when={events()}
        fallback={
          <p role="status" class="px-4 pb-4 text-fg-subtle text-sm">
            {t.loading}…
          </p>
        }
      >
        <Show
          when={shown().length > 0}
          fallback={
            <div class="flex flex-1 flex-col items-center justify-center gap-3 px-6 pb-6 text-center">
              <span class="grid size-10 place-items-center rounded-xl border border-edge bg-raised text-fg-subtle">
                <Icon icon={BellRing} class="size-4.5" />
              </span>
              <p class="max-w-xs text-fg-subtle text-sm">{t.empty}</p>
            </div>
          }
        >
          <ul class="flex flex-col px-2 pb-2">
            <For each={shown()}>
              {(item) => (
                <li>
                  <a
                    href={
                      item.event.node === null
                        ? "/insights/events"
                        : `/insights/events?nodes=${item.event.node}`
                    }
                    class="flex min-w-0 items-center gap-3 rounded-control px-2 py-1.5 hover:bg-raised"
                  >
                    <span
                      class={[
                        "grid size-8 shrink-0 place-items-center rounded-lg",
                        TILES[levelTones[item.event.level]],
                      ]}
                    >
                      <Icon icon={categoryIcons[item.event.category]} class="size-4" />
                      <span class="sr-only">{strings.events.levelName[item.event.level]}</span>
                    </span>
                    <span class="flex min-w-0 flex-1 flex-col">
                      <span class="truncate font-medium text-fg text-sm">
                        {item.described.title}
                      </span>
                      <Show when={item.described.detail}>
                        {(detail) => (
                          <span class="truncate text-fg-subtle text-xs">{detail()}</span>
                        )}
                      </Show>
                    </span>
                    <time
                      datetime={new Date(item.event.at).toISOString()}
                      class="shrink-0 font-mono text-fg-subtle text-xs tabular-nums"
                    >
                      {shortTime(item.event.at)}
                    </time>
                  </a>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </Show>
    </Card>
  );
}
