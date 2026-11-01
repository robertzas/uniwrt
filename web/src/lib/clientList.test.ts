import { createRoot, createStore, flush } from "solid-js";
import { describe, expect, it } from "vitest";
import type { ServerMessage } from "../api/gen/ServerMessage";
import { client } from "../test/clients";
import { applyClients, averageScore, type ClientsState, scoresByNode } from "./clientList";

describe("the clients list", () => {
  it("merges deltas in place, online clients first", () => {
    const [state, setState] = createRoot(() =>
      createStore<ClientsState>({ clients: [], ready: false }),
    );
    const apply = (message: ServerMessage) => {
      setState((draft) => applyClients(draft, message));
      flush();
    };
    const anna = client("f0:18:98:00:00:01");
    const tv = client("f0:18:98:00:00:02", { hostname: "Living-Room-TV", online: false });
    apply({ type: "clients_snapshot", topic: "clients", clients: [tv, anna] });
    expect(state.ready).toBe(true);
    expect(state.clients.map((c) => c.hostname)).toEqual(["Annas-iPhone", "Living-Room-TV"]);
    const kept = state.clients[0];
    apply({
      type: "clients_delta",
      topic: "clients",
      clients: [
        { ...anna, alias: "Anna" },
        { ...tv, online: true },
      ],
      removed: [],
    });
    expect(state.clients[0]).toBe(kept);
    expect(kept?.alias).toBe("Anna");
    apply({
      type: "clients_delta",
      topic: "clients",
      clients: [],
      removed: [anna.mac],
    });
    expect(state.clients.map((c) => c.mac)).toEqual([tv.mac]);
  });

  it("averages the experience of online Wi-Fi clients, by access point", () => {
    const at = (index: number, node: number, score: number, online = true) => {
      const base = client(`f0:18:98:00:00:0${index}`);
      return {
        ...base,
        online,
        connection: base.connection && { ...base.connection, node },
        experience: { score, parts: [] },
      };
    };
    const clients = [at(1, 1, 90), at(2, 1, 70), at(3, 2, 40), at(4, 2, 10, false)];
    expect(averageScore(clients)).toEqual({ score: 67, count: 3 });
    expect(scoresByNode(clients)).toEqual([
      { node: 2, score: 40, count: 1 },
      { node: 1, score: 80, count: 2 },
    ]);
    expect(averageScore([])).toEqual({ score: null, count: 0 });
  });
});
