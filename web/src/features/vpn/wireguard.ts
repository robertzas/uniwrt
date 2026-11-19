import type { Ddns } from "../../api/gen/Ddns";
import type { SiteConfig } from "../../api/gen/SiteConfig";
import type { VpnEndpoint } from "../../api/gen/VpnEndpoint";
import type { VpnPeer } from "../../api/gen/VpnPeer";
import type { VpnPeerStatus } from "../../api/gen/VpnPeerStatus";
import type { WireGuard } from "../../api/gen/WireGuard";
import { formatBytes, formatDuration } from "../../lib/format";
import { type Cidr, cidr, formatIpv4, ipv4, isHost, isPrivate, subnetText } from "../../lib/ipv4";
import { isDnsName } from "../../lib/names";
import { strings } from "../../lib/strings";
import type { Tone } from "../../ui/Badge";

const t = strings.vpn.wireguard;
const e = t.errors;

/** A new server's port and tunnel (`DEFAULT_PORT`, `DEFAULT_ADDRESS` in the model). */
export const DEFAULT_PORT = 51820;
export const DEFAULT_ADDRESS = "10.0.250.1/24";
/** The most peers a server has, a peer's longest name and a site's most networks (the model's). */
export const MAX_PEERS = 250;
const NAME_MAX = 48;
const MAX_NETWORKS = 16;
/** CGNAT space and the factory default LAN, which the tunnel must not overlap. */
const CGNAT = cidr("100.64.0.0/10") as Cidr;
const FACTORY = cidr("192.168.1.0/24") as Cidr;

const overlap = (a: Cidr, b: Cidr) => a.network <= b.broadcast && b.network <= a.broadcast;

/** A WireGuard key as `wg` prints it: 32 bytes in padded base64 (`is_key` in the model). */
export function isKey(key: string): boolean {
  if (!/^[A-Za-z0-9+/]{43}=$/.test(key)) return false;
  try {
    return atob(key).length === 32;
  } catch {
    return false;
  }
}

/** A DNS name or an IP address a peer can dial (`is_host` in the model). */
export function isEndpointHost(host: string): boolean {
  if (ipv4(host) !== undefined) return true;
  if (host.includes(":")) return /^[0-9a-fA-F:.]+$/.test(host) && host.split(":").length > 2;
  return host.includes(".") && isDnsName(host);
}

/** A port typed in, or `undefined` unless it is 1 to 65535. */
function portOf(text: string): number | undefined {
  const value = Number(text.trim());
  return /^\d{1,5}$/.test(text.trim()) && value >= 1 && value <= 65_535 ? value : undefined;
}

// --- The server -------------------------------------------------------------------------------

/** The server as its editor holds it. */
export interface ServerDraft {
  enabled: boolean;
  port: string;
  address: string;
  endpoint: VpnEndpoint["kind"];
  ddns: string;
  host: string;
  dns: boolean;
}

export type ServerErrors = Partial<Record<"port" | "address" | "endpoint", string>>;

/** The editor's start: the server's settings, or a new one's (the first enabled DDNS name). */
export function serverDraft(server: WireGuard | null, ddns: readonly Ddns[]): ServerDraft {
  const first = ddns.find((d) => d.enabled) ?? ddns[0];
  const endpoint = server?.endpoint;
  return {
    enabled: server?.enabled ?? true,
    port: String(server?.port ?? DEFAULT_PORT),
    address: server?.address ?? DEFAULT_ADDRESS,
    endpoint: endpoint?.kind ?? (first ? "ddns" : "host"),
    ddns: endpoint?.kind === "ddns" ? endpoint.id : (first?.id ?? ""),
    host: endpoint?.kind === "host" ? endpoint.host : "",
    dns: server?.dns ?? true,
  };
}

/** Why `address` can't be the gateway's in the tunnel, if it can't (the model's `tunnel`). */
function tunnelError(config: SiteConfig, address: string): string | undefined {
  const net = cidr(address);
  if (!net) return e.address;
  if (net.len < 16 || net.len > 30) return e.prefix;
  if (!isPrivate(net.addr)) return e.private;
  if (!isHost(net, net.addr)) return e.gatewayHost(subnetText(net));
  const clash = config.networks.find((n) => {
    const other = n.ipv4 ? cidr(n.ipv4.address) : undefined;
    return other !== undefined && overlap(other, net);
  });
  if (clash) return e.overlapsNetwork(clash.name);
  if (overlap(net, CGNAT)) return e.cgnat;
  if (overlap(net, FACTORY)) return e.factory;
  return undefined;
}

/**
 * The server a draft describes, its peers and keys kept from `current`, or what is wrong with
 * it, checked as the server checks it. A peer outside a new tunnel's subnet is the server's
 * to refuse (on the peer).
 */
