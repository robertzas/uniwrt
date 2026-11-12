import type { BackhaulFailure } from "../../../api/gen/BackhaulFailure";
import type { CableEnd } from "../../../api/gen/CableEnd";
import type { NodeFacts } from "../../../api/gen/NodeFacts";
import type { TopoEdge } from "../../../api/gen/TopoEdge";
import type { Graph } from "../../../viz/topology/graph";

/**
 * A device's Mesh tab, read off the topology (features/mesh.md, Telemetry & health) and the
 * failed cables (Wired backhaul): its links to other nodes, switches and peers, the ports it
 * holds for the backhaul alone, and its cables' failed probes.
 */

/** One of the node's backhaul links, seen from the node. */
export interface MeshLink {
  edge: TopoEdge;
  /** The vertex at the other end. */
  peer: string;
  /** The node's port or interface on it, and the peer's. */
  port: string | null;
  peerPort: string | null;
}

/** The vertex id of an adopted node. */
export function vertexOf(node: number): string {
  return `node:${node}`;
}

/**
 * The links between `vertex` and other nodes, switches or peers (not its clients or the WAN):
 * down first, then by score, weakest first.
 */
export function meshLinksOf(graph: Graph, vertex: string): MeshLink[] {
  const links: MeshLink[] = [];
  for (const edge of graph.edges.values()) {
    if (edge.medium === "wan" || (edge.a !== vertex && edge.b !== vertex)) continue;
    const mine = edge.a === vertex;
    const peer = mine ? edge.b : edge.a;
    const kind = graph.nodes.get(peer)?.kind;
    if (kind === undefined || kind === "client" || kind === "internet") continue;
    links.push({
      edge,
      peer,
      port: mine ? edge.a_port : edge.b_port,
      peerPort: mine ? edge.b_port : edge.a_port,
    });
  }
  const rank = (link: MeshLink) => (link.edge.up ? (link.edge.score ?? 100) : -1);
  return links.sort((x, y) => rank(x) - rank(y) || x.peer.localeCompare(y.peer));
}

/** Whether a link runs over the port: the port itself, or a VLAN on it (`lan1.4094`). */
function onPort(link: MeshLink, port: string): boolean {
  return link.port === port || (link.port?.startsWith(`${port}.`) ?? false);
}

/**
 * The ports the node holds for the backhaul alone (a `uw_bh_<port>` interface, the port out of
 * any bridge) with no link up over them: whatever is plugged in there gets no LAN, and the
 * apply keeps them so while the far end may come back (D-097).
 */
export function strandedPorts(facts: NodeFacts | null, links: readonly MeshLink[]): string[] {
  if (!facts) return [];
  const stranded: string[] = [];
  for (const iface of facts.interfaces) {
    const port = iface.name.startsWith("uw_bh_") ? iface.name.slice("uw_bh_".length) : "";
    if (!port) continue;
    const device = facts.devices.find((dev) => dev.name === port);
    if (!device || device.master !== null) continue;
    if (!links.some((link) => link.edge.up && onPort(link, port))) stranded.push(port);
  }
  return stranded.sort();
}

/** A failed cable seen from the node: its end first. */
export interface NodeFailure {
  failure: BackhaulFailure;
  mine: CableEnd;
  peer: CableEnd;
}

/** The failed cables with an end on `node`, newest first. */
export function failuresOf(failures: readonly BackhaulFailure[], node: number): NodeFailure[] {
  return failures
    .flatMap((failure): NodeFailure[] => {
      const [a, b] = failure.ends;
      if (a.node === node) return [{ failure, mine: a, peer: b }];
      if (b.node === node) return [{ failure, mine: b, peer: a }];
      return [];
    })
    .sort((x, y) => y.failure.at - x.failure.at);
}

/** The adopted nodes that go down with the vertex, by node id. */
export function lostNodes(graph: Graph, lost: ReadonlySet<string>): number[] {
  const ids: number[] = [];
  for (const vertex of lost) {
    const node = graph.nodes.get(vertex);
    if (node?.kind !== "client" && node?.node != null) ids.push(node.node);
  }
  return ids.sort((a, b) => a - b);
}
