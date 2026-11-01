import { createRoot, createStore, flush } from "solid-js";
import { describe, expect, it } from "vitest";
import type { ServerMessage } from "../../api/gen/ServerMessage";
import { client, detail, NOW, sample, session } from "../../test/clients";
import { applyClient, type ClientViewState, emptyClientView, groupClients } from "./store";

describe("the clients list's rows", () => {
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
