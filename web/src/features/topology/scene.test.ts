import { describe, expect, it } from "vitest";
import type { TopoNode } from "../../api/gen/TopoNode";
import { house } from "../../test/topology";
import { applyTopology, type Graph, graphOf } from "./graph";
import { buildScene, clusterId, sceneCounts, type ViewKind } from "./scene";

const names = new Map([[1, "Gateway"]]);
const options = (clustered: boolean) => ({
  clustered: () => clustered,
  nodeName: (node: { node: number | null }) =>
    node.node === null ? undefined : names.get(node.node),
});
const ids = (graph: Graph, view: ViewKind, clustered = false) =>
  buildScene(graph, view, options(clustered)).vertices.map((v) => v.id);

describe("the topology graph", () => {
  it("applies a snapshot, then deltas by id", () => {
    const graph: Graph = { at: 0, nodes: new Map(), edges: new Map() };
    expect(
      applyTopology(graph, { type: "topology_snapshot", topic: "topology", topology: house() }),
    ).toBe(true);
    expect(graph.nodes.size).toBe(7);
    const renamed = { ...(house().nodes[4] as TopoNode), name: "tablet" };
    applyTopology(graph, {
      type: "topology_delta",
      topic: "topology",
      delta: {
        at: 2_000,
        nodes: [renamed],
        edges: [],
        removed_nodes: ["client:aa:00:00:00:00:03"],
        removed_edges: ["node:1~client:aa:00:00:00:00:03"],
      },
    });
    expect(graph.at).toBe(2_000);
    expect(graph.nodes.get("client:aa:00:00:00:00:01")?.name).toBe("tablet");
    expect(graph.nodes.has("client:aa:00:00:00:00:03")).toBe(false);
    expect(graph.edges.size).toBe(5);
    expect(applyTopology(graph, { type: "jobs_delta", topic: "jobs", jobs: [] })).toBe(false);
  });
});

describe("scenes", () => {
  const graph = graphOf(house());

  it("name nodes as the user did, clients by hostname or MAC", () => {
    const scene = buildScene(graph, "clients", options(false));
    expect(scene.byId.get("node:1")?.label).toBe("Gateway");
    expect(scene.byId.get("node:2")?.label).toBe("ap");
    expect(scene.byId.get("internet")?.label).toBe("Internet");
    expect(scene.byId.get("client:aa:00:00:00:00:01")?.label).toBe("phone");
    expect(scene.byId.get("client:aa:00:00:00:00:02")?.label).toBe("aa:00:00:00:00:02");
    expect(scene.byId.get("client:aa:00:00:00:00:02")?.band).toBe("2g");
    expect(scene.byId.get("client:aa:00:00:00:00:03")?.band).toBeNull();
    expect(sceneCounts(graph)).toEqual({ devices: 2, clients: 3 });
  });

  it("draw the tree along uplinks only, grouping clients", () => {
    const scene = buildScene(graph, "tree", options(true));
    expect(scene.links.map((l) => l.id).sort()).toEqual(
      [
        "cluster:node:2~link",
        "internet~node:1",
        "node:1~client:aa:00:00:00:00:03",
        "node:1~node:2",
      ].sort(),
    );
    const cluster = scene.byId.get(clusterId("node:2"));
    expect(cluster?.members).toEqual(["client:aa:00:00:00:00:01", "client:aa:00:00:00:00:02"]);
    expect(cluster?.host).toBe("node:2");
    // One client is not a group.
    expect(scene.byId.has("client:aa:00:00:00:00:03")).toBe(true);
  });

  it("show the nodes, peers and every link between them in RF, without the internet or clients", () => {
    expect(ids(graph, "rf").sort()).toEqual(["node:1", "node:2", "peer:02:00:00:00:00:99"]);
    const scene = buildScene(graph, "rf", options(true));
    expect(scene.links.map((l) => l.id)).toContain("node:2~peer:02:00:00:00:00:99");
  });

  it("show every client in the clients view unless grouped", () => {
    expect(ids(graph, "clients")).toHaveLength(7);
    expect(ids(graph, "clients", true)).toContain(clusterId("node:2"));
  });
});
