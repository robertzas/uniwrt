import type { Ipv6Clients } from "../../api/gen/Ipv6Clients";
import type { Ipv6Mode } from "../../api/gen/Ipv6Mode";
import type { Network } from "../../api/gen/Network";
import type { Purpose } from "../../api/gen/Purpose";
import type { SiteConfig } from "../../api/gen/SiteConfig";
import { cidr, ipv4, isHost, isPrivate } from "../../lib/ipv4";
import { isDnsName, isHostname, normalizeMac } from "../../lib/names";
import { strings } from "../../lib/strings";

const e = strings.networks.errors;
const u = strings.networks.editor.uses;

/** The lease times offered, in seconds (OpenWrt's default is 12 hours). */
export const LEASES = [1800, 3600, 7200, 21600, 43200, 86400, 604800] as const;

export interface ReservationDraft {
  mac: string;
  ip: string;
  name: string;
}

/** A network as its editor holds it: text as typed. */
export interface NetworkDraft {
  id: string;
  name: string;
  purpose: Purpose;
  vlan: string;
  ipv4: boolean;
  address: string;
  dhcp: boolean;
  start: string;
  end: string;
  /** Seconds, as a select's value. */
  lease: string;
  dns: string;
  ntp: string;
  domain: string;
  /** One option per line. */
  custom: string;
  boot: boolean;
  bootFile: string;
  bootName: string;
  bootServer: string;
  ipv6: Ipv6Mode;
  ipv6Clients: Ipv6Clients;
  isolation: boolean;
  mdns: boolean;
  igmp: boolean;
  reservations: ReservationDraft[];
}

/**
 * The fields an error can name; a reservation's are `r<index>.mac`, `r<index>.ip` and
 * `r<index>.name`.
 */
export type DraftErrors = Partial<Record<string, string>>;

export function draftOf(network: Network): NetworkDraft {
  const v4 = network.ipv4;
  const dhcp = v4?.dhcp;
  const options = dhcp?.options;
  const boot = options?.boot;
  return {
    id: network.id,
    name: network.name,
    purpose: network.purpose,
    vlan: network.vlan === null ? "" : String(network.vlan),
    ipv4: v4 !== null,
    address: v4?.address ?? "",
    dhcp: dhcp != null,
    start: dhcp?.start ?? "",
    end: dhcp?.end ?? "",
    lease: String(dhcp?.lease_time_s ?? 43200),
    dns: options?.dns.join(", ") ?? "",
    ntp: options?.ntp.join(", ") ?? "",
    domain: options?.domain ?? "",
    custom: options?.custom.join("\n") ?? "",
    boot: boot != null,
    bootFile: boot?.filename ?? "",
    bootName: boot?.server_name ?? "",
    bootServer: boot?.server ?? "",
    ipv6: network.ipv6,
    ipv6Clients: network.ipv6_clients,
    isolation: network.isolation,
    mdns: network.mdns,
    igmp: network.igmp_snooping,
    reservations: (dhcp?.reservations ?? []).map((r) => ({
      mac: r.mac,
      ip: r.ip,
      name: r.name ?? "",
    })),
  };
}

const ID = /^[a-z][a-z0-9_]{0,11}$/;
/** Ids render's own interfaces use (`validate/network.rs`). */
const RESERVED = ["loopback", "wan", "wan6", "wan_6", "wanb", "bat0"];

function list(text: string): string[] {
  return text
    .split(/[,\s]+/)
    .map((item) => item.trim())
    .filter((item) => item !== "");
}

/** Up to 4 IPv4 addresses, or `undefined`. */
function servers(text: string): string[] | undefined {
  const items = list(text);
  return items.length <= 4 && items.every((ip) => ipv4(ip) !== undefined) ? items : undefined;
}

/** A DHCP option dnsmasq reads as render writes it: a code or `option:<name>`, `,`, a value. */
function isOption(line: string): boolean {
  const comma = line.indexOf(",");
  if (comma < 1) return false;
  const code = line.slice(0, comma);
  const value = line.slice(comma + 1);
  const named = code.startsWith("option:") && /^[a-z-]+$/.test(code.slice(7));
  const numbered = /^\d{1,3}$/.test(code) && Number(code) >= 1 && Number(code) <= 254;
  return (named || numbered) && /^[\x21-\x7e]+$/.test(value);
}

/**
 * The editor's network, or what is wrong with it, checked as the server checks it except what
 * needs the whole site (overlaps, the WAN): those come back as the save's issues. `others` are
 * the site's other networks; `isNew` lets the id be chosen.
 */
