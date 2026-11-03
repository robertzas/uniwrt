/**
 * Alerts live and in words (features/observability.md, Alerts): the WS topic `alerts` kept as a
 * store for the whole signed-in app (the top bar's bell, the alerts page, the dashboard), what
 * the bell counts, and each alert's subject, cause and pages in words. Shared by `app/` and the
 * features.
 */
import { Globe, RadioTower, Router, Smartphone, Waypoints } from "lucide";
import { createStore, reconcile } from "solid-js";
import type { AlertRecord } from "../api/gen/AlertRecord";
import type { EventLevel } from "../api/gen/EventLevel";
import type { ServerMessage } from "../api/gen/ServerMessage";
import { type Names as EntityNames, entityLabel, entityPage, parseEntity } from "./entities";
import { LEVELS } from "./events";
import { formatDuration } from "./format";
import { formatMetric, metricLabel } from "./metrics";
import { strings } from "./strings";
import type { LiveSocket } from "./ws";

const t = strings.alerts;

/** The topic of the alerts: those firing, then each one stored (opened, changed, resolved). */
export const TOPIC = "alerts";

/** The alerts firing, as the `alerts` topic keeps them, newest first. */
export interface AlertsState {
  active: AlertRecord[];
  /** The first snapshot arrived. */
  ready: boolean;
}

/** Newest first by start, then by id. */
export function compareAlerts(a: AlertRecord, b: AlertRecord): number {
  return b.started_at - a.started_at || b.id - a.id;
}

/**
 * Merges `alerts` into the list in place: an alert already listed keeps its object, a resolved
 * one leaves, and with `replace` so does every alert `alerts` doesn't name (a snapshot).
 */
function merge(state: AlertsState, alerts: readonly AlertRecord[], replace: boolean) {
  const incoming = new Map(alerts.map((alert) => [alert.id, alert]));
  const next: AlertRecord[] = [];
  for (const alert of state.active) {
    const update = incoming.get(alert.id);
    if (update) {
      incoming.delete(alert.id);
      if (update.resolved_at === null) {
        reconcile(update)(alert);
        next.push(alert);
      }
    } else if (!replace) next.push(alert);
  }
  for (const alert of incoming.values()) if (alert.resolved_at === null) next.push(alert);
  next.sort(compareAlerts);
  if (next.length !== state.active.length || next.some((alert, i) => alert !== state.active[i])) {
    state.active.splice(0, state.active.length, ...next);
  }
}

/** Applies an `alerts` message to the state (a store draft or a plain object). */
export function applyAlerts(state: AlertsState, message: ServerMessage) {
  if (message.type === "alerts_snapshot") {
    merge(state, message.alerts, true);
    state.ready = true;
  } else if (message.type === "alerts_delta") {
    merge(state, message.alerts, false);
  }
}

/** The active alerts over `socket`, subscribed to `alerts` for as long as the socket is open. */
export function createAlerts(socket: LiveSocket): AlertsState {
  const [alerts, setAlerts] = createStore<AlertsState>({ active: [], ready: false });
  socket.subscribe(TOPIC, (message) => {
    setAlerts((draft) => applyAlerts(draft, message));
  });
  return alerts;
}

/** The alerts that want attention: firing, neither acknowledged nor silenced. */
export function unattended(alerts: readonly AlertRecord[]): AlertRecord[] {
  return alerts.filter((alert) => alert.acknowledged === null && !alert.silenced);
}

/** The most severe level among `alerts`, if any. */
export function worstLevel(alerts: readonly AlertRecord[]): EventLevel | undefined {
  let worst: number | undefined;
  for (const alert of alerts) {
    const rank = LEVELS.indexOf(alert.level);
    if (worst === undefined || rank < worst) worst = rank;
  }
  return worst === undefined ? undefined : LEVELS[worst];
}

