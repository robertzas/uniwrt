import type { AdoptionPort } from "../../../api/gen/AdoptionPort";
import type { NodeFacts } from "../../../api/gen/NodeFacts";
import type { SiteConfig } from "../../../api/gen/SiteConfig";

/** The gateway's address on the segment unless the admin picks another (`AdoptionPort::DEFAULT`). */
export const ADOPTION_ADDRESS = "192.168.1.250";

/**
 * The gateway's ports that can be the adoption port: its LAN's own devices (render refuses the
 * WAN port and a swconfig switch's ports), and the port set now even when the board lacks it.
 */
export function adoptionPorts(facts: NodeFacts | null, current: string | null): string[] {
  const layout = facts?.board.layout;
  const ports = layout?.switch ? [] : (layout?.lan ?? []).filter((name) => name !== layout?.wan);
  return current && !ports.includes(current) ? [...ports, current] : ports;
}

/**
 * Why `address` can't be the gateway's on the segment, if it can't: a host of 192.168.1.0/24 other
 * than .1, where factory-default OpenWrt answers (the site's validation says the same).
 */
export function addressError(address: string): "format" | "factory" | null {
  const match = /^192\.168\.1\.(\d{1,3})$/.exec(address.trim());
  const host = match ? Number(match[1]) : Number.NaN;
  if (!match || match[1]?.startsWith("0") || host < 1 || host > 254) return "format";
  return host === 1 ? "factory" : null;
}

/** `config` with the adoption port set, or removed (`null`). */
export function withAdoptionPort(config: SiteConfig, port: AdoptionPort | null): SiteConfig {
  return { ...config, adoption_port: port };
}
