import { createEffect, createMemo, createSignal, For, Show, useContext } from "solid-js";
import type { ClientDetail } from "../../api/gen/ClientDetail";
import type { ClientSession } from "../../api/gen/ClientSession";
import { api } from "../../lib/api";
import { clientName, upstreamName } from "../../lib/clients";
import { describeEvent, type Names } from "../../lib/events";
import { formatDateTime, formatDuration, formatTime } from "../../lib/format";
import { LiveContext } from "../../lib/live";
import {
  createSteering,
  outcomeCounts,
  outcomeOf,
  outcomeTones,
  type Steered,
} from "../../lib/steering";
import { strings } from "../../lib/strings";
import { Card, CardHeader } from "../../ui/Card";
import { Segmented } from "../../ui/Segmented";
import { type Column, Table } from "../../ui/Table";
import { createNow } from "./columns";
import { mergeSessions } from "./store";

const t = strings.client.timeline;
const bands = strings.device.radios.band;

type Range = "day" | "week";
const RANGE_MS: Record<Range, number> = { day: 86_400_000, week: 7 * 86_400_000 };

/** Steering events listed under the lanes; the rest are on the events page. */
const STEERING_SHOWN = 10;

/** A place's lane: its key, label and sessions. */
interface Lane {
  key: string;
  label: string;
  sessions: ClientSession[];
}

export function placeKey(session: ClientSession): string {
  return `${session.node ?? "-"}|${session.port ?? ""}|${session.medium}`;
}

/** The lane of the access point (a node's BSS) a steering event happened at. */
export function steerKey(event: Steered): string {
  return `${event.node ?? "-"}|${event.what.iface}|wifi`;
}

/** A steering event in words: its title, and the detail after it. */
type Marked = { event: Steered; text: string };

/** One lane per place, in the order the client first went there. */
export function lanesOf(
  sessions: readonly ClientSession[],
  nodeName: (id: number) => string,
  peerName: string | null = null,
): Lane[] {
  const lanes = new Map<string, Lane>();
  for (const session of sessions) {
    const key = placeKey(session);
    let lane = lanes.get(key);
    if (!lane) {
      const node = upstreamName({ node: session.node, upstream_name: peerName }, nodeName);
      const where =
        session.medium === "wired"
          ? session.port
          : [session.ssid, session.band && bands[session.band]].filter(Boolean).join(" · ");
      lane = { key, label: where ? `${node} · ${where}` : node, sessions: [] };
      lanes.set(key, lane);
    }
    lane.sessions.push(session);
  }
  return [...lanes.values()];
}

/**
 * The Timeline tab: where the client connected and when, as a lane per access point (or port),
 * over the last day (live) or week (fetched), with marks where access points steered it (D-091),
 * then the steering in words and each session.
 */
