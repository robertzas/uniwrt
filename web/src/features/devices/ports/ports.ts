import type { Network } from "../../../api/gen/Network";
import type { NodeConfig } from "../../../api/gen/NodeConfig";
import type { NodeFacts } from "../../../api/gen/NodeFacts";
import type { PortConfig } from "../../../api/gen/PortConfig";
import type { SiteConfig } from "../../../api/gen/SiteConfig";
import { strings } from "../../../lib/strings";

const t = strings.ports;

/** One Ethernet port of a node, as the Ports tab lists it. */
export interface PortRow {
  /** As a profile names it: `lan2`, `eth1`, or a switch port such as `switch0:3`. */
  port: string;
  /** Its link, when the node reports the device (a switch's ports have none of their own). */
  link?: { carrier: boolean; speed: number | null };
  /** board.json lists it: a profile naming another port is left out by render. */
  present: boolean;
}

/**
 * The node's LAN ports from board.json (a swconfig switch's user ports by number, else the LAN's
 * devices), then the ports its profiles name that the board lacks.
 */
export function portsOf(facts: NodeFacts | null, profiles: readonly PortConfig[]): PortRow[] {
  const layout = facts?.board.layout;
  const sw = layout?.switch;
  const names = sw
    ? sw.lan_ports.map((n) => `${sw.name}:${n}`)
    : (layout?.lan ?? []).filter((name) => name !== layout?.wan);
  const rows: PortRow[] = names.map((port) => {
    const device = facts?.devices.find((d) => d.name === port);
    return {
      port,
      present: true,
      ...(device && { link: { carrier: device.carrier, speed: device.speed_mbps } }),
    };
  });
  for (const profile of profiles) {
    if (!names.includes(profile.port)) rows.push({ port: profile.port, present: false });
  }
  return rows;
}

/** A port's profile as its row edits it; `native` is `""` for none. */
export interface PortDraft {
  native: string;
  tagged: "all" | "none" | "only";
  only: string[];
}

/** The site's untagged network, which a trunk carries untagged. */
export function untaggedOf(networks: readonly Network[]): string {
  return networks.find((n) => n.vlan === null)?.id ?? "";
}

/** The profile's draft; a port without one is a trunk. */
export function portDraft(profile: PortConfig | undefined, untagged: string): PortDraft {
  if (!profile) return { native: untagged, tagged: "all", only: [] };
  return {
    native: profile.native ?? "",
    tagged: profile.tagged.kind,
    only: profile.tagged.kind === "only" ? [...profile.tagged.networks] : [],
  };
}

/** The draft's profile, or `null` for a trunk (no profile). */
export function profileOf(port: string, draft: PortDraft, untagged: string): PortConfig | null {
  if (draft.native === untagged && draft.tagged === "all") return null;
  return {
    port,
    native: draft.native === "" ? null : draft.native,
    tagged:
      draft.tagged === "only"
        ? { kind: "only", networks: draft.only.filter((id) => id !== draft.native) }
        : { kind: draft.tagged },
  };
}

/** What the port carries, in words. */
export function carriesOf(
  draft: PortDraft,
  untagged: string,
  networks: readonly Network[],
): string {
  const name = (id: string) => networks.find((n) => n.id === id)?.name ?? id;
  if (draft.native === untagged && draft.tagged === "all") return t.trunk;
  const none = draft.tagged === "none" || (draft.tagged === "only" && draft.only.length === 0);
  if (none) return draft.native === "" ? t.off : t.access(name(draft.native));
  return t.custom;
}

/** `config` with the node's port profiles replaced; a node without an entry gets one. */
export function withPorts(config: SiteConfig, node: number, ports: PortConfig[]): SiteConfig {
  const known = config.nodes.some((entry) => entry.node === node);
  const blank: NodeConfig = { node, hostname: null, managed: [], radios: [], ports: [], raw: [] };
  const nodes = known ? config.nodes : [...config.nodes, blank];
  return {
    ...config,
    nodes: nodes.map((entry) => (entry.node === node ? { ...entry, ports } : entry)),
  };
}