export function networkOf(
  draft: NetworkDraft,
  others: readonly Network[],
  isNew: boolean,
): { network?: Network; errors: DraftErrors } {
  const errors: DraftErrors = {};
  const id = draft.id.trim();
  if (isNew) {
    if (!ID.test(id)) errors.id = e.id;
    else if (RESERVED.includes(id) || id.startsWith("uw_")) errors.id = e.idReserved;
    else if (others.some((n) => n.id === id)) errors.id = e.idTaken;
  }
  const name = draft.name.trim();
  if (name === "" || [...name].length > 32) errors.name = e.name;
  else if (others.some((n) => n.name.trim().toLowerCase() === name.toLowerCase())) {
    errors.name = e.nameTaken;
  }
  const vlan = draft.vlan.trim() === "" ? null : Number(draft.vlan.trim());
  if (vlan !== null && !(Number.isInteger(vlan) && vlan >= 1 && vlan <= 4094)) errors.vlan = e.vlan;
  else {
    const taken = others.find((n) => n.vlan === vlan);
    if (taken) errors.vlan = vlan === null ? e.untaggedTaken(taken.name) : e.vlanTaken(taken.name);
  }

  let v4: Network["ipv4"] = null;
  if (draft.ipv4) {
    const subnet = cidr(draft.address);
    if (!subnet || subnet.len < 8 || subnet.len > 30 || !isPrivate(subnet.addr)) {
      errors.address = e.address;
    } else if (!isHost(subnet, subnet.addr)) {
      errors.address = e.host;
    }
    let dhcp: NonNullable<Network["ipv4"]>["dhcp"] = null;
    if (draft.dhcp && subnet) {
      const start = ipv4(draft.start);
      const end = ipv4(draft.end);
      if (start === undefined) errors.start = e.ip;
      else if (!isHost(subnet, start)) errors.start = e.outside;
      if (end === undefined) errors.end = e.ip;
      else if (!isHost(subnet, end)) errors.end = e.outside;
      if (start !== undefined && end !== undefined && !errors.start && !errors.end) {
        if (start > end) errors.end = e.poolOrder;
        else if (subnet.addr >= start && subnet.addr <= end) errors.start = e.poolGateway;
      }
      const dns = servers(draft.dns);
      const ntp = servers(draft.ntp);
      if (!dns) errors.dns = e.servers;
      if (!ntp) errors.ntp = e.servers;
      const domain = draft.domain.trim();
      if (domain !== "" && !isDnsName(domain)) errors.domain = e.domain;
      const custom = draft.custom
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line !== "");
      const bad = custom.find((line) => !isOption(line));
      if (bad) errors.custom = e.custom(bad);
      const bootFile = draft.bootFile.trim();
      const bootName = draft.bootName.trim();
      const bootServer = draft.bootServer.trim();
      if (draft.boot) {
        if (!/^[\x21-\x7e]+$/.test(bootFile) || bootFile.includes(","))
          errors.bootFile = e.bootFile;
        if (bootName !== "" && !isDnsName(bootName)) errors.bootName = e.domain;
        if (bootServer !== "" && ipv4(bootServer) === undefined) errors.bootServer = e.ip;
        if (bootServer !== "" && bootName === "") errors.bootName = e.bootName;
      }
      const reservations = reservationsOf(draft.reservations, subnet, errors);
      dhcp = {
        start: draft.start.trim(),
        end: draft.end.trim(),
        lease_time_s: Number(draft.lease),
        reservations,
        options: {
          dns: dns ?? [],
          ntp: ntp ?? [],
          domain: domain === "" ? null : domain,
          boot: draft.boot
            ? {
                filename: bootFile,
                server_name: bootName === "" ? null : bootName,
                server: bootServer === "" ? null : bootServer,
              }
            : null,
          custom,
        },
      };
    }
    v4 = { address: draft.address.trim(), dhcp };
  }

  if (Object.keys(errors).length > 0) return { errors };
  return {
    network: {
      id,
      name,
      purpose: draft.purpose,
      vlan,
      ipv4: v4,
      ipv6: draft.ipv6,
      ipv6_clients: draft.ipv6Clients,
      isolation: draft.isolation,
      mdns: draft.mdns,
      igmp_snooping: draft.igmp,
    },
    errors,
  };
}

