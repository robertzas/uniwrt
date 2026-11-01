import type { Band } from "../../api/gen/Band";
import type { Medium } from "../../api/gen/Medium";
import type { TopoEdge } from "../../api/gen/TopoEdge";
import type { TopoKind } from "../../api/gen/TopoKind";
import type { TopoNode } from "../../api/gen/TopoNode";
import { strings } from "../../lib/strings";
import type { Graph } from "./graph";

/**
 * Tree: the uplinks only, from the internet down. RF / mesh: the nodes, switches and peers, and
 * every link between them. Clients: everything, each access point's clients around it (features/observability.md).
 */
export type ViewKind = "tree" | "rf" | "clients";
export const viewKinds: readonly ViewKind[] = ["tree", "rf", "clients"];

/** A group of one host's clients, drawn as one vertex until expanded. */
export type VertexKind = TopoKind | "cluster";

export interface SceneVertex {
  id: string;
  kind: VertexKind;
  label: string;
  radius: number;
  /** The topology's vertex; `null` for a cluster. */
  node: TopoNode | null;
  /** A client's band, from its Wi-Fi link; `null` when wired. */
  band: Band | null;
  /** A cluster's clients, and the vertex they hang from. */
  members: readonly string[];
  host: string | null;
}

export interface SceneLink {
  id: string;
  /** The upstream end. */
  source: string;
  target: string;
  medium: Medium;
  /** The topology's edge; `null` for a cluster's link to its host. */
  edge: TopoEdge | null;
}

export interface Scene {
  view: ViewKind;
  vertices: SceneVertex[];
  links: SceneLink[];
  byId: Map<string, SceneVertex>;
}

export interface SceneOptions {
  /** Whether `host`'s clients are drawn as one cluster. */
  clustered: (host: string) => boolean;
  /** An adopted node's name, as the user named it. */
  nodeName: (node: TopoNode) => string | undefined;
}

/** A cluster of fewer would hide nothing. */
const MIN_CLUSTER = 2;

const radii: Record<VertexKind, number> = {
  internet: 20,
  gateway: 20,
  ap: 17,
  bridge: 15,
  switch: 12,
  peer: 13,
  client: 6,
  cluster: 11,
};

export function clusterId(host: string): string {
  return `cluster:${host}`;
}

/** What a vertex is called: the user's name for a node, a client's hostname, else its MAC. */
export function vertexLabel(node: TopoNode, nodeName: SceneOptions["nodeName"]): string {
  if (node.kind === "internet") return strings.topology.kind.internet;
  if (node.kind === "switch") return strings.topology.kind.switch;
  return nodeName(node) ?? node.name ?? node.mac ?? node.ips[0] ?? node.id;
}

/** Each client's one edge and the vertex at its other end. */
function clientLinks(graph: Graph): Map<string, { edge: TopoEdge; host: string }> {
  const links = new Map<string, { edge: TopoEdge; host: string }>();
  for (const edge of graph.edges.values()) {
    const a = graph.nodes.get(edge.a);
    const b = graph.nodes.get(edge.b);
    if (b?.kind === "client") links.set(b.id, { edge, host: edge.a });
    else if (a?.kind === "client") links.set(a.id, { edge, host: edge.b });
  }
  return links;
}

/** The vertices and links that `view` draws of the graph. */
export function buildScene(graph: Graph, view: ViewKind, options: SceneOptions): Scene {
  const vertices: SceneVertex[] = [];
  const links: SceneLink[] = [];
  const byId = new Map<string, SceneVertex>();
  const add = (vertex: SceneVertex) => {
    vertices.push(vertex);
    byId.set(vertex.id, vertex);
  };
  const clients = clientLinks(graph);
  const byHost = new Map<string, string[]>();
  for (const [client, { host }] of clients) {
    const list = byHost.get(host);
    if (list) list.push(client);
    else byHost.set(host, [client]);
  }
  const clusteredHosts = new Set(
    [...byHost]
      .filter(([host, list]) => list.length >= MIN_CLUSTER && options.clustered(host))
      .map(([host]) => host),
  );

  for (const node of graph.nodes.values()) {
    // The internet is no part of the air.
    if (node.kind === "client" || (view === "rf" && node.kind === "internet")) continue;
    add({
      id: node.id,
      kind: node.kind,
      label: vertexLabel(node, options.nodeName),
      radius: radii[node.kind],
      node,
      band: null,
      members: [],
      host: null,
    });
  }
  if (view !== "rf") {
    for (const [host, list] of byHost) {
      if (!byId.has(host)) continue;
      if (clusteredHosts.has(host)) {
        const id = clusterId(host);
        add({
          id,
          kind: "cluster",
          label: strings.topology.cluster(list.length),
          radius: radii.cluster + Math.min(9, 3 * Math.log2(list.length)),
          node: null,
          band: null,
          members: list,
          host,
        });
        links.push({ id: `${id}~link`, source: host, target: id, medium: "wifi", edge: null });
        continue;
      }
      for (const client of list) {
        const node = graph.nodes.get(client);
        const link = clients.get(client);
        if (!node || !link) continue;
        add({
          id: node.id,
          kind: "client",
          label: vertexLabel(node, options.nodeName),
          radius: radii.client,
          node,
          band: link.edge.medium === "wifi" ? link.edge.band : null,
          members: [],
          host,
        });
      }
    }
  }

  const uplinks = new Set<string>();
  if (view === "tree") {
    for (const node of graph.nodes.values()) if (node.uplink) uplinks.add(node.uplink);
  }
  for (const edge of graph.edges.values()) {
    if (!byId.has(edge.a) || !byId.has(edge.b)) continue;
    if (view === "tree" && !uplinks.has(edge.id)) continue;
    links.push({ id: edge.id, source: edge.a, target: edge.b, medium: edge.medium, edge });
  }
  return { view, vertices, links, byId };
}

/** The clients and devices a scene shows, counting those in clusters. */
export function sceneCounts(graph: Graph): { devices: number; clients: number } {
  let devices = 0;
  let clients = 0;
  for (const node of graph.nodes.values()) {
    if (node.kind === "client") clients += 1;
    else if (node.node !== null) devices += 1;
  }
  return { devices, clients };
}
