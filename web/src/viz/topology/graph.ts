import { type Accessor, createSignal, onSettled } from "solid-js";
import type { ServerMessage } from "../../api/gen/ServerMessage";
import type { TopoEdge } from "../../api/gen/TopoEdge";
import type { Topology } from "../../api/gen/Topology";
import type { TopoNode } from "../../api/gen/TopoNode";
import type { Live } from "../../lib/live";

/**
 * The topology by id. Plain maps rather than a store: the canvas redraws from them whole, and
 * the accessor that hands them out tells Solid when they changed.
 */
export interface Graph {
  at: number;
  nodes: Map<string, TopoNode>;
  edges: Map<string, TopoEdge>;
}

export function graphOf(topology: Topology): Graph {
  return {
    at: topology.at,
    nodes: new Map(topology.nodes.map((node) => [node.id, node])),
    edges: new Map(topology.edges.map((edge) => [edge.id, edge])),
  };
}

/** Applies a `topology` message to the graph; false for any other message. */
export function applyTopology(graph: Graph, message: ServerMessage): boolean {
  if (message.type === "topology_snapshot") {
    Object.assign(graph, graphOf(message.topology));
    return true;
  }
  if (message.type !== "topology_delta") return false;
  const delta = message.delta;
  graph.at = delta.at;
  for (const id of delta.removed_nodes) graph.nodes.delete(id);
  for (const id of delta.removed_edges) graph.edges.delete(id);
  for (const node of delta.nodes) graph.nodes.set(node.id, node);
  for (const edge of delta.edges) graph.edges.set(edge.id, edge);
  return true;
}

/** The live topology: `graph()` is undefined until the first snapshot, then a new view of it per change. */
export function createLiveGraph(live: Live): Accessor<Graph | undefined> {
  const graph: Graph = { at: 0, nodes: new Map(), edges: new Map() };
  const [current, setCurrent] = createSignal<Graph | undefined>(undefined, { equals: false });
  onSettled(() =>
    live.socket.subscribe("topology", (message) => {
      if (applyTopology(graph, message)) setCurrent({ ...graph });
    }),
  );
  return current;
}
