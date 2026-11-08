import type { ForwardProto } from "../../api/gen/ForwardProto";
import type { Network } from "../../api/gen/Network";
import type { PortForward } from "../../api/gen/PortForward";
import type { PortRange } from "../../api/gen/PortRange";
import type { SiteConfig } from "../../api/gen/SiteConfig";
import { cidr, formatIpv4, ipv4, subnetText } from "../../lib/ipv4";
import { strings } from "../../lib/strings";

const e = strings.security.forward.errors;

/** The DMZ host's redirect name, which no forward may take. */
const DMZ = "DMZ";
const NAME_MAX = 48;

/** A forward as its form holds it: ports and addresses as typed. */
export interface ForwardDraft {
  name: string;
  enabled: boolean;
  proto: ForwardProto;
  ports: string;
  to: string;
  toPort: string;
  source: string;
  reflection: boolean;
}

export type ForwardField = "name" | "ports" | "to" | "to_port" | "source";
export type ForwardErrors = Partial<Record<ForwardField, string>>;

export function forwardDraft(forward?: PortForward): ForwardDraft {
  return {
    name: forward?.name ?? "",
    enabled: forward?.enabled ?? true,
    proto: forward?.proto ?? "tcp",
    ports: forward ? portsText(forward.ports) : "",
    to: forward?.to ?? "",
    toPort: forward?.to_port?.toString() ?? "",
    source: forward?.source ?? "",
    reflection: forward?.reflection ?? true,
  };
}

/** `80`, or `8000-8100`. */
export function portsText(range: PortRange): string {
  return range.first === range.last ? `${range.first}` : `${range.first}-${range.last}`;
}

/** A port from 1, as typed. */
function port(text: string): number | undefined {
  const t = text.trim();
  if (!/^\d{1,5}$/.test(t)) return undefined;
  const n = Number(t);
  return n >= 1 && n <= 65535 ? n : undefined;
}

/** `80` or `8000-8100` (or `8000:8100`), the first no larger than the last. */
export function parsePorts(text: string): PortRange | undefined {
  const [a, b, ...rest] = text.split(/[-:]/);
  if (rest.length > 0) return undefined;
  const first = port(a ?? "");
  const last = b === undefined ? first : port(b);
  if (first === undefined || last === undefined || first > last) return undefined;
  return { first, last };
}

/** The host's ports: from `to_port`, or the WAN's. */
export function hostPorts(forward: PortForward): PortRange {
  if (forward.to_port === null) return forward.ports;
  return {
    first: forward.to_port,
    last: forward.to_port + forward.ports.last - forward.ports.first,
  };
}

/** The network whose subnet holds `ip`. */
export function networkOfHost(networks: readonly Network[], ip: string): Network | undefined {
  const value = ipv4(ip);
  if (value === undefined) return undefined;
  return networks.find((n) => {
    const subnet = n.ipv4 ? cidr(n.ipv4.address) : undefined;
    return subnet !== undefined && value >= subnet.network && value <= subnet.broadcast;
  });
}

/** Why `text` can't be a host's address on the networks, as the server checks it. */
export function hostError(networks: readonly Network[], text: string): string | undefined {
  const value = ipv4(text);
  if (value === undefined) return e.ip;
  const network = networkOfHost(networks, text);
  const subnet = network?.ipv4 ? cidr(network.ipv4.address) : undefined;
  if (!subnet) return e.offNetworks;
  if (value === subnet.addr) return e.gateway;
  if (subnet.len < 31 && (value === subnet.network || value === subnet.broadcast)) {
    return e.notHost(subnetText(subnet));
  }
  return undefined;
}

/** An address or a subnet as the server reads it: a bare address is a /32. */
function source(text: string): string | null | undefined {
  const t = text.trim();
  if (!t) return null;
  if (ipv4(t) !== undefined) return `${formatIpv4(ipv4(t) ?? 0)}/32`;
  const net = cidr(t);
  return net ? `${formatIpv4(net.addr)}/${net.len}` : undefined;
}

/** Whether two forwards claim one of the same WAN ports for one of the same protocols. */
function clashes(a: PortForward, b: PortForward): boolean {
  const protos = (p: ForwardProto) => (p === "tcp_udp" ? ["tcp", "udp"] : [p]);
  return (
    a.ports.first <= b.ports.last &&
    b.ports.first <= a.ports.last &&
    protos(a.proto).some((p) => protos(b.proto).includes(p))
  );
}

/** The draft's forward, or its errors; `others` are the site's other forwards. */
export function forwardOf(
  draft: ForwardDraft,
  others: readonly PortForward[],
  networks: readonly Network[],
): { forward?: PortForward; errors: ForwardErrors } {
  const errors: ForwardErrors = {};
  const name = draft.name;
  if (!name.trim() || name.trim().length > NAME_MAX || /\p{Cc}/u.test(name)) errors.name = e.name;
  else if (name.trim() !== name) errors.name = e.nameSpaces;
  else if (name.toLowerCase() === DMZ.toLowerCase()) errors.name = e.nameDmz;
  else if (others.some((f) => f.name.toLowerCase() === name.toLowerCase())) {
    errors.name = e.nameTaken;
  }
  const ports = parsePorts(draft.ports);
  if (!ports) errors.ports = e.ports;
  const toPort = draft.toPort.trim() ? port(draft.toPort) : null;
  if (toPort === undefined) errors.to_port = e.toPort;
  else if (ports && toPort !== null && toPort + ports.last - ports.first > 65535) {
    errors.to_port = e.toPort;
  }
  const to = hostError(networks, draft.to);
  if (to) errors.to = to;
  const from = source(draft.source);
  if (from === undefined) errors.source = e.source;
  if (Object.keys(errors).length > 0 || !ports || toPort === undefined || from === undefined) {
    return { errors };
  }
  const forward: PortForward = {
    name,
    enabled: draft.enabled,
    proto: draft.proto,
    ports,
    to: formatIpv4(ipv4(draft.to) ?? 0),
    to_port: toPort === ports.first ? null : toPort,
    source: from,
    reflection: draft.reflection,
  };
  const clash = forward.enabled && others.find((f) => f.enabled && clashes(forward, f));
  if (clash) return { errors: { ports: e.clash(clash.name) } };
  return { forward, errors };
}

/** The server's issues on `firewall/forwards/<index>/…` as the form's errors. */
export function forwardErrorsOfIssues(
  issues: readonly { path: string; message: string }[],
  index: number,
): { errors: ForwardErrors; other: string[] } {
  const errors: ForwardErrors = {};
  const other: string[] = [];
  for (const issue of issues) {
    const match = issue.path.match(/^firewall\/forwards\/(\d+)\/(name|ports|to|to_port|source)$/);
    const field = match?.[2] as ForwardField | undefined;
    if (match && Number(match[1]) === index && field) errors[field] ??= issue.message;
    else other.push(issue.message);
  }
  return { errors, other };
}

/**
 * `config` with the forward named `name` replaced by `forward` (or deleted with `null`);
 * without a name (or one another save removed), `forward` is added. Names are unique, so they key forwards across revisions.
 */
export function withForward(
  config: SiteConfig,
  name: string | undefined,
  forward: PortForward | null,
): SiteConfig {
  const now = config.firewall.forwards;
  const forwards =
    name === undefined || !now.some((f) => f.name === name)
      ? [...now, ...(forward ? [forward] : [])]
      : now.flatMap((f) => (f.name === name ? (forward ? [forward] : []) : [f]));
  return { ...config, firewall: { ...config.firewall, forwards } };
}
