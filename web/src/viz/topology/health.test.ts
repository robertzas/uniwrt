import { describe, expect, it } from "vitest";
import { chain, edge, house, vertex } from "../../test/topology";
import { graphOf } from "./graph";
import { articulations, canGoDown, impactEmphasis, impactOf, linkHealth } from "./health";
import { buildScene } from "./scene";

describe("the mesh's health", () => {
  it("finds the vertices others can't lose", () => {
    expect(articulations(graphOf(chain()))).toEqual(new Set(["node:2"]));
    expect(articulations(graphOf(house()))).toEqual(new Set());
  });

  it("cuts off what depends on a vertex, along the chain, with their clients and its own", () => {
    const topology = chain();
    // A fourth access point behind ap3: it depends on ap3, which depends on the access point.
    topology.nodes.push(vertex("node:4", { kind: "ap", node: 4, depends_on: "node:3" }));
    const impact = impactOf(graphOf(topology), "node:2");
    expect(impact.lost).toEqual(
      new Set([
        "node:3",
        "node:4",
        "client:aa:00:00:00:00:01",
        "client:aa:00:00:00:00:02",
        "client:aa:00:00:00:00:04",
      ]),
    );
    expect([impact.devices, impact.clients]).toEqual([2, 3]);
    // ap3 takes only its own client.
    expect(impactOf(graphOf(topology), "node:3").lost).toEqual(
      new Set(["node:4", "client:aa:00:00:00:00:04"]),
    );
  });

  it("survives a loop in what the server names", () => {
    const topology = chain();
    topology.nodes = topology.nodes.map((node) =>
      node.id === "node:2" ? { ...node, depends_on: "node:3" } : node,
    );
    expect(impactOf(graphOf(topology), "peer:none").lost).toEqual(new Set());
  });

  it("asks only of nodes, switches and peers that aren't gateways", () => {
    const graph = graphOf(chain());
    expect(canGoDown(graph, "node:2")).toBe(true);
    expect(canGoDown(graph, "peer:02:00:00:00:00:99")).toBe(true);
    expect(canGoDown(graph, "node:1")).toBe(false);
    expect(canGoDown(graph, "internet")).toBe(false);
    expect(canGoDown(graph, "client:aa:00:00:00:00:04")).toBe(false);
  });

  it("loses a cluster with its host", () => {
    const graph = graphOf(chain());
    const scene = buildScene(graph, "tree", { clustered: () => true, nodeName: () => undefined });
    const emphasis = impactEmphasis(scene, impactOf(graph, "node:2"));
    expect(emphasis.kind === "impact" && emphasis.lost.has("cluster:node:2")).toBe(true);
  });

  it("rates a link by its score, else its quality", () => {
    expect(linkHealth(edge("a", "b", { score: 40, quality: 90 }))).toBe(40);
    expect(linkHealth(edge("a", "b", { quality: 90 }))).toBe(90);
    expect(linkHealth(edge("a", "b"))).toBeNull();
  });
});