export function serverOf(
  draft: ServerDraft,
  current: WireGuard | null,
  config: SiteConfig,
): { server?: WireGuard; errors: ServerErrors } {
  const errors: ServerErrors = {};
  const port = portOf(draft.port);
  if (port === undefined) errors.port = e.port;
  else {
    const forward = config.firewall.forwards.find(
      (f) => f.enabled && f.proto !== "tcp" && f.ports.first <= port && port <= f.ports.last,
    );
    if (forward) errors.port = e.forwarded(forward.name, port);
  }
  const address = draft.address.trim();
  const tunnel = tunnelError(config, address);
  if (tunnel) errors.address = tunnel;
  const host = draft.host.trim();
  let endpoint: VpnEndpoint | undefined;
  if (draft.endpoint === "ddns") {
    if (config.services.ddns.some((d) => d.id === draft.ddns)) {
      endpoint = { kind: "ddns", id: draft.ddns };
    } else errors.endpoint = e.ddns;
  } else if (isEndpointHost(host)) endpoint = { kind: "host", host };
  else errors.endpoint = e.host;
  if (Object.keys(errors).length > 0 || port === undefined || !endpoint) return { errors };
  return {
    server: {
      enabled: draft.enabled,
      port,
      address,
      endpoint,
      dns: draft.dns,
      private_key: current?.private_key ?? null,
      public_key: current?.public_key ?? null,
      peers: current?.peers ?? [],
    },
    errors,
  };
}

/** The `vpn` zone's (`ZONE` in the model). */
const ZONE = "vpn";

/**
 * The config with the server saved, or removed (`null`): the zone's policies go with it, as the
 * model knows the zone only while the server is set up.
 */
export function withServer(config: SiteConfig, server: WireGuard | null): SiteConfig {
  const firewall = server
    ? config.firewall
    : {
        ...config.firewall,
        policies: config.firewall.policies.filter((p) => p.from !== ZONE && p.to !== ZONE),
      };
  return { ...config, firewall, vpn: { ...config.vpn, wireguard: server } };
}

const SERVER_FIELDS: Record<string, keyof ServerErrors> = {
  port: "port",
  address: "address",
  endpoint: "endpoint",
};

