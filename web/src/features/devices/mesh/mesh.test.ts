import { describe, expect, it } from "vitest";
import type { DeviceFacts } from "../../../api/gen/DeviceFacts";
import type { InterfaceFacts } from "../../../api/gen/InterfaceFacts";
import type { NodeFacts } from "../../../api/gen/NodeFacts";
import { facts } from "../../../test/fixtures";
import { chain, edge, vertex } from "../../../test/topology";
import { graphOf } from "../../../viz/topology/graph";
import { impactOf } from "../../../viz/topology/health";
import { failuresOf, lostNodes, meshLinksOf, strandedPorts } from "./mesh";

const iface = (name: string) => ({ name }) as InterfaceFacts;
const device = (name: string, master: string | null) => ({ name, master }) as DeviceFacts;

function withPorts(interfaces: InterfaceFacts[], devices: DeviceFacts[]): NodeFacts {
  return { ...facts(1), interfaces, devices };
}

describe("mesh links", () => {
  it("lists a node's links to devices, down first, then weakest first", () => {
    const links = meshLinksOf(graphOf(chain()), "node:2");
    expect(links.map((link) => [link.peer, link.edge.up, link.edge.score])).toEqual([
      ["peer:02:00:00:00:00:99", false, null],
      ["node:1", true, 55],
      ["node:3", true, 100],
    ]);
  });

  it("sees each link from the node, its port first, and leaves out the WAN", () => {
    const topology = chain();
    topology.nodes.push(vertex("switch:3:lan1", { kind: "switch" }));
    topology.edges.push(
      edge("switch:3:lan1", "node:3", { b_port: "lan1.4094" }),
      edge("internet", "node:3", { medium: "wan", b_port: "wan" }),
    );
    const links = meshLinksOf(graphOf(topology), "node:3");
    expect(links.map((link) => [link.peer, link.port, link.peerPort])).toEqual([
      ["node:2", null, null],
      ["switch:3:lan1", "lan1.4094", null],
    ]);
  });
});

describe("stranded ports", () => {
  it("flags a port held for the backhaul alone with no link up over it", () => {
    const facts = withPorts(
      [iface("lan"), iface("uw_bh_lan1"), iface("uw_bh_lan2"), iface("uw_bh_lan3")],
      [device("lan1", null), device("lan2", "br-lan"), device("lan3", null)],
    );
    const topology = chain();
    topology.edges.push(edge("node:2", "node:3", { id: "wired", a_port: "lan3.4094" }));
    const links = meshLinksOf(graphOf(topology), "node:2");
    // lan2 is still an overlay (in the bridge); lan3 has its cable up.
    expect(strandedPorts(facts, links)).toEqual(["lan1"]);
    expect(strandedPorts(null, links)).toEqual([]);
  });
});

describe("failed cables", () => {
  it("keeps the node's, its end first, newest first", () => {
    const failure = (a: number, b: number, at: number) => ({
      ends: [
        { node: a, port: `eth${a}` },
        { node: b, port: `eth${b}` },
      ] as [{ node: number; port: string }, { node: number; port: string }],
      job: 1,
      at,
      why: "no answer at 1532 bytes",
    });
    const rows = failuresOf([failure(2, 3, 10), failure(1, 2, 20), failure(1, 3, 30)], 2);
    expect(rows.map((row) => [row.mine.port, row.peer.port])).toEqual([
      ["eth2", "eth1"],
      ["eth2", "eth3"],
    ]);
  });
});

describe("lost nodes", () => {
  it("names the adopted nodes a loss cuts off, not clients", () => {
    const graph = graphOf(chain());
    expect(lostNodes(graph, impactOf(graph, "node:2").lost)).toEqual([3]);
  });
});