function reservationsOf(
  drafts: readonly ReservationDraft[],
  subnet: NonNullable<ReturnType<typeof cidr>>,
  errors: DraftErrors,
) {
  const macs = new Set<string>();
  const ips = new Set<number>();
  return drafts.map((draft, i) => {
    const mac = normalizeMac(draft.mac);
    const ip = ipv4(draft.ip);
    const name = draft.name.trim();
    if (!mac) errors[`r${i}.mac`] = e.mac;
    else if (macs.has(mac)) errors[`r${i}.mac`] = e.macTwice;
    else macs.add(mac);
    if (ip === undefined) errors[`r${i}.ip`] = e.ip;
    else if (!isHost(subnet, ip) || ip === subnet.addr) errors[`r${i}.ip`] = e.outside;
    else if (ips.has(ip)) errors[`r${i}.ip`] = e.ipTwice;
    else ips.add(ip);
    if (name !== "" && !isHostname(name)) errors[`r${i}.name`] = e.hostname;
    return { mac: mac ?? draft.mac, ip: draft.ip.trim(), name: name === "" ? null : name };
  });
}

const FIELDS: Record<string, string> = {
  name: "name",
  vlan: "vlan",
  "ipv4/address": "address",
  "ipv4/dhcp/start": "start",
  "ipv4/dhcp/end": "end",
  "ipv4/dhcp/lease_time_s": "lease",
  "ipv4/dhcp/options/dns": "dns",
  "ipv4/dhcp/options/ntp": "ntp",
  "ipv4/dhcp/options/domain": "domain",
  "ipv4/dhcp/options/boot/filename": "bootFile",
  "ipv4/dhcp/options/boot/server_name": "bootName",
  "ipv4/dhcp/options/boot/server": "bootServer",
  ipv6: "ipv6",
  ipv6_clients: "ipv6Clients",
};

/**
 * The server's issues (a 422, or a saved document's warnings) on the network `id` as the
 * editor's errors; the others, and those of no field, apart.
 */
export function errorsOfIssues(
  issues: readonly { path: string; message: string }[],
  id: string,
  reservations: readonly ReservationDraft[],
): { errors: DraftErrors; other: string[] } {
  const errors: DraftErrors = {};
  const other: string[] = [];
  const prefix = `networks/${id}/`;
  for (const issue of issues) {
    const rest = issue.path.startsWith(prefix) ? issue.path.slice(prefix.length) : undefined;
    let field = rest === undefined ? undefined : FIELDS[rest];
    if (rest?.startsWith("ipv4/dhcp/options/custom")) field = "custom";
    const reserved = rest?.match(/^ipv4\/dhcp\/reservations\/([^/]+)(?:\/(ip|name))?$/);
    if (reserved) {
      const index = reservations.findIndex((r) => normalizeMac(r.mac) === reserved[1]);
      if (index >= 0) field = `r${index}.${reserved[2] ?? "mac"}`;
    }
    if (issue.path === `networks/${id}`) field = "id";
    if (field) errors[field] ??= issue.message;
    else other.push(issue.message);
  }
  return { errors, other };
}

/**
 * What in the site refers to network `id`, in words: its WLANs, the nodes whose ports name it,
 * UPnP, and the port forwards and DMZ host on its subnet.
 */
export function usesOf(config: SiteConfig, id: string, nodeName: (id: number) => string): string[] {
  const wlans = config.wlans.filter((wlan) => wlan.network === id).map((wlan) => wlan.ssid);
  const nodes = config.nodes
    .filter((node) =>
      node.ports.some(
        (port) =>
          port.native === id || (port.tagged.kind === "only" && port.tagged.networks.includes(id)),
      ),
    )
    .map((node) => nodeName(node.node));
  const address = config.networks.find((n) => n.id === id)?.ipv4?.address;
  const subnet = address ? cidr(address) : undefined;
  const inSubnet = (text: string) => {
    const ip = ipv4(text);
    return (
      subnet !== undefined && ip !== undefined && ip >= subnet.network && ip <= subnet.broadcast
    );
  };
  const firewall = config.firewall;
  return [
    ...wlans,
    ...nodes,
    ...(firewall.upnp.networks.includes(id) ? [u.upnp] : []),
    ...firewall.forwards.filter((f) => inSubnet(f.to)).map((f) => u.forward(f.name)),
    ...(firewall.dmz !== null && inSubnet(firewall.dmz) ? [u.dmz] : []),
  ];
}

/**
 * `config` with `network` in place of the one with its id, or added; `remove` drops `id` and
 * the firewall policies naming it.
 */
export function withNetwork(config: SiteConfig, network: Network | { remove: string }): SiteConfig {
  if ("remove" in network) {
    const gone = network.remove;
    // The firewall's cells to and from the network go with it.
    const policies = config.firewall.policies.filter((p) => p.from !== gone && p.to !== gone);
    return {
      ...config,
      networks: config.networks.filter((n) => n.id !== gone),
      firewall: { ...config.firewall, policies },
    };
  }
  const known = config.networks.some((n) => n.id === network.id);
  return {
    ...config,
    networks: known
      ? config.networks.map((n) => (n.id === network.id ? network : n))
      : [...config.networks, network],
  };
}
