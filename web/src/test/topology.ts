import type { TopoEdge } from "../api/gen/TopoEdge";
import type { Topology } from "../api/gen/Topology";
import type { TopoNode } from "../api/gen/TopoNode";

export function vertex(id: string, extra: Partial<TopoNode> = {}): TopoNode {
  return {
    id,
    kind: "client",
    node: null,
    mac: null,
    name: null,
    ips: [],
    state: null,
    cpu_percent: null,
    memory_percent: null,
    parent: null,
    uplink: null,
    hops: null,
    gateway_hops: null,
    wireless_hops: null,
    depends_on: null,
    ...extra,
  };
}

export function edge(a: string, b: string, extra: Partial<TopoEdge> = {}): TopoEdge {
  return {
    id: `${a}~${b}`,
    a,
    b,
    medium: "ethernet",
    a_port: null,
    b_port: null,
    up: true,
    band: null,
    ssid: null,
    signal_dbm: null,
    tq: null,
    capacity_mbps: null,
    quality: null,
    traffic: null,
    score: null,
    weakest: null,
    drops: [],
    critical: false,
    ...extra,
  };
}

/**
 * The internet, gateway 1 (`gw`) and access point 2 (`ap`) meshed at −60 dBm, two Wi-Fi
 * clients on the access point, one wired client on the gateway, and a mesh link down between
 * the access point and a peer that is not adopted.
 */
export function house(): Topology {
  const clientOf = (mac: string, host: string, hops: number) =>
    vertex(`client:${mac}`, { mac, parent: host, uplink: `${host}~client:${mac}`, hops });
  return {
    at: 1_000,
    nodes: [
      vertex("internet", { kind: "internet", hops: 0 }),
      vertex("node:1", {
        kind: "gateway",
        node: 1,
        name: "gw",
        ips: ["192.168.1.1"],
        state: "online",
        cpu_percent: 30,
        memory_percent: 40,
        parent: "internet",
        uplink: "internet~node:1",
        hops: 1,
      }),
      vertex("node:2", {
        kind: "ap",
        node: 2,
        name: "ap",
        state: "degraded",
        cpu_percent: 90,
        parent: "node:1",
        uplink: "node:1~node:2",
        hops: 2,
      }),
      vertex("peer:02:00:00:00:00:99", { kind: "peer", mac: "02:00:00:00:00:99" }),
      { ...clientOf("aa:00:00:00:00:01", "node:2", 3), name: "phone" },
      clientOf("aa:00:00:00:00:02", "node:2", 3),
      clientOf("aa:00:00:00:00:03", "node:1", 2),
    ],
    edges: [
      edge("internet", "node:1", { medium: "wan", b_port: "eth1" }),
      edge("node:1", "node:2", {
        medium: "mesh",
        band: "5g",
        signal_dbm: -60,
        quality: 75,
        capacity_mbps: 400,
        traffic: {
          at: 1_000,
          down_bytes: 5_000,
          up_bytes: 1_000,
          down_bps: 2_000_000,
          up_bps: 30_000,
        },
      }),
      edge("node:2", "peer:02:00:00:00:00:99", { medium: "mesh", up: false }),
      edge("node:2", "client:aa:00:00:00:00:01", {
        medium: "wifi",
        band: "5g",
        ssid: "Home",
        signal_dbm: -55,
      }),
      edge("node:2", "client:aa:00:00:00:00:02", { medium: "wifi", band: "2g", ssid: "Home" }),
      edge("node:1", "client:aa:00:00:00:00:03", { medium: "ethernet", a_port: "lan2" }),
    ],
  };
}

/**
 * The house with health: a third access point (`ap3`) meshed to the access point only, so the
 * access point is a single point of failure and its link to the gateway critical, scored 55 by
 * its signal after two drops; a client on `ap3`.
 */
export function chain(): Topology {
  const base = house();
  const nodes = base.nodes.map((node) =>
    node.id === "node:2" ? { ...node, gateway_hops: 1, wireless_hops: 1 } : node,
  );
  const edges = base.edges.map((link) =>
    link.id === "node:1~node:2"
      ? { ...link, score: 55, weakest: "signal" as const, drops: [400, 900], critical: true }
      : link,
  );
  return {
    at: base.at,
    nodes: [
      ...nodes,
      vertex("node:3", {
        kind: "ap",
        node: 3,
        name: "ap3",
        state: "online",
        parent: "node:2",
        uplink: "node:2~node:3",
        hops: 3,
        gateway_hops: 2,
        wireless_hops: 2,
        depends_on: "node:2",
      }),
      vertex("client:aa:00:00:00:00:04", { parent: "node:3", hops: 4 }),
    ],
    edges: [
      ...edges,
      edge("node:2", "node:3", { medium: "mesh", band: "5g", score: 100, critical: true }),
      edge("node:3", "client:aa:00:00:00:00:04", { medium: "wifi", band: "5g" }),
    ],
  };
}
