import { createRoot, createStore, flush } from "solid-js";
import { describe, expect, it } from "vitest";
import type { ServerMessage } from "../../api/gen/ServerMessage";
import { client, detail, NOW, sample, session } from "../../test/clients";
import {
  applyClient,
  applyClients,
  averageScore,
  type ClientsState,
  type ClientViewState,
  emptyClientView,
  groupClients,
  scoresByNode,
} from "./store";

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

  it("groups the randomized addresses of a device under the one online", () => {
    const old = client("5a:00:00:00:00:01", {
      randomized: true,
      group: "name:annas-pixel",
      online: false,
      last_seen: NOW - 1000,
    });
    const now = client("5a:00:00:00:00:02", { randomized: true, group: "name:annas-pixel" });
    const other = client("f0:18:98:00:00:03");
    const rows = groupClients([old, other, now]);
    expect(rows.map((row) => [row.client.mac, row.others.length])).toEqual([
      [other.mac, 0],
      [now.mac, 1],
    ]);
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

describe("one client", () => {
  it("takes the snapshot, then views, samples and sessions as they change", () => {
    const [state, setState] = createRoot(() => createStore<ClientViewState>(emptyClientView()));
    const apply = (message: ServerMessage) => {
      setState((draft) => applyClient(draft, message));
      flush();
    };
    apply({ type: "client_snapshot", topic: "client:x", client: detail() });
    expect(state.ready && !state.missing).toBe(true);
    const ended = { ...session(NOW - 3_600_000), end: NOW, ended_by: "left" as const };
    apply({
      type: "client_delta",
      topic: "client:x",
      client: { ...client("f0:18:98:00:00:01"), online: false },
      samples: [sample(NOW + 3_600_000, -70)],
      sessions: [ended, session(NOW + 100)],
      removed: false,
    });
    expect(state.detail?.client.online).toBe(false);
    // Samples older than an hour before the newest go.
    expect(state.detail?.samples.map((s) => s.signal_dbm)).toEqual([-70]);
    expect(state.detail?.sessions.map((s) => [s.start, s.ended_by])).toEqual([
      [NOW - 7_200_000, "roamed"],
      [NOW - 3_600_000, "left"],
      [NOW + 100, null],
    ]);
    apply({
      type: "client_delta",
      topic: "client:x",
      samples: [],
      sessions: [],
      removed: true,
    });
    expect(state.missing).toBe(true);
  });

  it("is missing when the hub refuses the topic", () => {
    const state = emptyClientView();
    applyClient(state, { type: "error", topic: "client:x", message: "There is no client" });
    expect(state).toMatchObject({ ready: true, missing: true });
  });
});
