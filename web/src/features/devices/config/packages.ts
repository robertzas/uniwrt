import type { ManagedPackage } from "../../../api/gen/ManagedPackage";
import type { NodeId } from "../../../api/gen/NodeId";
import type { SiteConfig } from "../../../api/gen/SiteConfig";

/** The packages render writes today (lifecycle.md, Render): only these can be chosen. */
export const RENDERED: readonly ManagedPackage[] = [
  "system",
  "network",
  "wireless",
  "dhcp",
  "firewall",
];

/** Every package uniwrt will own (D-011), in the server's order. */
export const PACKAGES: readonly ManagedPackage[] = [
  ...RENDERED,
  "dropbear",
  "usteer",
  "sqm",
  "mwan3",
  "upnpd",
  "wireguard",
];

/** In the server's order, each once. */
function ordered(packages: Iterable<ManagedPackage>): ManagedPackage[] {
  const chosen = new Set(packages);
  return PACKAGES.filter((name) => chosen.has(name));
}

/** The packages `node` manages in `config`: none without an entry. */
export function managedOf(config: SiteConfig, node: NodeId): ManagedPackage[] {
  return ordered(config.nodes.find((entry) => entry.node === node)?.managed ?? []);
}

/** `config` with `node` managing `managed`, adding the node's entry if it has none. */
export function withManaged(
  config: SiteConfig,
  node: NodeId,
  managed: Iterable<ManagedPackage>,
): SiteConfig {
  const chosen = ordered(managed);
  const known = config.nodes.some((entry) => entry.node === node);
  const nodes = known
    ? config.nodes.map((entry) => (entry.node === node ? { ...entry, managed: chosen } : entry))
    : [...config.nodes, { node, hostname: null, managed: chosen, radios: [], ports: [], raw: [] }];
  return { ...config, nodes };
}

/** What a change of choice adds and removes, in the server's order. */
export function changeOf(
  before: Iterable<ManagedPackage>,
  after: Iterable<ManagedPackage>,
): { added: ManagedPackage[]; removed: ManagedPackage[] } {
  const was = new Set(before);
  const now = new Set(after);
  return {
    added: PACKAGES.filter((name) => now.has(name) && !was.has(name)),
    removed: PACKAGES.filter((name) => was.has(name) && !now.has(name)),
  };
}