export function ClientTimeline(props: { detail: ClientDetail }) {
  const live = useContext(LiveContext);
  const now = createNow();
  const [range, setRange] = createSignal<Range>("day");
  // The week is fetched into a plain signal (D-048), then kept current from the live sessions.
  const [week, setWeek] = createSignal<ClientSession[] | null>(null);
  createEffect(
    () => (range() === "week" ? props.detail.client.mac : null),
    (mac) => {
      if (!mac) return;
      const controller = new AbortController();
      setWeek(null);
      api.clients
        .sessions(mac, Date.now() - RANGE_MS.week, controller.signal)
        .then(setWeek)
        .catch(() => setWeek([]));
      return () => controller.abort();
    },
  );
  const nodeName = (id: number) =>
    live.fleet.nodes.find((node) => node.id === id)?.name ?? `#${id}`;
  const steering = createSteering(() => ({
    nodes: [],
    clients: [props.detail.client.mac],
    spanMs: RANGE_MS[range()],
  }));
  const names: Names = {
    node: nodeName,
    client: () => clientName(props.detail.client),
    pattern: () => undefined,
  };
  const marked = createMemo(() =>
    steering().map((event): Marked => {
      const described = describeEvent(event, names);
      const text = described.detail ? `${described.title} · ${described.detail}` : described.title;
      return { event, text };
    }),
  );
  const marks = createMemo(() => {
    const byLane = new Map<string, Marked[]>();
    for (const mark of marked()) {
      const key = steerKey(mark.event);
      byLane.set(key, [...(byLane.get(key) ?? []), mark]);
    }
    return byLane;
  });
  const sessions = createMemo(() => {
    if (range() === "day") return props.detail.sessions;
    const fetched = week();
    if (!fetched) return null;
    const merged = [...fetched];
    mergeSessions(merged, props.detail.sessions);
    return merged;
  });
  const from = () => now() - RANGE_MS[range()];
  const shown = () => (sessions() ?? []).filter((s) => (s.end ?? now()) >= from());
  // Sessions behind an unadopted node are named after the one the client is at now, if any.
  const lanes = createMemo(() =>
    lanesOf(shown(), nodeName, props.detail.client.connection?.upstream_name ?? null),
  );

  return (
    <Card class="min-w-0 overflow-hidden">
      <CardHeader
        title={t.title}
        actions={
          <Segmented
            label={t.range}
            value={range()}
            options={(["day", "week"] as const).map((value) => ({ value, label: t[value] }))}
            onChange={setRange}
          />
        }
      />
      <Show
        when={sessions()}
        fallback={
          <p role="status" class="px-4 py-10 text-center text-fg-subtle text-sm">
            {t.loading}…
          </p>
        }
      >
        <Show
          when={lanes().length > 0}
          fallback={<p class="px-4 py-10 text-center text-fg-subtle text-sm">{t.empty}</p>}
        >
          <Swimlanes
            lanes={lanes()}
            marks={marks()}
            from={from()}
            to={now()}
            range={range()}
            now={now()}
          />
          <Show when={marked().length > 0}>
            <SteeringList marked={marked()} mac={props.detail.client.mac} />
          </Show>
          <SessionTable sessions={shown()} lanes={lanes()} now={now()} />
        </Show>
      </Show>
    </Card>
  );
}

function Swimlanes(props: {
  lanes: Lane[];
  marks: Map<string, Marked[]>;
  from: number;
  to: number;
  range: Range;
  now: number;
}) {
  const span = () => props.to - props.from;
  const pct = (ms: number) => `${(Math.min(Math.max(ms - props.from, 0), span()) / span()) * 100}%`;
  const ticks = () => {
    const step = props.range === "day" ? 4 * 3_600_000 : 86_400_000;
    const first = Math.ceil(props.from / step) * step;
    const out: number[] = [];
    for (let at = first; at < props.to; at += step) out.push(at);
    return out;
  };
  const tickLabel = (ms: number) =>
    props.range === "day"
      ? formatTime(ms).replace(/:\d\d(?=\s|$)/, "")
      : new Date(ms).toLocaleDateString(undefined, { weekday: "short", day: "numeric" });
  return (
    <div class="overflow-x-auto px-4 pb-4">
      <div class="min-w-[36rem]">
        <For each={props.lanes}>
          {(lane) => (
            <div class="grid grid-cols-[12rem_minmax(0,1fr)] items-center gap-3 py-1">
              <span class="truncate text-sm" title={lane.label}>
                {lane.label}
              </span>
              <div class="relative h-6 rounded-control bg-raised">
                <For each={lane.sessions}>
                  {(session) => {
                    const end = () => session.end ?? props.now;
                    const label = () =>
                      t.session(
                        lane.label,
                        formatDateTime(session.start),
                        formatDuration((end() - session.start) / 1000),
                      );
                    return (
                      <span
                        role="img"
                        aria-label={label()}
                        title={label()}
                        class={[
                          "absolute inset-y-0.5 min-w-1 rounded-sm",
                          session.end == null ? "bg-accent" : "bg-accent/60",
                        ]}
                        style={{
                          left: pct(session.start),
                          width: `calc(${pct(end())} - ${pct(session.start)})`,
                        }}
                      />
                    );
                  }}
                </For>
                <For each={props.marks.get(lane.key) ?? []}>
                  {(mark) => {
                    const label = () => t.mark(formatDateTime(mark.event.at), mark.text);
                    return (
                      <span
                        role="img"
                        aria-label={label()}
                        title={label()}
                        class={[
                          "-translate-x-1/2 absolute inset-y-0 w-1 rounded-full ring-1 ring-surface",
                          outcomeTones[outcomeOf[mark.event.what.action]],
                        ]}
                        style={{ left: pct(mark.event.at) }}
                      />
                    );
                  }}
                </For>
              </div>
            </div>
          )}
        </For>
        <div
          class="relative mt-1 ml-[calc(12rem+0.75rem)] h-4 text-fg-subtle text-xs"
          aria-hidden="true"
        >
          <For each={ticks()}>
            {(tick) => (
              <span class="-translate-x-1/2 absolute whitespace-nowrap" style={{ left: pct(tick) }}>
                {tickLabel(tick)}
              </span>
            )}
          </For>
        </div>
      </div>
    </div>
  );
}

