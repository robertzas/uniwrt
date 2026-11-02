/**
 * The page's events: stored pages from `GET v1/events`, newest first, and for a live range what
 * the WS topic `events` announces after them, new or folded (a fold replaces its record, which
 * keeps its place by when it first happened).
 */
import { createEffect, createMemo, createSignal } from "solid-js";
import type { EventRecord } from "../../api/gen/EventRecord";
import type { ServerMessage } from "../../api/gen/ServerMessage";
import { api } from "../../lib/api";
import { admits } from "../../lib/events";
import type { Live } from "../../lib/live";
import { type EventQuery, isLive, topicOf, windowOf } from "./query";

/** Events per page read. */
export const PAGE = 100;
/** The most events the page keeps; narrower filters reach further back. */
export const MAX_EVENTS = 5_000;

/** Newest first: by when it happened, then by when it was recorded. */
export function compareEvents(a: EventRecord, b: EventRecord): number {
  return b.at - a.at || b.id - a.id;
}

/** `incoming` merged into `current` (newest first): a known id is replaced, a new one placed. */
export function mergeEvents(
  current: readonly EventRecord[],
  incoming: readonly EventRecord[],
): EventRecord[] {
  if (incoming.length === 0) return current as EventRecord[];
  const byId = new Map(current.map((event) => [event.id, event]));
  for (const event of incoming) byId.set(event.id, event);
  return [...byId.values()].sort(compareEvents);
}

type Status = "loading" | "ready" | "failed";

interface FeedState {
  key: string;
  events: readonly EventRecord[];
  next: string | null;
  status: Status;
  older: "idle" | "loading" | "failed";
}

export interface EventFeed {
  events(): readonly EventRecord[];
  status(): Status;
  hasOlder(): boolean;
  capped(): boolean;
  olderStatus(): "idle" | "loading" | "failed";
  loadOlder(): void;
  live(): boolean;
  retry(): void;
}

interface Session {
  key: string;
  controller: AbortController;
  query: EventQuery;
  window: { from: number; to: number | null };
  /** Arrivals before the first page came. */
  early: EventRecord[] | undefined;
  snapshots: number;
}

/** The events `query` finds, live while its range is, for as long as the caller lives. */
export function createEventFeed(live: Live, query: () => EventQuery): EventFeed {
  const key = createMemo(() => JSON.stringify(query()));
  const [state, setState] = createSignal<FeedState | undefined>();
  const [attempt, setAttempt] = createSignal(0);
  let session: Session | undefined;

  const current = () => {
    const value = state();
    return value?.key === key() ? value : undefined;
  };
  const patch = (s: Session, change: (state: FeedState) => Partial<FeedState>) => {
    if (session !== s || s.controller.signal.aborted) return;
    setState((prev) => {
      const base: FeedState =
        prev?.key === s.key
          ? prev
          : { key: s.key, events: [], next: null, status: "loading", older: "idle" };
      return { ...base, ...change(base) };
    });
  };
  const filterOf = (s: Session) => ({ ...s.query.filter, ...s.window });
  const admitted = (s: Session, events: readonly EventRecord[]) =>
    events.filter((event) => admits(filterOf(s), event));

  const arrive = (s: Session, incoming: readonly EventRecord[]) => {
    if (s.early !== undefined) {
      s.early.push(...incoming);
      return;
    }
    if (incoming.length === 0) return;
    patch(s, (st) => {
      const merged = mergeEvents(st.events, incoming);
      if (merged.length <= MAX_EVENTS) return { events: merged };
      const kept = merged.slice(0, MAX_EVENTS);
      const last = kept.at(-1);
      return { events: kept, next: last ? `${last.at}:${last.id}` : st.next };
    });
  };

  /** Reads the newest page: the first time, or again after a reconnect (which may miss some). */
  const read = async (s: Session, again: boolean) => {
    try {
      const page = await api.events.page(filterOf(s), null, PAGE, s.controller.signal);
      if (session !== s) return;
      if (again) {
        arrive(s, page.events);
        return;
      }
      const early = s.early ?? [];
      s.early = undefined;
      patch(s, () => ({ events: page.events, next: page.next, status: "ready" }));
      arrive(s, early);
    } catch {
      if (!again) patch(s, () => ({ status: "failed" }));
    }
  };

  createEffect(
    () => [key(), attempt()] as const,
    ([text]) => {
      const parsed = JSON.parse(text) as EventQuery;
      const s: Session = {
        key: text,
        controller: new AbortController(),
        query: parsed,
        window: windowOf(parsed.range, Date.now()),
        early: [],
        snapshots: 0,
      };
      session = s;
      const onMessage = (message: ServerMessage) => {
        if (message.type === "events_snapshot") {
          s.snapshots += 1;
          if (s.snapshots > 1 && s.early === undefined) void read(s, true);
          arrive(s, admitted(s, message.events));
        } else if (message.type === "events_delta") {
          arrive(s, admitted(s, message.events));
        }
      };
      const unsubscribe = isLive(parsed.range)
        ? live.socket.subscribe(topicOf(parsed.filter), onMessage)
        : undefined;
      void read(s, false);
      return () => {
        s.controller.abort();
        unsubscribe?.();
        if (session === s) session = undefined;
      };
    },
  );

  return {
    events: () => current()?.events ?? [],
    status: () => current()?.status ?? "loading",
    hasOlder: () => {
      const st = current();
      return st !== undefined && st.next !== null && st.events.length < MAX_EVENTS;
    },
    capped: () => {
      const st = current();
      return st !== undefined && st.next !== null && st.events.length >= MAX_EVENTS;
    },
    olderStatus: () => current()?.older ?? "idle",
    loadOlder: () => {
      const s = session;
      const st = current();
      if (s === undefined || st === undefined || st.status !== "ready") return;
      if (st.next === null || st.older === "loading" || st.events.length >= MAX_EVENTS) return;
      const before = st.next;
      patch(s, () => ({ older: "loading" }));
      api.events.page(filterOf(s), before, PAGE, s.controller.signal).then(
        (page) =>
          patch(s, (now) =>
            now.next === before
              ? { events: mergeEvents(now.events, page.events), next: page.next, older: "idle" }
              : { older: "idle" },
          ),
        () => patch(s, () => ({ older: "failed" })),
      );
    },
    live: () => isLive(query().range),
    retry: () => setAttempt((n) => n + 1),
  };
}
