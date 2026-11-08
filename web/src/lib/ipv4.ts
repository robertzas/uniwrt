/** IPv4 addresses and subnets as numbers, for the forms' checks and the pools' counts. */

/** A dotted quad as a 32-bit number, or `undefined`. */
export function ipv4(text: string): number | undefined {
  const parts = text.trim().split(".");
  if (parts.length !== 4) return undefined;
  let value = 0;
  for (const part of parts) {
    if (!/^\d{1,3}$/.test(part) || Number(part) > 255) return undefined;
    value = value * 256 + Number(part);
  }
  return value;
}

export function formatIpv4(value: number): string {
  return [24, 16, 8, 0].map((shift) => Math.floor(value / 2 ** shift) % 256).join(".");
}

/** An address with its prefix length, such as the gateway's `10.0.30.1/24`. */
export interface Cidr {
  addr: number;
  len: number;
  /** The subnet's first address. */
  network: number;
  /** The subnet's last address. */
  broadcast: number;
}

export function cidr(text: string): Cidr | undefined {
  const [ip, len, ...rest] = text.trim().split("/");
  const addr = ipv4(ip ?? "");
  if (addr === undefined || rest.length > 0 || !len || !/^\d{1,2}$/.test(len)) return undefined;
  const bits = Number(len);
  if (bits > 32) return undefined;
  const size = 2 ** (32 - bits);
  const network = Math.floor(addr / size) * size;
  return { addr, len: bits, network, broadcast: network + size - 1 };
}

/** `ip` is a host of the subnet: inside it, neither its first nor its last address. */
export function isHost(subnet: Cidr, ip: number): boolean {
  return ip > subnet.network && ip < subnet.broadcast;
}

/** The subnet as `10.0.30.0/24`. */
export function subnetText(subnet: Cidr): string {
  return `${formatIpv4(subnet.network)}/${subnet.len}`;
}

/** The private (RFC 1918) ranges. */
export function isPrivate(ip: number): boolean {
  const a = Math.floor(ip / 2 ** 24);
  const b = Math.floor(ip / 2 ** 16) % 256;
  return a === 10 || (a === 172 && b >= 16 && b < 32) || (a === 192 && b === 168);
}