/** The steering in words, newest first: a dot of its outcome, when, and what happened. */
function SteeringList(props: { marked: Marked[]; mac: string }) {
  const steered = () => props.marked.map((mark) => mark.event);
  const summary = () =>
    outcomeCounts(steered())
      .map(([outcome, n]) => strings.events.steerOutcome[outcome](n))
      .join(" · ");
  const more = () => props.marked.length - STEERING_SHOWN;
  return (
    <section aria-label={t.steering} class="flex flex-col gap-2 border-edge border-t px-4 py-3">
      <h3 class="font-medium text-sm">
        {t.steering}
        <span class="font-normal text-fg-muted"> · {summary()}</span>
      </h3>
      <ul class="flex flex-col gap-1 text-sm">
        <For each={props.marked.slice(0, STEERING_SHOWN)}>
          {(mark) => (
            <li class="flex items-baseline gap-3">
              <span
                aria-hidden="true"
                class={[
                  "size-2 shrink-0 self-center rounded-full",
                  outcomeTones[outcomeOf[mark.event.what.action]],
                ]}
              />
              <span class="min-w-36 shrink-0 whitespace-nowrap text-fg-subtle tabular-nums">
                {formatDateTime(mark.event.at)}
              </span>
              <span class="min-w-0">{mark.text}</span>
            </li>
          )}
        </For>
      </ul>
      <Show when={more() > 0}>
        <a
          href={`/insights/events?clients=${encodeURIComponent(props.mac)}&categories=client`}
          class="self-start text-accent text-sm hover:underline"
        >
          {t.steeringMore(more())}
        </a>
      </Show>
    </section>
  );
}

function SessionTable(props: { sessions: ClientSession[]; lanes: Lane[]; now: number }) {
  const c = t.columns;
  const labels = () => new Map(props.lanes.map((lane) => [lane.key, lane.label]));
  const columns: Column<ClientSession>[] = [
    { id: "place", header: c.place, cell: (s) => labels().get(placeKey(s)) ?? "" },
    {
      id: "start",
      header: c.start,
      cell: (s) => formatDateTime(s.start),
      class: "whitespace-nowrap",
    },
    {
      id: "duration",
      header: c.duration,
      cell: (s) => formatDuration(((s.end ?? props.now) - s.start) / 1000),
      align: "end",
    },
    {
      id: "end",
      header: c.end,
      cell: (s) =>
        s.ended_by === "roamed" ? t.roamed : s.ended_by === "left" ? t.left : t.ongoing,
      align: "end",
    },
  ];
  const newest = () => [...props.sessions].reverse();
  return (
    <div class="border-edge border-t">
      <Table label={t.title} columns={columns} rows={newest()} rowKey={(s) => String(s.start)} />
    </div>
  );
}
