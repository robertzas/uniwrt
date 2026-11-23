/** What the device's tables show (D-142): filtering, address order, clients and NAT. */
import type { Client } from "../../../api/gen/Client";
import type { IpFamily } from "../../../api/gen/IpFamily";
import type { NeighborEntry } from "../../../api/gen/NeighborEntry";
import type { TrackedConnection } from "../../../api/gen/TrackedConnection";
import type { TrackedFlow } from "../../../api/gen/TrackedFlow";
import type { Tone } from "../../../ui/Badge";

/** The lowercased words of a filter. */
export function filterWords(query: string): string[] {
  return query.toLowerCase().split(/\s+/).filter(Boolean);
}

/** Every word is in one of the fields (case-insensitively). */
export function matchesWords(
  fields: readonly (string | number | null | undefined)[],
  words: readonly string[],
): boolean {
  if (words.length === 0) return true;
  const text = fields
    .filter((field) => field != null)
    .join(" ")
    .toLowerCase();
  return words.every((word) => text.includes(word));
}

/** The family filter: both, or one. */
export type FamilyFilter = "all" | IpFamily;

function ipv4Parts(text: string): number[] | undefined {
  const parts = text.split(".");
  if (parts.length !== 4 || !parts.every((part) => /^\d{1,3}$/.test(part))) return undefined;
  const bytes = parts.map(Number);
  return bytes.every((byte) => byte <= 255) ? bytes : undefined;
}

function ipv6Groups(text: string): number[] | undefined {
  if (!text.includes(":")) return undefined;
  let address = text.toLowerCase().split("%")[0] ?? "";
  // An embedded IPv4 address (::ffff:192.0.2.1) is the last two groups.
  const tail = address.slice(address.lastIndexOf(":") + 1);
  const v4 = tail.includes(".") ? ipv4Parts(tail) : undefined;
  if (tail.includes(".")) {
    if (!v4) return undefined;
    const [a = 0, b = 0, c = 0, d = 0] = v4;
    address = `${address.slice(0, address.lastIndexOf(":") + 1)}${((a << 8) | b).toString(16)}:${((c << 8) | d).toString(16)}`;
  }
  const halves = address.split("::");
  if (halves.length > 2) return undefined;
  const words = (half: string | undefined) => (half ? half.split(":") : []);
  const head = words(halves[0]);
  const rest = words(halves[1]);
  const missing = 8 - head.length - rest.length;
  if (halves.length === 1 ? missing !== 0 : missing < 1) return undefined;
  const groups = [...head, ...Array<string>(halves.length === 1 ? 0 : missing).fill("0"), ...rest];
  if (!groups.every((group) => /^[0-9a-f]{1,4}$/.test(group))) return undefined;
  return groups.map((group) => Number.parseInt(group, 16));
}

/**
 * A sort key for an address or a prefix (`default`, `192.0.2.0/24`, `2001:db8::1`): `default`
 * first, then IPv4 and IPv6 by value, then by prefix length; anything else after them.
 */
function addressKey(text: string): number[] | undefined {
  if (text === "default") return [0];
  const [address = "", length] = text.split("/");
  const v4 = ipv4Parts(address);
  if (v4) return [4, ...v4, length === undefined ? 32 : Number(length)];
  const v6 = ipv6Groups(address);
  if (v6) return [6, ...v6, length === undefined ? 128 : Number(length)];
  return undefined;
}

/** Orders addresses and prefixes by value, as people read them (not as text). */
export function compareAddresses(a: string, b: string): number {
  const ka = addressKey(a);
  const kb = addressKey(b);
  if (ka && kb) {
    for (let i = 0; i < Math.min(ka.length, kb.length); i++) {
      const diff = (ka[i] ?? 0) - (kb[i] ?? 0);
      if (diff !== 0) return diff;
    }
    return ka.length - kb.length;
  }
  if (ka) return -1;
  if (kb) return 1;
  return a.localeCompare(b);
}

/** Orders numbers that may be missing: the missing ones last. */
export function compareNumbers(a: number | null | undefined, b: number | null | undefined): number {
  if (a == null) return b == null ? 0 : 1;
  if (b == null) return -1;
  return a - b;
}

/** Orders text that may be missing: the missing ones last. */
export function compareText(a: string | null | undefined, b: string | null | undefined): number {
  if (a == null) return b == null ? 0 : 1;
  if (b == null) return -1;
  return a.localeCompare(b);
}

/** The clients the controller knows, by MAC address and by IP address. */
export interface ClientIndex {
  byMac: ReadonlyMap<string, Client>;
  byIp: ReadonlyMap<string, Client>;
}

/** Indexes the clients; an address two clients held goes to the one online (else the first). */
export function indexClients(clients: readonly Client[]): ClientIndex {
  const byMac = new Map<string, Client>();
  const byIp = new Map<string, Client>();
  for (const client of clients) {
    byMac.set(client.mac.toLowerCase(), client);
    for (const ip of client.ips) {
      const key = ip.toLowerCase();
      if (!byIp.has(key) || client.online) byIp.set(key, client);
    }
  }
  return { byMac, byIp };
}

/** The client a neighbor entry is: by its MAC address, else by its IP address. */
export function neighborClient(entry: NeighborEntry, index: ClientIndex): Client | undefined {
  return (
    (entry.mac ? index.byMac.get(entry.mac.toLowerCase()) : undefined) ??
    index.byIp.get(entry.address.toLowerCase())
  );
}

const STATE_TONES: Record<string, Tone> = {
  REACHABLE: "ok",
  PERMANENT: "info",
  NOARP: "info",
  STALE: "neutral",
  DELAY: "neutral",
  PROBE: "neutral",
  INCOMPLETE: "warn",
  FAILED: "crit",
};

/** The tone of a neighbor's state. */
export function neighborTone(state: string | null): Tone {
  return (state && STATE_TONES[state]) || "neutral";
}

/** An address and port as one endpoint: `[2001:db8::1]:443` for IPv6. */
export function endpoint(address: string, port: number | null): string {
  if (port === null) return address;
  return address.includes(":") ? `[${address}]:${port}` : `${address}:${port}`;
}

const sameEnd = (address: string, port: number | null, other: string, otherPort: number | null) =>
  address.toLowerCase() === other.toLowerCase() && port === otherPort;

/**
 * Where NAT moved a connection: the destination it was forwarded to (a port forward), and the
 * address it went out as (masquerading), each when it differs from what the first packet asked.
 */
export function natOf(connection: TrackedConnection): { dnat?: string; snat?: string } {
  const { original, reply } = connection;
  const out: { dnat?: string; snat?: string } = {};
  if (!sameEnd(reply.src, reply.sport, original.dst, original.dport)) {
    out.dnat = endpoint(reply.src, reply.sport);
  }
  if (!sameEnd(reply.dst, reply.dport, original.src, original.sport)) {
    out.snat = endpoint(reply.dst, reply.dport);
  }
  return out;
}

/** Both directions' bytes; none where the kernel doesn't count them (`nf_conntrack_acct`). */
export function trafficOf(connection: TrackedConnection): number | null {
  const bytes = (flow: TrackedFlow) => flow.bytes;
  const a = bytes(connection.original);
  const b = bytes(connection.reply);
  return a === null && b === null ? null : (a ?? 0) + (b ?? 0);
}
