import { createEffect, createStore, reconcile } from "solid-js";
import type { Client } from "../../api/gen/Client";
import type { ClientDetail } from "../../api/gen/ClientDetail";
import type { ClientSession } from "../../api/gen/ClientSession";
import type { ServerMessage } from "../../api/gen/ServerMessage";
import type { Live } from "../../lib/live";

/** One client as the `client:<mac>` topic keeps it. */
export interface ClientViewState {
  detail: ClientDetail | null;
  ready: boolean;
  /** The hub knows no such client, or forgot it. */
  missing: boolean;
}

export function emptyClientView(): ClientViewState {
  return { detail: null, ready: false, missing: false };
}

/** How far back the samples go, as on the server. */
const SAMPLES_MS = 3_600_000;

/** Replaces each session with the same start, or adds it; oldest first. */
export function mergeSessions(sessions: ClientSession[], changed: readonly ClientSession[]) {
  for (const session of changed) {
    const index = sessions.findIndex((known) => known.start === session.start);
    if (index >= 0) sessions[index] = session;
    else sessions.push(session);
  }
  sessions.sort((a, b) => a.start - b.start);
}

/** Applies a `client:<mac>` message to the state (a store draft or a plain object). */
export function applyClient(state: ClientViewState, message: ServerMessage) {
  switch (message.type) {
    case "client_snapshot":
      state.detail = message.client;
      state.ready = true;
      state.missing = false;
      break;
    case "error":
      state.ready = true;
      state.missing = true;
      break;
    case "client_delta": {
      if (message.removed) state.missing = true;
      const detail = state.detail;
      if (!detail) break;
      if (message.client) reconcile(message.client)(detail.client);
      if (message.samples.length > 0) {
        detail.samples.push(...message.samples);
        const newest = detail.samples[detail.samples.length - 1]?.at ?? 0;
        const first = detail.samples.findIndex((sample) => sample.at >= newest - SAMPLES_MS);
        if (first > 0) detail.samples.splice(0, first);
      }
      if (message.sessions.length > 0) mergeSessions(detail.sessions, message.sessions);
      break;
    }
    default:
      break;
  }
}

/**
 * One client, live: subscribes to `client:<mac>` while the calling component lives (which makes
 * the engine collect its access point more often) and follows `mac` when it changes.
 */
export function createClientView(live: Live, mac: () => string): ClientViewState {
  const [view, setView] = createStore<ClientViewState>(emptyClientView());
  createEffect(mac, (address) => {
    setView(() => emptyClientView());
    return live.socket.subscribe(`client:${address}`, (message) => {
      setView((draft) => applyClient(draft, message));
    });
  });
  return view;
}

/** A row of the list: a client, and the other addresses of the same device (randomized ones). */
export interface ClientRow {
  client: Client;
  others: Client[];
}

/**
 * The list's rows: randomized addresses of one device (the same `group`) share a row, led by the
 * one online or seen last. Keeps the order of `clients`, at each row's leader.
 */
export function groupClients(clients: readonly Client[]): ClientRow[] {
  const groups = new Map<string, Client[]>();
  for (const client of clients) {
    if (client.group) groups.set(client.group, [...(groups.get(client.group) ?? []), client]);
  }
  const better = (c: Client, best: Client) =>
    c.online !== best.online ? c.online : c.last_seen > best.last_seen;
  const leader = (members: Client[]) => members.reduce((best, c) => (better(c, best) ? c : best));
  const rows: ClientRow[] = [];
  for (const client of clients) {
    const members = client.group ? (groups.get(client.group) ?? [client]) : [client];
    if (leader(members) !== client) continue;
    rows.push({ client, others: members.filter((member) => member !== client) });
  }
  return rows;
}
