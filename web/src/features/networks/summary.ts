import type { Client } from "../../api/gen/Client";
import type { Issue } from "../../api/gen/Issue";
import type { LeaseFacts } from "../../api/gen/LeaseFacts";
import type { Network } from "../../api/gen/Network";
import { cidr, ipv4, subnetText } from "../../lib/ipv4";

/** What the networks list shows of one network: its subnet and how full it is. */
export interface NetworkUse {
  /** `10.0.30.0/24`, or `undefined` without IPv4. */
  subnet?: string;
  gateway?: string;
  /** The pool's size and the leases in it, when DHCP is on. */
  pool?: { start: string; end: string; size: number; leased: number };
  /** Online clients with an address in the subnet. */
  clients: number;
  reservations: number;
}

/** `network`'s use from the gateway's leases and the live clients. */
export function useOf(
  network: Network,
  leases: readonly LeaseFacts[],
  clients: readonly Client[],
): NetworkUse {
  const v4 = network.ipv4;
  const subnet = v4 ? cidr(v4.address) : undefined;
  if (!v4 || !subnet) return { clients: 0, reservations: 0 };
  const inSubnet = (text: string) => {
    const ip = ipv4(text);
    return ip !== undefined && ip >= subnet.network && ip <= subnet.broadcast;
  };
  const dhcp = v4.dhcp;
  const start = dhcp ? ipv4(dhcp.start) : undefined;
  const end = dhcp ? ipv4(dhcp.end) : undefined;
  const pool =
    dhcp && start !== undefined && end !== undefined && end >= start
      ? {
          start: dhcp.start,
          end: dhcp.end,
          size: end - start + 1,
          leased: new Set(
            leases
              .map((lease) => ipv4(lease.ip))
              .filter((ip): ip is number => ip !== undefined && ip >= start && ip <= end),
          ).size,
        }
      : undefined;
  return {
    subnet: subnetText(subnet),
    gateway: v4.address.split("/")[0],
    ...(pool && { pool }),
    clients: clients.filter((client) => client.online && client.ips.some(inSubnet)).length,
    reservations: dhcp?.reservations.length ?? 0,
  };
}

/** The saved document's warnings about network `id`. */
export function warningsOf(issues: readonly Issue[], id: string): string[] {
  const at = `networks/${id}`;
  return issues
    .filter((issue) => issue.path === at || issue.path.startsWith(`${at}/`))
    .map((issue) => issue.message);
}
