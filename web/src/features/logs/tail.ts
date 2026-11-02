/**
 * The page's entries: stored pages from `GET v1/logs`, newest first, and for a live range the
 * entries the WS topic `logs` announces after them. While the tail is paused, or the reader has
 * scrolled away from the newest, arrivals are held and counted; releasing them shows them.
 */
import { createEffect, createMemo, createSignal } from "solid-js";
import type { LogRecord } from "../../api/gen/LogRecord";
import type { ServerMessage } from "../../api/gen/ServerMessage";
import { api } from "../../lib/api";
import type { Live } from "../../lib/live";
import { admitter } from "./match";
import { isLive, type LogQuery, topicOf, windowOf, withWindow } from "./query";

/** Entries per page read. */
export const PAGE = 200;
/** The most entries the page keeps; narrower filters reach further back. */
export const MAX_RECORDS = 10_000;
/** Held arrivals beyond this are counted only, and releasing them reads the newest again. */
const MAX_HELD = 5_000;

/** Newest first: by the time logged, then by arrival. */
export function compareRecords(a: LogRecord, b: LogRecord): number {
  return b.time - a.time || b.id - a.id;
}

/** The `before` cursor that reads the entries after `record` (the API's `<time>:<id>`). */
export function cursorOf(record: LogRecord): string {
  return `${record.time}:${record.id}`;
}

/**
 * `incoming` merged into `current` (both newest first), each id once. With `bound`, the oldest
 * entry shown while older ones are still unread, nothing older than it is taken: it would sit
 * past entries not read yet.
 */
export function mergeRecords(
  current: readonly LogRecord[],
  incoming: readonly LogRecord[],
  bound?: LogRecord,
): LogRecord[] {
  const known = new Set(current.map((record) => record.id));
  const fresh = incoming.filter(
    (record) => !known.has(record.id) && (bound === undefined || compareRecords(record, bound) < 0),
  );
  if (fresh.length === 0) return current as LogRecord[];
  const unique = [...new Map(fresh.map((record) => [record.id, record])).values()];
  unique.sort(compareRecords);
  const first = current[0];
  const last = unique.at(-1);
  // Arrivals are usually all newer than what is shown: no need to sort everything again.
  if (first === undefined || (last !== undefined && compareRecords(last, first) < 0)) {
    return [...unique, ...current];
  }
  return [...current, ...unique].sort(compareRecords);
}

type Status = "loading" | "ready" | "failed";

interface TailState {
  key: string;
  records: readonly LogRecord[];
  /** The cursor of the next older page, when there may be one. */
  next: string | null;
  status: Status;
  older: "idle" | "loading" | "failed";
  /** Arrivals held while paused or scrolled away. */
  held: number;
}

export interface LogTail {
  records(): readonly LogRecord[];
  status(): Status;
  /** Older entries may be read (`loadOlder`). */
  hasOlder(): boolean;
  /** The page keeps the most entries it may: narrow the filter to read further back. */
  capped(): boolean;
  olderStatus(): "idle" | "loading" | "failed";
  loadOlder(): void;
  /** The range is live: new entries arrive. */
  live(): boolean;
  paused(): boolean;
  setPaused(paused: boolean): void;
  /** The reader is at the newest entries, which then show as they arrive. */
  following(): boolean;
  setFollowing(following: boolean): void;
  /** How many arrivals are held. */
  held(): number;
  /** Reads the first page again. */
  retry(): void;
}

interface Session {
  key: string;
  controller: AbortController;
  query: LogQuery;
  window: { from: number; to: number | null };
  admit: (record: LogRecord) => boolean;
  /** Arrivals before the first page came. */
  early: LogRecord[] | undefined;
  held: Map<number, LogRecord>;
  /** More arrived than were held: releasing reads the newest page again. */
  overflow: boolean;
  snapshots: number;
}

