/**
 * The alert history: stored pages from `GET v1/alerts`, newest first by start, and what the WS
 * topic `alerts` announces after them (an alert opened, changed, acknowledged or resolved replaces
 * its record by id).
 */
import { createEffect, createMemo, createSignal } from "solid-js";
import type { AlertRecord } from "../../api/gen/AlertRecord";
import type { EventLevel } from "../../api/gen/EventLevel";
import type { ServerMessage } from "../../api/gen/ServerMessage";
import { compareAlerts, TOPIC } from "../../lib/alerts";
import { type AlertFilter, api } from "../../lib/api";
import { isLevel, LEVELS } from "../../lib/events";
import type { Live } from "../../lib/live";

/** Alerts per page read. */
export const PAGE = 100;
/** The most alerts the tab keeps; narrower filters reach further back. */
export const MAX_ALERTS = 2_000;

export const RANGES = ["24h", "7d", "30d", "90d"] as const;
export type HistoryRange = (typeof RANGES)[number];
const RANGE_MS: Record<HistoryRange, number> = {
  "24h": 86_400_000,
  "7d": 7 * 86_400_000,
  "30d": 30 * 86_400_000,
  "90d": 90 * 86_400_000,
};

/** What the history shows: the least level, one device or all, and how far back they started. */
export interface HistoryQuery {
  min: EventLevel | null;
  node: number | null;
  range: HistoryRange;
}

/** The URL's parameters (`?tab=history&min=warning&node=3&range=30d`) as a query. */
export function parseHistory(params: {
  min?: string | undefined;
  node?: string | undefined;
  range?: string | undefined;
}): HistoryQuery {
  const node = Number(params.node);
  return {
    min: isLevel(params.min) ? params.min : null,
    node: Number.isInteger(node) && node > 0 ? node : null,
    range: (RANGES as readonly string[]).includes(params.range ?? "")
      ? (params.range as HistoryRange)
      : "7d",
  };
}

/** The filter `GET v1/alerts` takes for `query`, as of `now`. */
export function filterOf(query: HistoryQuery, now: number): AlertFilter {
  return {
    active: false,
    min: query.min,
    nodes: query.node === null ? [] : [query.node],
    rules: [],
    from: now - RANGE_MS[query.range],
    to: null,
  };
}

/** Whether `alert` passes `filter`, as the store decides (`AlertFilter::admits`). */
export function admits(filter: AlertFilter, alert: AlertRecord): boolean {
  const rank = (level: EventLevel) => LEVELS.indexOf(level);
  return (
    (!filter.active || alert.resolved_at === null) &&
    (filter.min === null || rank(alert.level) <= rank(filter.min)) &&
    (filter.nodes.length === 0 || (alert.node !== null && filter.nodes.includes(alert.node))) &&
    (filter.rules.length === 0 || filter.rules.includes(alert.rule)) &&
    (filter.from === null || alert.started_at >= filter.from) &&
    (filter.to === null || alert.started_at < filter.to)
  );
}

/** `incoming` merged into `current` (newest first): a known id is replaced, a new one placed. */
export function mergeAlerts(
  current: readonly AlertRecord[],
  incoming: readonly AlertRecord[],
): AlertRecord[] {
  if (incoming.length === 0) return current as AlertRecord[];
  const byId = new Map(current.map((alert) => [alert.id, alert]));
  for (const alert of incoming) byId.set(alert.id, alert);
  return [...byId.values()].sort(compareAlerts);
}

type Status = "loading" | "ready" | "failed";

interface HistoryState {
  key: string;
  alerts: readonly AlertRecord[];
  next: string | null;
  status: Status;
  older: "idle" | "loading" | "failed";
}

export interface AlertHistory {
  alerts(): readonly AlertRecord[];
  status(): Status;
  hasOlder(): boolean;
  olderStatus(): "idle" | "loading" | "failed";
  loadOlder(): void;
  retry(): void;
}

interface Session {
  key: string;
  controller: AbortController;
  filter: AlertFilter;
  /** Arrivals before the first page came. */
  early: AlertRecord[] | undefined;
}

/** The alerts `query` finds, kept current by the `alerts` topic, while the caller lives. */
export function createAlertHistory(live: Live, query: () => HistoryQuery): AlertHistory {
  const key = createMemo(() => JSON.stringify(query()));
  const [state, setState] = createSignal<HistoryState | undefined>();
  const [attempt, setAttempt] = createSignal(0);
  let session: Session | undefined;

  const current = () => {
    const value = state();
    return value?.key === key() ? value : undefined;
  };
  const patch = (s: Session, change: (state: HistoryState) => Partial<HistoryState>) => {
    if (session !== s || s.controller.signal.aborted) return;
    setState((prev) => {
      const base: HistoryState =
        prev?.key === s.key
          ? prev
          : { key: s.key, alerts: [], next: null, status: "loading", older: "idle" };
      return { ...base, ...change(base) };
    });
  };
  const arrive = (s: Session, incoming: readonly AlertRecord[]) => {
    const admitted = incoming.filter((alert) => admits(s.filter, alert));
    if (s.early !== undefined) {
      s.early.push(...admitted);
      return;
    }
    if (admitted.length === 0) return;
    patch(s, (st) => ({ alerts: mergeAlerts(st.alerts, admitted).slice(0, MAX_ALERTS) }));
  };

  createEffect(
    () => [key(), attempt()] as const,
    ([text]) => {
      const s: Session = {
        key: text,
        controller: new AbortController(),
        filter: filterOf(JSON.parse(text) as HistoryQuery, Date.now()),
        early: [],
      };
      session = s;
      const unsubscribe = live.socket.subscribe(TOPIC, (message: ServerMessage) => {
        if (message.type === "alerts_snapshot" || message.type === "alerts_delta") {
          arrive(s, message.alerts);
        }
      });
      api.alerts.page(s.filter, null, PAGE, s.controller.signal).then(
        (page) => {
          if (session !== s) return;
          const early = s.early ?? [];
          s.early = undefined;
          patch(s, () => ({ alerts: page.alerts, next: page.next, status: "ready" }));
          arrive(s, early);
        },
        () => patch(s, () => ({ status: "failed" })),
      );
      return () => {
        s.controller.abort();
        unsubscribe();
        if (session === s) session = undefined;
      };
    },
  );

  return {
    alerts: () => current()?.alerts ?? [],
    status: () => current()?.status ?? "loading",
    hasOlder: () => {
      const st = current();
      return st !== undefined && st.next !== null && st.alerts.length < MAX_ALERTS;
    },
    olderStatus: () => current()?.older ?? "idle",
    loadOlder: () => {
      const s = session;
      const st = current();
      if (s === undefined || st === undefined || st.status !== "ready") return;
      if (st.next === null || st.older === "loading") return;
      const before = st.next;
      patch(s, () => ({ older: "loading" }));
      api.alerts.page(s.filter, before, PAGE, s.controller.signal).then(
        (page) =>
          patch(s, (now) =>
            now.next === before
              ? { alerts: mergeAlerts(now.alerts, page.alerts), next: page.next, older: "idle" }
              : { older: "idle" },
          ),
        () => patch(s, () => ({ older: "failed" })),
      );
    },
    retry: () => setAttempt((n) => n + 1),
  };
}
