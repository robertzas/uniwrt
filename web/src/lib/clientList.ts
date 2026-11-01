/** The live list of clients (the `clients` topic) and what it adds up to. */
import { createEffect, createStore, reconcile } from "solid-js";
import type { Client } from "../api/gen/Client";
import type { ServerMessage } from "../api/gen/ServerMessage";
import { clientName } from "./clients";
import type { Live } from "./live";

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