/** The server's issues on `vpn/wireguard/…` as the editor's errors; the rest as text. */
export function serverErrorsOfIssues(issues: readonly { path: string; message: string }[]): {
  errors: ServerErrors;
  other: string[];
} {
  const errors: ServerErrors = {};
  const other: string[] = [];
  for (const issue of issues) {
    const field = SERVER_FIELDS[issue.path.replace(/^vpn\/wireguard\//, "")];
    if (field && issue.path.startsWith("vpn/wireguard/")) errors[field] ??= issue.message;
    else other.push(issue.message);
  }
  return { errors, other };
}

/** What peers dial, for people: the DDNS name or the host, with the port. */
export function endpointText(server: WireGuard, ddns: readonly Ddns[]): string {
  const host =
    server.endpoint.kind === "host"
      ? server.endpoint.host
      : (ddns.find((d) => d.id === (server.endpoint as { id: string }).id)?.hostname ??
        t.missingDdns);
  return `${host.includes(":") && ipv4(host) === undefined ? `[${host}]` : host}:${server.port}`;
}

// --- Peers ------------------------------------------------------------------------------------

/** The lowest host of the tunnel's subnet that neither the gateway nor a peer holds. */
export function freeAddress(server: WireGuard, except?: string): string | undefined {
  const net = cidr(server.address);
  if (!net) return undefined;
  const taken = new Set([
    net.addr,
    ...server.peers.filter((p) => p.id !== except).map((p) => ipv4(p.address)),
  ]);
  for (let ip = net.network + 1; ip < net.broadcast; ip++) {
    if (!taken.has(ip)) return formatIpv4(ip);
  }
  return undefined;
}

/** A new peer's id (`[a-z][a-z0-9_]{0,11}`) from its name, not in `taken`. */
export function peerId(name: string, taken: ReadonlySet<string>): string {
  const slug = name
    .trim()
    .toLowerCase()
    .normalize("NFKD")
    .replace(/['’]/g, "")
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "");
  const full = /^[a-z]/.test(slug) ? slug : `p${slug}`;
  // Whole words where they fit: `living_room_tv` → `living_room`.
  const cut =
    full.length <= 12 ? full : full.slice(0, 13).replace(/_[^_]*$/, "") || full.slice(0, 12);
  const base = cut.slice(0, 12).replace(/_+$/, "") || "peer";
  if (!taken.has(base)) return base;
  for (let n = 2; ; n++) {
    const id = `${base.slice(0, 12 - String(n).length - 1)}_${n}`;
    if (!taken.has(id)) return id;
  }
}

/** A peer as its editor holds it. */
export interface PeerDraft {
  name: string;
  enabled: boolean;
  address: string;
  kind: "client" | "site";
  fullTunnel: boolean;
  /** A site's networks, one per line (or separated by commas or spaces). */
  networks: string;
  /** Where the gateway dials a site; empty waits to be dialed. */
  host: string;
  port: string;
  /** The peer brings its own keypair and gives its public key. */
  ownKey: boolean;
  publicKey: string;
}

export type PeerErrors = Partial<
  Record<"name" | "address" | "networks" | "host" | "port" | "publicKey", string>
>;

export function peerDraft(peer: VpnPeer | undefined, server: WireGuard): PeerDraft {
  const role = peer?.role;
  const endpoint = role?.kind === "site" ? role.endpoint : null;
  const ownKey = peer !== undefined && peer.private_key === null && peer.public_key !== "";
  return {
    name: peer?.name ?? "",
    enabled: peer?.enabled ?? true,
    address: peer?.address ?? freeAddress(server) ?? "",
    kind: role?.kind ?? "client",
    fullTunnel: role?.kind === "client" ? role.full_tunnel : true,
    networks: role?.kind === "site" ? role.networks.join("\n") : "",
    host: endpoint?.host ?? "",
    port: endpoint ? String(endpoint.port) : String(DEFAULT_PORT),
    ownKey,
    publicKey: ownKey ? (peer?.public_key ?? "") : "",
  };
}

/** The networks a site's field lists, or the first entry that isn't one. */
function networksOf(text: string): { nets: Cidr[]; bad?: string } {
  const nets: Cidr[] = [];
  for (const entry of text.split(/[\s,]+/).filter(Boolean)) {
    const net = cidr(entry);
    if (!net) return { nets, bad: e.network(entry) };
    if (net.addr !== net.network) return { nets, bad: e.networkWritten(entry, subnetText(net)) };
    nets.push(net);
  }
  return { nets };
}

/**
 * The peer a draft describes, with the secrets its save carries, or what is wrong with it,
 * checked as the server checks it. A new peer, or one switching back from its own keypair,
 * has no keys: the save makes them. One switching to its own keypair has its private key
 * cleared.
 */
export function peerOf(
  draft: PeerDraft,
  current: VpnPeer | undefined,
  server: WireGuard,
  config: SiteConfig,
): { peer?: VpnPeer; secrets?: Record<string, null>; errors: PeerErrors } {
  const errors: PeerErrors = {};
  const others = server.peers.filter((p) => p.id !== current?.id);
  const name = draft.name.trim();
  if (name === "" || [...name].length > NAME_MAX || /\p{Cc}/u.test(name)) errors.name = e.name;
  else if (others.some((p) => p.name.trim().toLowerCase() === name.toLowerCase())) {
    errors.name = e.nameTaken(name);
  }
  const tunnel = cidr(server.address);
  const address = draft.address.trim();
  const ip = ipv4(address);
  if (ip === undefined || !tunnel || !isHost(tunnel, ip)) {
    errors.address = e.peerAddress(tunnel ? subnetText(tunnel) : server.address);
  } else if (ip === tunnel.addr || others.some((p) => p.address === address)) {
    errors.address = e.addressTaken(address);
  }
  const publicKey = draft.publicKey.trim();
  if (draft.ownKey) {
    if (!isKey(publicKey)) errors.publicKey = e.key;
    else if (publicKey === server.public_key || others.some((p) => p.public_key === publicKey)) {
      errors.publicKey = e.keyTaken;
    }
  }
  let role: VpnPeer["role"] = { kind: "client", full_tunnel: draft.fullTunnel };
  if (draft.kind === "site") {
    const { nets, bad } = networksOf(draft.networks);
    if (bad) errors.networks = bad;
    else if (nets.length === 0 || nets.length > MAX_NETWORKS) errors.networks = e.networks;
    else {
      const clash = siteClash(nets, config, server, current?.id);
      if (clash) errors.networks = clash;
    }
    const host = draft.host.trim();
    const port = portOf(draft.port);
    if (host !== "" && !isEndpointHost(host)) errors.host = e.host;
    if (host !== "" && port === undefined) errors.port = e.port;
    role = {
      kind: "site",
      networks: nets.map(subnetText),
      endpoint: host === "" || port === undefined ? null : { host, port },
    };
  }
  if (Object.keys(errors).length > 0) return { errors };

  const id = current?.id ?? peerId(name, new Set(others.map((p) => p.id)));
  const wasOwn = current !== undefined && current.private_key === null;
  const keys: Pick<VpnPeer, "public_key" | "private_key"> = draft.ownKey
    ? { public_key: publicKey, private_key: null }
    : wasOwn || current === undefined
      ? { public_key: "", private_key: null }
      : { public_key: current.public_key, private_key: current.private_key };
  const peer: VpnPeer = {
    id,
    name,
    enabled: draft.enabled,
    address,
    ...keys,
    preshared_key: current?.preshared_key ?? null,
    role,
  };
  const clears = draft.ownKey && current?.private_key != null;
  return {
    peer,
    ...(clears && { secrets: { [`vpn/wireguard/peers/${id}/private_key`]: null } }),
    errors,
  };
}

/** What a site's networks collide with: the site's networks, the tunnel or another site's. */
function siteClash(
  nets: readonly Cidr[],
  config: SiteConfig,
  server: WireGuard,
  self: string | undefined,
): string | undefined {
  const tunnel = cidr(server.address);
  const routed = server.peers.flatMap((p) =>
    p.id !== self && p.role.kind === "site"
      ? p.role.networks.flatMap((n) => {
          const net = cidr(n);
          return net ? [{ name: p.name, net }] : [];
        })
      : [],
  );
  for (const net of nets) {
    const text = subnetText(net);
    const local = config.networks.find((n) => {
      const other = n.ipv4 ? cidr(n.ipv4.address) : undefined;
      return other !== undefined && overlap(other, net);
    });
    if (local) return e.overlapsLocal(text, local.name);
    if (tunnel && overlap(tunnel, net)) return e.overlapsTunnel(text);
    const other = routed.find((r) => overlap(r.net, net));
    if (other) return e.overlapsSite(text, other.name);
  }
  const own = nets.findIndex((a, i) => nets.some((b, j) => j < i && overlap(a, b)));
  return own >= 0 ? e.overlapsSite(subnetText(nets[own] as Cidr), e.itself) : undefined;
}

/** The config with `edit` saved (in place, or appended) or a peer removed. */
export function withPeer(config: SiteConfig, edit: VpnPeer | { remove: string }): SiteConfig {
  const server = config.vpn.wireguard;
  if (!server) return config;
  const list = server.peers;
  const peers =
    "remove" in edit
      ? list.filter((p) => p.id !== edit.remove)
      : list.some((p) => p.id === edit.id)
        ? list.map((p) => (p.id === edit.id ? edit : p))
        : [...list, edit];
  return withServer(config, { ...server, peers });
}

const PEER_FIELDS: [RegExp, keyof PeerErrors][] = [
  [/^(name|id)$/, "name"],
  [/^address$/, "address"],
  [/^public_key$/, "publicKey"],
  [/^role\/networks(\/\d+)?$/, "networks"],
  [/^role\/endpoint\/host$/, "host"],
  [/^role\/endpoint\/port$/, "port"],
];

/** The server's issues on `vpn/wireguard/peers/<id>/…` as the editor's errors; the rest as text. */
export function peerErrorsOfIssues(
  issues: readonly { path: string; message: string }[],
  id: string | undefined,
): { errors: PeerErrors; other: string[] } {
  const errors: PeerErrors = {};
  const other: string[] = [];
  const prefix = `vpn/wireguard/peers/${id}/`;
  for (const issue of issues) {
    const rest =
      id !== undefined && issue.path.startsWith(prefix)
        ? issue.path.slice(prefix.length)
        : undefined;
    const field = rest === undefined ? undefined : PEER_FIELDS.find(([re]) => re.test(rest))?.[1];
    if (field) errors[field] ??= issue.message;
    else other.push(issue.message);
  }
  return { errors, other };
}

/** What a peer is, for its row: `Client · full tunnel`, `Site · 192.168.50.0/24`. */
export function roleText(peer: VpnPeer): string {
  return peer.role.kind === "client"
    ? t.client(peer.role.full_tunnel)
    : t.site(peer.role.networks.join(", "));
}

/** A peer's state on the gateway, for its row. */
export function peerState(
  peer: VpnPeer,
  status: VpnPeerStatus | undefined,
  tunnel: { running: boolean; online: boolean } | undefined,
  now: number,
): { tone: Tone; text: string; traffic?: string; endpoint?: string } {
  if (!peer.enabled) return { tone: "neutral", text: t.state.off };
  if (!status || !tunnel) return { tone: "neutral", text: t.state.unknown };
  if (!status.on_gateway) return { tone: "info", text: t.state.pending };
  if (!tunnel.running) return { tone: "warn", text: t.state.stopped };
  const ago =
    status.handshake_at === null
      ? undefined
      : formatDuration(Math.max(0, (now - status.handshake_at) / 1000));
  const traffic =
    status.handshake_at === null
      ? undefined
      : t.traffic(formatBytes(status.rx_bytes), formatBytes(status.tx_bytes));
  const extra = {
    ...(traffic && { traffic }),
    ...(status.endpoint && { endpoint: status.endpoint }),
  };
  if (status.connected) return { tone: "ok", text: t.state.connected(ago ?? ""), ...extra };
  if (ago === undefined) return { tone: "neutral", text: t.state.never };
  return { tone: "neutral", text: t.state.idle(ago), ...extra };
}