/** The entries `query` finds, live while its range is, for as long as the caller lives. */
export function createLogTail(live: Live, query: () => LogQuery): LogTail {
  const key = createMemo(() => JSON.stringify(query()));
  const [state, setState] = createSignal<TailState | undefined>();
  const [paused, setPausedSignal] = createSignal(false);
  const [following, setFollowingSignal] = createSignal(true);
  const [attempt, setAttempt] = createSignal(0);
  let session: Session | undefined;

  const current = () => {
    const value = state();
    return value?.key === key() ? value : undefined;
  };
  const patch = (s: Session, change: (state: TailState) => Partial<TailState>) => {
    if (session !== s || s.controller.signal.aborted) return;
    setState((prev) => {
      const base: TailState = prev?.key === s.key ? prev : blank(s.key);
      return { ...base, ...change(base) };
    });
  };
  const holding = () => paused() || !following();

  /** Shows entries: merged (or, after a gap, in place of what was shown), within the cap. */
  const show = (s: Session, incoming: readonly LogRecord[], gap = false) =>
    patch(s, (st) => {
      if (gap) return { records: incoming.slice(0, MAX_RECORDS), next: cursorAt(incoming) };
      const bound = st.next === null ? undefined : st.records.at(-1);
      const merged = mergeRecords(st.records, incoming, bound);
      if (merged.length <= MAX_RECORDS) return { records: merged };
      const kept = merged.slice(0, MAX_RECORDS);
      const last = kept.at(-1);
      return { records: kept, next: last ? cursorOf(last) : st.next };
    });

  const hold = (s: Session, incoming: readonly LogRecord[]) => {
    const shown = new Set((current()?.records ?? []).map((record) => record.id));
    for (const record of incoming) {
      if (shown.has(record.id) || s.held.has(record.id)) continue;
      if (s.held.size >= MAX_HELD) s.overflow = true;
      else s.held.set(record.id, record);
    }
    patch(s, () => ({ held: s.held.size + (s.overflow ? 1 : 0) }));
  };

  const arrive = (s: Session, incoming: readonly LogRecord[]) => {
    if (s.early !== undefined) {
      s.early.push(...incoming);
      return;
    }
    if (incoming.length === 0) return;
    if (holding()) hold(s, incoming);
    else show(s, incoming);
  };

  /** Reads the newest page: the first time, or again after the socket may have missed some. */
  const read = async (s: Session, again: boolean) => {
    const filter = withWindow(s.query.filter, s.window);
    try {
      const page = await api.logs.page(filter, null, PAGE, s.controller.signal);
      if (session !== s) return;
      if (!again) {
        const early = s.early ?? [];
        s.early = undefined;
        patch(s, () => ({ records: page.records, next: page.next, status: "ready" }));
        arrive(s, early);
        return;
      }
      const newest = current()?.records[0];
      const oldest = page.records.at(-1);
      const gap =
        page.next !== null &&
        newest !== undefined &&
        oldest !== undefined &&
        compareRecords(oldest, newest) < 0;
      if (gap && holding()) {
        s.overflow = true;
        patch(s, () => ({ held: s.held.size + 1 }));
      } else if (gap) show(s, page.records, true);
      else arrive(s, page.records);
    } catch {
      if (!again) patch(s, () => ({ status: "failed" }));
    }
  };

  const release = () => {
    const s = session;
    if (s === undefined || (s.held.size === 0 && !s.overflow)) return;
    const held = [...s.held.values()];
    const overflow = s.overflow;
    s.held.clear();
    s.overflow = false;
    patch(s, () => ({ held: 0 }));
    if (overflow) void read(s, true);
    else show(s, held);
  };

  createEffect(
    () => [key(), attempt()] as const,
    ([text]) => {
      const parsed = JSON.parse(text) as LogQuery;
      const window = windowOf(parsed.range, Date.now());
      const s: Session = {
        key: text,
        controller: new AbortController(),
        query: parsed,
        window,
        admit: admitter(parsed.filter, window),
        early: [],
        held: new Map(),
        overflow: false,
        snapshots: 0,
      };
      session = s;
      const onMessage = (message: ServerMessage) => {
        if (message.type === "log_snapshot") {
          s.snapshots += 1;
          // After a reconnect the snapshot holds only the newest 200: read the page again.
          if (s.snapshots > 1 && s.early === undefined) void read(s, true);
          arrive(s, message.entries.filter(s.admit));
        } else if (message.type === "log_delta") {
          arrive(s, message.entries.filter(s.admit));
          if (message.dropped > 0 && s.early === undefined) void read(s, true);
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
    records: () => current()?.records ?? [],
    status: () => current()?.status ?? "loading",
    hasOlder: () => {
      const st = current();
      return st !== undefined && st.next !== null && st.records.length < MAX_RECORDS;
    },
    capped: () => {
      const st = current();
      return st !== undefined && st.next !== null && st.records.length >= MAX_RECORDS;
    },
    olderStatus: () => current()?.older ?? "idle",
    loadOlder: () => {
      const s = session;
      const st = current();
      if (s === undefined || st === undefined || st.status !== "ready") return;
      if (st.next === null || st.older === "loading" || st.records.length >= MAX_RECORDS) return;
      const before = st.next;
      patch(s, () => ({ older: "loading" }));
      const filter = withWindow(s.query.filter, s.window);
      api.logs.page(filter, before, PAGE, s.controller.signal).then(
        (page) =>
          patch(s, (now) =>
            now.next === before
              ? { records: mergeRecords(now.records, page.records), next: page.next, older: "idle" }
              : { older: "idle" },
          ),
        () => patch(s, () => ({ older: "failed" })),
      );
    },
    live: () => isLive(query().range),
    paused,
    setPaused: (value) => {
      setPausedSignal(value);
      if (!value && following()) release();
    },
    following,
    setFollowing: (value) => {
      if (value === following()) return;
      setFollowingSignal(value);
      if (value && !paused()) release();
    },
    held: () => current()?.held ?? 0,
    retry: () => setAttempt((n) => n + 1),
  };
}

function blank(key: string): TailState {
  return { key, records: [], next: null, status: "loading", older: "idle", held: 0 };
}

/** After a gap the page in place of the list: older entries follow its last. */
function cursorAt(records: readonly LogRecord[]): string | null {
  const last = records.at(-1);
  return last === undefined ? null : cursorOf(last);
}
