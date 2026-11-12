import type { TopoEdge } from "../../api/gen/TopoEdge";
import type { Graph } from "./graph";
import type { Scene } from "./scene";

/**
 * Reading the mesh's health off the topology (features/mesh.md, Telemetry & health): the
 * server scores each link and names each vertex's `depends_on`, the nearest node, switch or
 * peer it can't lose; this module turns those into what the map highlights.
 */

/** The vertices some other vertex can't lose: the network's single points of failure. */
export function articulations(graph: Graph): Set<string> {
  const found = new Set<string>();
  for (const node of graph.nodes.values()) if (node.depends_on) found.add(node.depends_on);
  return found;
}

/** What a vertex's loss cuts off from the gateway, and how many of each. */
export interface Impact {
  /** The vertex taken away. */
  gone: string;
  /** Every vertex cut off: nodes, switches and peers whose `depends_on` chain reaches it, and the clients of any of them (or of it). */
  lost: Set<string>;
  /** Adopted nodes among them. */
  devices: number;
  clients: number;
}

/** What goes with `id` if it goes down. */
export function impactOf(graph: Graph, id: string): Impact {
  const lost = new Set<string>();
  const reaches = (from: string | null, seen: Set<string>): boolean => {
    let at = from;
    while (at && !seen.has(at)) {
      if (at === id) return true;
      seen.add(at);
      at = graph.nodes.get(at)?.depends_on ?? null;
    }
    return false;
  };
  for (const node of graph.nodes.values()) {
    if (node.id !== id && reaches(node.depends_on, new Set())) lost.add(node.id);
  }
  // Clients relay nothing, so the server doesn't ask what they depend on: they go with their host.
  for (const edge of graph.edges.values()) {
    const a = graph.nodes.get(edge.a);
    const b = graph.nodes.get(edge.b);
    if (b?.kind === "client" && (edge.a === id || lost.has(edge.a))) lost.add(b.id);
    else if (a?.kind === "client" && (edge.b === id || lost.has(edge.b))) lost.add(a.id);
  }
  let devices = 0;
  let clients = 0;
  for (const vertex of lost) {
    const node = graph.nodes.get(vertex);
    if (node?.kind === "client") clients += 1;
    else if (node?.node != null) devices += 1;
  }
  return { gone: id, lost, devices, clients };
}

/** Whether asking what a vertex's loss cuts off means anything: a node, switch or peer that isn't a gateway. */
export function canGoDown(graph: Graph, id: string): boolean {
  const kind = graph.nodes.get(id)?.kind;
  return kind === "ap" || kind === "bridge" || kind === "switch" || kind === "peer";
}

/**
 * What the map stresses: a what-if dims all but the vertex gone and what it cuts off; weak
 * spots ring the single points of failure and halo the critical links.
 */
export type Emphasis =
  | { kind: "impact"; gone: string; lost: ReadonlySet<string> }
  | { kind: "weak"; vertices: ReadonlySet<string> };

/** The impact in the scene's terms: a cluster is lost when its host is. */
export function impactEmphasis(scene: Scene, impact: Impact): Emphasis {
  const lost = new Set(impact.lost);
  for (const vertex of scene.vertices) {
    if (
      vertex.kind === "cluster" &&
      vertex.host &&
      (vertex.host === impact.gone || lost.has(vertex.host))
    ) {
      lost.add(vertex.id);
    }
  }
  return { kind: "impact", gone: impact.gone, lost };
}

/** The link's health, 0–100: its score where the server gives one, else its quality. */
export function linkHealth(edge: TopoEdge): number | null {
  return edge.score ?? edge.quality;
}
