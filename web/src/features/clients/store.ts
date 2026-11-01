import { createEffect, createStore, reconcile } from "solid-js";
import type { Client } from "../../api/gen/Client";
import type { ClientDetail } from "../../api/gen/ClientDetail";
import type { ClientSession } from "../../api/gen/ClientSession";
import type { ServerMessage } from "../../api/gen/ServerMessage";
import { clientName } from "../../lib/clients";
import type { Live } from "../../lib/live";

/** Every client as the `clients` topic keeps it, in the order of `compareClients`. */
export interface ClientsState {
  clients: Client[];
  /** The first snapshot arrived. */
  ready: boolean;
}

/** Online clients first, then those with a name (before bare MAC addresses), then by name. */
export function compareClients(a: Client, b: Client): number {
  const named = (client: Client) => Number(client.alias != null || client.hostname != null);
  return (
    Number(b.online) - Number(a.online) ||
    named(b) - named(a) ||
    clientName(a).localeCompare(clientName(b)) ||
    a.mac.localeCompare(b.mac)
  );
}

/**
 * Merges `clients` into the list in place: a client already listed keeps its object, so rows
 * re-render only the cells that changed.
 */
function mergeClients(
  state: ClientsState,
  clients: readonly Client[],
  keep: (mac: string) => boolean,
) {
  const incoming = new Map(clients.map((client) => [client.mac, client]));
  const next: Client[] = [];
  for (const client of state.clients) {
    const update = incoming.get(client.mac);
    if (update) {
      reconcile(update)(client);
      incoming.delete(client.mac);
    }
    if (update || keep(client.mac)) next.push(client);
  }
  next.push(...incoming.values());
  next.sort(compareClients);
  if (next.length !== state.clients.length || next.some((c, i) => c !== state.clients[i])) {
    state.clients.splice(0, state.clients.length, ...next);
  }
}

/** Applies a `clients` message to the state (a store draft or a plain object). */
export function applyClients(state: ClientsState, message: ServerMessage) {
  if (message.type === "clients_snapshot") {
    mergeClients(state, message.clients, () => false);
    state.ready = true;
  } else if (message.type === "clients_delta") {
    const removed = new Set(message.removed);
    mergeClients(state, message.clients, (mac) => !removed.has(mac));
  }
}

/** Every client, live, while the calling component lives. */
export function createClientsView(live: Live): ClientsState {
  const [state, setState] = createStore<ClientsState>({ clients: [], ready: false });
  createEffect(
    () => "clients",
    (topic) =>
      live.socket.subscribe(topic, (message) => {
        setState((draft) => applyClients(draft, message));
      }),
  );
  return state;
}

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

/** The mean score of the online Wi-Fi clients with one, and how many there are. */
export function averageScore(clients: readonly Client[]): { score: number | null; count: number } {
  const scores = clients
    .filter((client) => client.online)
    .map((client) => client.experience?.score)
    .filter((score): score is number => score != null);
  if (scores.length === 0) return { score: null, count: 0 };
  return {
    score: Math.round(scores.reduce((sum, score) => sum + score, 0) / scores.length),
    count: scores.length,
  };
}

/** The experience per access point: each node's online Wi-Fi clients, averaged; worst first. */
export function scoresByNode(
  clients: readonly Client[],
): { node: number; score: number; count: number }[] {
  const byNode = new Map<number, Client[]>();
  for (const client of clients) {
    const node = client.connection?.node;
    if (client.online && node != null && client.experience) {
      byNode.set(node, [...(byNode.get(node) ?? []), client]);
    }
  }
  return [...byNode]
    .map(([node, members]) => {
      const { score, count } = averageScore(members);
      return { node, score: score ?? 0, count };
    })
    .sort((a, b) => a.score - b.score || a.node - b.node);
}
