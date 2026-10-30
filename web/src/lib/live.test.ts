import { createRoot, createStore, flush } from "solid-js";
import { describe, expect, it } from "vitest";
import type { ServerMessage } from "../api/gen/ServerMessage";
import { detail, facts, summary } from "../test/fixtures";
import { applyFleet, applyNode, emptyNodeView, type FleetState, HISTORY } from "./live";

const delta = (fields: Partial<Extract<ServerMessage, { type: "node_delta" }>>): ServerMessage => ({
  type: "node_delta",
  topic: "node:1",
  events: [],
  roams: [],
  removed: false,
  ...fields,
});

describe("applyFleet", () => {
  it("sorts a snapshot gateway first, then by name, and keeps row identity across deltas", () => {
    const [fleet, setFleet, dispose] = createRoot(
      (dispose) => [...createStore<FleetState>({ nodes: [], ready: false }), dispose] as const,
    );
    const apply = (message: ServerMessage) => {
      setFleet((draft) => applyFleet(draft, message));
      flush();
    };
    apply({
      type: "fleet_snapshot",
      topic: "fleet",
      nodes: [summary(3, "office"), summary(1, "gw", { role: "gateway" }), summary(2, "attic")],
    });
    expect(fleet.ready).toBe(true);
    expect(fleet.nodes.map((n) => n.name)).toEqual(["gw", "attic", "office"]);
    const attic = fleet.nodes[1];

    apply({
      type: "fleet_delta",
      topic: "fleet",
      nodes: [summary(2, "attic", { cpu_percent: 42 }), summary(4, "basement")],
      removed: [3],
    });
    expect(fleet.nodes.map((n) => n.name)).toEqual(["gw", "attic", "basement"]);
    expect(fleet.nodes[1]).toBe(attic);
    expect(attic?.cpu_percent).toBe(42);
    dispose();
  });
});

describe("applyNode", () => {
  it("patches the parts of the facts that changed and samples CPU and memory", () => {
    const view = emptyNodeView();
    expect(
      applyNode(view, { type: "node_snapshot", topic: "node:1", node: detail(facts(10)) }),
    ).toBe(true);
    expect(view.ready).toBe(true);
    expect(view.cpu).toEqual([10]);
    expect(view.memory).toEqual([25]);

    const next = facts(20);
    applyNode(
      view,
      delta({
        status: { state: "degraded", since: 5, reasons: [{ kind: "no_agent" }] },
        facts: { collected_at: 2, system: next.system },
      }),
    );
    expect(view.detail?.status.state).toBe("degraded");
    expect(view.detail?.facts?.system.cpu_percent).toBe(20);
    expect(view.detail?.facts?.board.hostname).toBe("gw");
    expect(view.cpu).toEqual([10, 20]);
  });

  it("keeps a bounded history", () => {
    const view = emptyNodeView();
    applyNode(view, { type: "node_snapshot", topic: "node:1", node: detail(facts(0)) });
    for (let i = 1; i <= HISTORY + 5; i++) {
      applyNode(view, delta({ facts: { collected_at: i, system: facts(i).system } }));
    }
    expect(view.cpu).toHaveLength(HISTORY);
    expect(view.cpu.at(-1)).toBe(HISTORY + 5);
  });

  it("asks for a snapshot when a partial patch arrives before any facts", () => {
    const view = emptyNodeView();
    applyNode(view, { type: "node_snapshot", topic: "node:1", node: detail(null) });
    expect(applyNode(view, delta({ facts: { collected_at: 1, system: facts(1).system } }))).toBe(
      false,
    );
    const full = facts(3);
    expect(applyNode(view, delta({ facts: { ...full } }))).toBe(true);
    expect(view.detail?.facts?.system.cpu_percent).toBe(3);
  });

  it("marks a node the hub does not know, or stopped watching, as missing", () => {
    const view = emptyNodeView();
    applyNode(view, { type: "error", topic: "node:9", message: "no node 9" });
    expect(view).toMatchObject({ ready: true, missing: true });
    const watched = emptyNodeView();
    applyNode(watched, { type: "node_snapshot", topic: "node:1", node: detail(null) });
    applyNode(watched, delta({ removed: true }));
    expect(watched.missing).toBe(true);
  });
});