/** Attended last: what wants attention first, then the acknowledged, then the silenced. */
export function byAttention(alerts: readonly AlertRecord[]): AlertRecord[] {
  const rank = (alert: AlertRecord) => (alert.silenced ? 2 : alert.acknowledged ? 1 : 0);
  return [...alerts].sort(
    (a, b) =>
      rank(a) - rank(b) || LEVELS.indexOf(a.level) - LEVELS.indexOf(b.level) || compareAlerts(a, b),
  );
}

/** How to name what alerts are about. */
export type Names = EntityNames;

function nodeName(id: number, names: Names): string {
  return names.node(id) ?? strings.metrics.node(String(id));
}

/**
 * What an alert is about, in words: a series' entity (`ap-garden · radio1`), else the condition
 * it follows (`node:3/wan:eth1`, `node:3/mesh:<link>`, `node:3/offline`), else its node, client
 * or the network.
 */
export function subjectLabel(alert: AlertRecord, names: Names): string {
  const entity = parseEntity(alert.subject);
  if (entity) return entityLabel(entity, names);
  const mesh = /\/mesh:(.+)$/.exec(alert.subject)?.[1];
  const link = mesh === undefined ? undefined : parseEntity(`link:${mesh}`);
  if (link) return entityLabel(link, names);
  if (alert.node !== null) {
    const wan = /\/wan:(.+)$/.exec(alert.subject)?.[1];
    const node = nodeName(alert.node, names);
    return wan === undefined ? node : `${node} · ${wan}`;
  }
  if (alert.client !== null) return names.client(alert.client) ?? alert.client;
  return strings.metrics.site;
}

/** What made it fire: the metric beyond its threshold, or how many events matched. */
export function causeText(alert: AlertRecord): string | undefined {
  const cause = alert.cause;
  if (cause.type === "event") return cause.matches > 1 ? t.cause.matches(cause.matches) : undefined;
  const format = (value: number) => formatMetric(cause.metric, value);
  const text = t.cause.metric(
    metricLabel(cause.metric),
    format(cause.value),
    t.cause[cause.op],
    format(cause.threshold),
  );
  return cause.worst === cause.value ? text : `${text} · ${t.cause.worst(format(cause.worst))}`;
}

/** How long it has been firing (or fired, once resolved): `12m`, `3h 5m`. */
export function firingFor(alert: AlertRecord, now = Date.now()): string {
  return formatDuration(((alert.resolved_at ?? now) - alert.started_at) / 1000);
}

/** The page of what it is about: its device or client, if it has one. */
export function alertPage(alert: AlertRecord): string | undefined {
  const entity = parseEntity(alert.subject);
  const page = entity && entityPage(entity);
  if (page) return page;
  if (alert.node !== null) return `/devices/${alert.node}`;
  if (alert.client !== null) return `/clients/${encodeURIComponent(alert.client)}`;
  return undefined;
}

/** Before and after an alert, the events page shows what happened: 5 minutes on each side. */
const AROUND_MS = 5 * 60_000;

/** The events page around an alert, for its device or client when it has one. */
export function eventsAround(alert: AlertRecord, now = Date.now()): string {
  const params = new URLSearchParams({
    from: String(alert.started_at - AROUND_MS),
    to: String(Math.min(now, (alert.resolved_at ?? now) + AROUND_MS)),
  });
  if (alert.node !== null) params.set("nodes", String(alert.node));
  else if (alert.client !== null) params.set("clients", alert.client);
  return `/insights/events?${params}`;
}

/** An icon for what it is about (lucide data, rendered by `ui/Icon`). */
export function subjectIcon(alert: AlertRecord): typeof Router {
  const entity = parseEntity(alert.subject);
  if (entity?.kind === "radio") return RadioTower;
  if (entity?.kind === "link" || alert.subject.includes("/mesh:")) return Waypoints;
  if (entity?.kind === "client" || (alert.node === null && alert.client !== null)) {
    return Smartphone;
  }
  if (entity?.kind === "site" || alert.subject.includes("/wan:") || alert.node === null) {
    return Globe;
  }
  return Router;
}
