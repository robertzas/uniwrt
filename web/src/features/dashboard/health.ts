/**
 * What the dashboard's health header says (features/observability.md, Dashboard): the internet,
 * the mesh, Wi-Fi, the nodes and the clients, each as a state with a tone. Pure: it reads the
 * fleet, the live topology and the clients it is handed.
 */
import type { Client } from "../../api/gen/Client";
import type { InterfaceFacts } from "../../api/gen/InterfaceFacts";
import type { NodeSummary } from "../../api/gen/NodeSummary";
import type { TopoEdge } from "../../api/gen/TopoEdge";
import { averageScore } from "../../lib/clientList";
import { scoreTone } from "../../lib/clients";
import type { Graph } from "../../viz/topology/graph";

export type Tone = "ok" | "warn" | "crit" | "neutral";

export interface InternetHealth {
  /** `unknown` without a gateway, its WAN, or while it is offline. */
  state: "online" | "offline" | "unknown";
  tone: Tone;
  /** What the WAN links carry now, in bit/s; `null` when no counter says. */
  down: number | null;
  up: number | null;
}

/** The gateway: the fleet's first node in that role. */
export function gatewayOf(nodes: readonly NodeSummary[]): NodeSummary | undefined {
  return nodes.find((node) => node.role === "gateway");
}

/** The gateway's links to the internet in the topology. */
export function wanLinks(graph: Graph | undefined): TopoEdge[] {
  return graph ? [...graph.edges.values()].filter((edge) => edge.medium === "wan") : [];
}

function sum(values: readonly (number | null | undefined)[]): number | null {
  const known = values.filter((value): value is number => value != null);
  return known.length > 0 ? known.reduce((total, value) => total + value, 0) : null;
}

/** Online while a WAN link has a default route; what they carry, download first. */
export function internetHealth(
  nodes: readonly NodeSummary[],
  graph: Graph | undefined,
): InternetHealth {
  const gateway = gatewayOf(nodes);
  const links = wanLinks(graph);
  const unknown: InternetHealth = { state: "unknown", tone: "neutral", down: null, up: null };
  if (!gateway || gateway.status.state === "offline" || links.length === 0) return unknown;
  const up = links.filter((link) => link.up);
  if (up.length === 0) return { state: "offline", tone: "crit", down: null, up: null };
  return {
    state: "online",
    tone: "ok",
    down: sum(up.map((link) => link.traffic?.down_bps)),
    up: sum(up.map((link) => link.traffic?.up_bps)),
  };
}

export interface MeshHealth {
  /** Mesh links between adopted nodes, and how many of them are up. */
  links: number;
  up: number;
  /** The quality (0–100) of the weakest link that is up. */
  weakest: number | null;
  tone: Tone;
}

/** A mesh link quality reads good from 60, fair from 40, poor below. */
export function qualityTone(quality: number | null): Tone {
  if (quality == null) return "neutral";
  if (quality >= 60) return "ok";
  return quality >= 40 ? "warn" : "crit";
}

/** The mesh between adopted nodes (not peers that aren't adopted): down links are critical. */
export function meshHealth(graph: Graph | undefined): MeshHealth {
  const adopted = (id: string) => id.startsWith("node:");
  const links = graph
    ? [...graph.edges.values()].filter(
        (edge) => edge.medium === "mesh" && adopted(edge.a) && adopted(edge.b),
      )
    : [];
  const up = links.filter((link) => link.up);
  const qualities = up.map((link) => link.quality).filter((q): q is number => q != null);
  const weakest = qualities.length > 0 ? Math.min(...qualities) : null;
  const tone: Tone =
    links.length === 0 ? "neutral" : up.length < links.length ? "crit" : qualityTone(weakest);
  return { links: links.length, up: up.length, weakest, tone };
}

/** A signal this weak or worse counts as weak (the clients list's "weak signal"). */
export const WEAK_DBM = -75;

export interface WifiHealth {
  /** Online Wi-Fi clients, those with a weak signal, and their mean experience score. */
  clients: number;
  weak: number;
  score: number | null;
  tone: Tone;
}

export function wifiHealth(clients: readonly Client[]): WifiHealth {
  const wifi = clients.filter((c) => c.online && c.connection?.medium === "wifi");
  const weak = wifi.filter((c) => (c.connection?.signal_dbm ?? 0) < WEAK_DBM).length;
  const { score } = averageScore(wifi);
  return { clients: wifi.length, weak, score, tone: scoreTone(score) };
}

export interface NodesHealth {
  total: number;
  online: number;
  degraded: number;
  offline: number;
  tone: Tone;
}

/** Every node online is good; any degraded is fair; any offline is critical. */
export function nodesHealth(nodes: readonly NodeSummary[]): NodesHealth {
  const count = (state: NodeSummary["status"]["state"]) =>
    nodes.filter((node) => node.status.state === state).length;
  const health = {
    total: nodes.length,
    online: count("online"),
    degraded: count("degraded"),
    offline: count("offline"),
  };
  const tone: Tone =
    health.total === 0
      ? "neutral"
      : health.offline > 0
        ? "crit"
        : health.degraded > 0
          ? "warn"
          : "ok";
  return { ...health, tone };
}

export interface ClientsHealth {
  online: number;
  wifi: number;
  wired: number;
}

export function clientsHealth(clients: readonly Client[]): ClientsHealth {
  const online = clients.filter((client) => client.online);
  const wifi = online.filter((client) => client.connection?.medium === "wifi").length;
  return { online: online.length, wifi, wired: online.length - wifi };
}

/**
 * The gateway's WAN interfaces, as the topology finds them: those with a default route, or named
 * like OpenWrt's WAN (`wan`, `wan6`) and configured. Those with a route first.
 */
export function wanInterfaces(interfaces: readonly InterfaceFacts[]): InterfaceFacts[] {
  const routed = (iface: InterfaceFacts) =>
    iface.ipv4_gateway !== null || iface.ipv6_gateway !== null;
  return interfaces
    .filter((iface) => routed(iface) || (iface.name.startsWith("wan") && iface.proto !== "none"))
    .sort((a, b) => Number(routed(b)) - Number(routed(a)) || a.name.localeCompare(b.name));
}

/** The clients moving the most traffic now, busiest first; only those moving any. */
export function topClients(clients: readonly Client[], count: number): Client[] {
  const rate = (client: Client) =>
    (client.connection?.traffic?.down_bps ?? 0) + (client.connection?.traffic?.up_bps ?? 0);
  return clients
    .filter((client) => client.online && rate(client) > 0)
    .sort((a, b) => rate(b) - rate(a) || a.mac.localeCompare(b.mac))
    .slice(0, count);
}

/** How the online Wi-Fi clients' scores spread: good (80+), fair (50+), poor. */
export function scoreSpread(clients: readonly Client[]): {
  ok: number;
  warn: number;
  crit: number;
} {
  const spread = { ok: 0, warn: 0, crit: 0 };
  for (const client of clients) {
    if (!client.online || client.connection?.medium !== "wifi" || !client.experience) continue;
    const tone = scoreTone(client.experience.score);
    if (tone !== "neutral") spread[tone] += 1;
  }
  return spread;
}
