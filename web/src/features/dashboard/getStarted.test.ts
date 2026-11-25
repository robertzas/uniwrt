import { describe, expect, it } from "vitest";
import { summary } from "../../test/fixtures";
import { edge, vertex } from "../../test/topology";
import { type Graph, graphOf } from "../../viz/topology/graph";
import { type GetStartedInput, getStarted } from "./getStarted";

const gateway = summary(1, "Gateway", { role: "gateway", mode: "managed" });
const ap = (id: number, extra = {}) => summary(id, `AP ${id}`, extra);

function graph(...edges: ReturnType<typeof edge>[]): Graph {
  return graphOf({
    at: 1,
    nodes: [vertex("node:1"), vertex("node:2"), vertex("node:3"), vertex("switch:1:lan1")],
    edges,
  });
}

const input = (extra: Partial<GetStartedInput> = {}): GetStartedInput => ({
  nodes: [gateway],
  graph: graph(),
  site: { wlans: 0, mesh: false },
  backups: false,
  notifiers: 0,
  ...extra,
});

const done = (extra: Partial<GetStartedInput>) =>
  Object.fromEntries(getStarted(input(extra))?.map((step) => [step.id, step.done]) ?? []);

describe("the Get started checklist", () => {
  it("waits until every step can be told", () => {
    expect(getStarted(input({ site: undefined }))).toBeUndefined();
    expect(getStarted(input({ backups: undefined }))).toBeUndefined();
    expect(getStarted(input({ notifiers: undefined }))).toBeUndefined();
  });

  it("starts with only the gateway, nothing done but its management", () => {
    expect(done({})).toEqual({
      aps: false,
      wifi: false,
      mesh: false,
      manage: true,
      backups: false,
      notify: false,
    });
  });

  it("ticks each step from its state", () => {
    const all = done({
      nodes: [gateway, ap(2, { mode: "managed" })],
      site: { wlans: 1, mesh: true },
      backups: true,
      notifiers: 2,
    });
    expect(Object.values(all).every(Boolean)).toBe(true);
  });

  it("counts the managed nodes until all are", () => {
    const steps = getStarted(input({ nodes: [gateway, ap(2), ap(3)] }));
    expect(steps?.find((step) => step.id === "manage")).toMatchObject({
      done: false,
      progress: { done: 1, total: 3 },
      href: "/devices",
    });
  });

  it("takes the mesh as connected when it's on, seen, or every access point has a cable", () => {
    const nodes = [gateway, ap(2), ap(3)];
    expect(done({ nodes }).mesh).toBe(false);
    expect(done({ nodes, site: { wlans: 0, mesh: true } }).mesh).toBe(true);
    expect(done({ nodes, graph: graph(edge("node:2", "node:3", { medium: "mesh" })) }).mesh).toBe(
      true,
    );
    const cabled = graph(
      edge("node:1", "node:2", { medium: "ethernet", up: true }),
      edge("switch:1:lan1", "node:3", { medium: "ethernet", up: true }),
    );
    expect(done({ nodes, graph: cabled }).mesh).toBe(true);
    const oneDown = graph(
      edge("node:1", "node:2", { medium: "ethernet", up: true }),
      edge("switch:1:lan1", "node:3", { medium: "ethernet", up: false }),
    );
    expect(done({ nodes, graph: oneDown }).mesh).toBe(false);
  });

  it("links each step to where it's taken", () => {
    expect(getStarted(input())?.map((step) => step.href)).toEqual([
      "/devices?adopt=1",
      "/wifi",
      "/wifi",
      "/devices",
      "/settings?tab=backups",
      "/settings?tab=notifications",
    ]);
  });
});
