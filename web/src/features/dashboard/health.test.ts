import { describe, expect, it } from "vitest";
import type { InterfaceFacts } from "../../api/gen/InterfaceFacts";
import { client } from "../../test/clients";
import { summary } from "../../test/fixtures";
import { edge, house } from "../../test/topology";
import { graphOf } from "../../viz/topology/graph";
import {
  clientsHealth,
  internetHealth,
  meshHealth,
  nodesHealth,
  scoreSpread,
  topClients,
  wanInterfaces,
  wifiHealth,
} from "./health";

const gateway = summary(1, "gw", { role: "gateway" });
const ap = summary(2, "ap");

function iface(name: string, extra: Partial<InterfaceFacts> = {}): InterfaceFacts {
  return {
    name,
    proto: "dhcp",
    up: true,
    device: "eth1",
    uptime_s: 60,
    ipv4: [],
    ipv6: [],
    delegated_prefixes: [],
    ipv4_gateway: null,
    ipv6_gateway: null,
    dns_servers: [],
    errors: [],
    ...extra,
  };
}

function traffic(down: number, up: number) {
  return { at: 1, down_bytes: 0, up_bytes: 0, down_bps: down, up_bps: up };
}

describe("the internet's health", () => {
  it("is online while a WAN link has a route, and sums what the routed ones carry", () => {
    const topology = house();
    topology.edges = [
      edge("internet", "node:1", { medium: "wan", b_port: "eth1", traffic: traffic(9e6, 1e6) }),
      edge("internet", "node:1", { medium: "wan", b_port: "wwan0", traffic: traffic(5e6, 5e6) }),
    ];
    topology.edges[1] = { ...topology.edges[1]!, id: "wwan", up: false };
    expect(internetHealth([gateway, ap], graphOf(topology))).toEqual({
      state: "online",
      tone: "ok",
      down: 9e6,
      up: 1e6,
    });
  });

  it("is offline when no WAN link has a route, unknown without a gateway or while it is offline", () => {
    const topology = house();
    topology.edges = [edge("internet", "node:1", { medium: "wan", up: false })];
    const graph = graphOf(topology);
    expect(internetHealth([gateway], graph)).toMatchObject({ state: "offline", tone: "crit" });
    expect(internetHealth([ap], graph).state).toBe("unknown");
    const down = { ...gateway, status: { ...gateway.status, state: "offline" as const } };
    expect(internetHealth([down], graph).state).toBe("unknown");
    expect(internetHealth([gateway], undefined).state).toBe("unknown");
  });
});

describe("the mesh's health", () => {
  it("counts links between adopted nodes only and reads the weakest", () => {
    // house(): node:1–node:2 at quality 75, and a down link to a peer that is not adopted.
    expect(meshHealth(graphOf(house()))).toEqual({ links: 1, up: 1, weakest: 75, tone: "ok" });
  });

  it("is critical while a link is down, and neutral without a mesh", () => {
    const topology = house();
    topology.edges.push(edge("node:2", "node:3", { medium: "mesh", up: false }));
    expect(meshHealth(graphOf(topology))).toMatchObject({ links: 2, up: 1, tone: "crit" });
    expect(meshHealth(undefined)).toEqual({ links: 0, up: 0, weakest: null, tone: "neutral" });
  });
});

describe("the clients' health", () => {
  const scored = (mac: string, score: number, extra = {}) => ({
    ...client(mac, extra),
    experience: { score, parts: [] },
  });
  const wired = (mac: string, rate: number) => {
    const base = client(mac);
    return {
      ...base,
      connection: base.connection && {
        ...base.connection,
        medium: "wired" as const,
        traffic: traffic(rate, 0),
      },
    };
  };

  it("averages Wi-Fi experience and counts weak signals", () => {
    const weak = scored("aa:00:00:00:00:02", 40);
    const clients = [
      scored("aa:00:00:00:00:01", 90),
      { ...weak, connection: weak.connection && { ...weak.connection, signal_dbm: -80 } },
      wired("aa:00:00:00:00:03", 0),
      scored("aa:00:00:00:00:04", 10, { online: false }),
    ];
    expect(wifiHealth(clients)).toEqual({ clients: 2, weak: 1, score: 65, tone: "warn" });
    expect(clientsHealth(clients)).toEqual({ online: 3, wifi: 2, wired: 1 });
    expect(scoreSpread(clients)).toEqual({ ok: 1, warn: 0, crit: 1 });
  });

  it("ranks the clients moving traffic, busiest first", () => {
    const clients = [
      wired("aa:00:00:00:00:01", 10),
      wired("aa:00:00:00:00:02", 0),
      wired("aa:00:00:00:00:03", 30),
      { ...wired("aa:00:00:00:00:04", 99), online: false },
    ];
    expect(topClients(clients, 5).map((c) => c.mac)).toEqual([
      "aa:00:00:00:00:03",
      "aa:00:00:00:00:01",
    ]);
    expect(topClients(clients, 1)).toHaveLength(1);
  });
});

describe("the nodes' health", () => {
  it("is critical with a node offline, fair with one degraded", () => {
    const state = (s: "online" | "degraded" | "offline") => ({ state: s, since: 0, reasons: [] });
    expect(nodesHealth([gateway, ap]).tone).toBe("ok");
    expect(nodesHealth([gateway, { ...ap, status: state("degraded") }])).toEqual({
      total: 2,
      online: 1,
      degraded: 1,
      offline: 0,
      tone: "warn",
    });
    expect(nodesHealth([{ ...gateway, status: state("offline") }, ap]).tone).toBe("crit");
    expect(nodesHealth([]).tone).toBe("neutral");
  });
});

describe("the WAN interfaces", () => {
  it("are those with a default route first, then configured wan* ones", () => {
    const list = wanInterfaces([
      iface("lan", { ipv4: ["192.168.1.1/24"] }),
      iface("wan6", { proto: "dhcpv6", ipv6_gateway: "fe80::1" }),
      iface("wan", { ipv4_gateway: "100.64.0.1" }),
      iface("wanb", { proto: "none" }),
      iface("wanc", { proto: "static" }),
    ]);
    expect(list.map((i) => i.name)).toEqual(["wan", "wan6", "wanc"]);
  });
});
