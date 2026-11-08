import { describe, expect, it } from "vitest";
import type { Network } from "../../api/gen/Network";
import { cidr, formatIpv4, ipv4, isHost } from "../../lib/ipv4";
import { sortNetworks } from "../../lib/site";
import { client } from "../../test/clients";
import { siteConfig } from "../../test/site";
import { recordErrorsOfIssues, recordsOf } from "./dns";
import { draftOf, errorsOfIssues, networkOf, usesOf, withNetwork } from "./draft";
import { useOf, warningsOf } from "./summary";

function iot(extra: Partial<Network> = {}): Network {
  return {
    id: "iot",
    name: "IoT",
    purpose: "iot",
    vlan: 30,
    ipv4: {
      address: "10.0.30.1/24",
      dhcp: {
        start: "10.0.30.100",
        end: "10.0.30.249",
        lease_time_s: 43200,
        reservations: [{ mac: "02:00:00:00:00:01", ip: "10.0.30.40", name: "plug" }],
        options: {
          dns: ["10.0.30.1"],
          ntp: [],
          domain: null,
          boot: { filename: "pxelinux.0", server_name: "tftp", server: "10.0.30.5" },
          custom: ["26,1400"],
        },
      },
    },
    ipv6: "delegated",
    ipv6_clients: "slaac_stateless",
    isolation: true,
    mdns: false,
    igmp_snooping: false,
    ...extra,
  };
}

const lan = () => siteConfig().networks;

describe("addresses", () => {
  it("parses addresses and subnets", () => {
    expect(ipv4("10.0.30.1")).toBe(10 * 2 ** 24 + 30 * 256 + 1);
    expect(ipv4("10.0.300.1")).toBeUndefined();
    expect(ipv4("10.0.30")).toBeUndefined();
    const subnet = cidr("10.0.30.1/24");
    expect(subnet && formatIpv4(subnet.network)).toBe("10.0.30.0");
    expect(subnet && formatIpv4(subnet.broadcast)).toBe("10.0.30.255");
    expect(subnet && isHost(subnet, subnet.broadcast)).toBe(false);
    expect(cidr("10.0.30.1")).toBeUndefined();
    expect(cidr("10.0.30.1/33")).toBeUndefined();
  });
});

describe("the network editor's draft", () => {
  it("reads a network back as it was", () => {
    expect(networkOf(draftOf(iot()), lan(), false)).toEqual({ network: iot(), errors: {} });
    const bare = iot({ ipv4: null, vlan: 40, ipv6: "off" });
    expect(networkOf(draftOf(bare), lan(), false).network).toEqual(bare);
  });

  it("checks fields as the server does", () => {
    const draft = {
      ...draftOf(iot()),
      id: "wan",
      name: "Default",
      vlan: "5000",
      start: "10.0.31.5",
      custom: "26 1400",
      bootName: "",
      dns: "10.0.30.1, nope",
    };
    expect(networkOf(draft, lan(), true).errors).toEqual({
      id: "Reserved for uniwrt's own interfaces",
      name: "Another network has this name",
      vlan: "A VLAN ID is 1 to 4094",
      start: "Not a host of the subnet",
      dns: "Up to 4 IPv4 addresses, comma-separated",
      custom: "“26 1400” is not code,value without spaces",
      bootName: "Name the TFTP server too: dnsmasq needs its name with its address",
    });
    const untagged = { ...draftOf(iot()), vlan: "" };
    expect(networkOf(untagged, lan(), false).errors.vlan).toMatch(/Default is untagged already/);
    const holding = { ...draftOf(iot()), start: "10.0.30.1" };
    expect(networkOf(holding, lan(), false).errors.start).toBe(
      "The pool holds the gateway's address",
    );
    const host = { ...draftOf(iot()), address: "10.0.30.0/24" };
    expect(networkOf(host, lan(), false).errors.address).toMatch(/not its first or last/);
  });

  it("checks reservations and normalizes their MAC addresses", () => {
    const draft = draftOf(iot());
    draft.reservations = [
      { mac: "02-00-00-00-00-AA", ip: "10.0.30.41", name: "" },
      { mac: "02:00:00:00:00:aa", ip: "10.0.30.41", name: "a_b" },
      { mac: "nope", ip: "10.0.30.1", name: "" },
    ];
    expect(networkOf(draft, lan(), false).errors).toEqual({
      "r1.mac": "This MAC address has another reservation",
      "r1.ip": "Reserved twice",
      "r1.name": "Letters, digits and hyphens, up to 63",
      "r2.mac": "A MAC address, such as 02:00:00:00:00:01",
      "r2.ip": "Not a host of the subnet",
    });
    draft.reservations = [{ mac: "02-00-00-00-00-AA", ip: "10.0.30.41", name: "" }];
    expect(networkOf(draft, lan(), false).network?.ipv4?.dhcp?.reservations).toEqual([
      { mac: "02:00:00:00:00:aa", ip: "10.0.30.41", name: null },
    ]);
  });

  it("puts the server's issues on their fields", () => {
    const draft = draftOf(iot());
    const { errors, other } = errorsOfIssues(
      [
        { path: "networks/iot/ipv4/address", message: "overlaps" },
        { path: "networks/iot/ipv4/dhcp/options/custom/0", message: "custom" },
        { path: "networks/iot/ipv4/dhcp/reservations/02:00:00:00:00:01/ip", message: "taken" },
        { path: "networks/iot", message: "id" },
        { path: "wlans/home/network", message: "elsewhere" },
      ],
      "iot",
      draft.reservations,
    );
    expect(errors).toEqual({ address: "overlaps", custom: "custom", "r0.ip": "taken", id: "id" });
    expect(other).toEqual(["elsewhere"]);
  });

  it("finds what refers to a network, and adds, replaces or removes it", () => {
    const config = { ...siteConfig(), networks: [...lan(), iot()] };
    config.wlans = [{ ...config.wlans[0], network: "iot" } as (typeof config.wlans)[0]];
    config.nodes = [
      {
        node: 2,
        hostname: null,
        managed: [],
        radios: [],
        ports: [{ port: "lan1", native: null, tagged: { kind: "only", networks: ["iot"] } }],
        raw: [],
      },
    ];
    expect(usesOf(config, "iot", (id) => `ap${id}`)).toEqual(["uniwrt-demo", "ap2"]);
    expect(usesOf(config, "lan", (id) => `ap${id}`)).toEqual(["UPnP"]);
    // UPnP, forwards and the DMZ host on its subnet use it too; policies go with it.
    config.firewall = {
      policies: [
        { from: "iot", to: "lan", action: "allow" },
        { from: "lan", to: "wan", action: "block" },
      ],
      forwards: [
        {
          name: "Camera",
          enabled: false,
          proto: "tcp",
          ports: { first: 8443, last: 8443 },
          to: "10.0.30.20",
          to_port: 443,
          source: null,
          reflection: true,
        },
      ],
      dmz: "10.0.30.21",
      upnp: { enabled: false, networks: ["iot"] },
    };
    expect(usesOf(config, "iot", (id) => `ap${id}`)).toEqual([
      "uniwrt-demo",
      "ap2",
      "UPnP",
      "the port forward Camera",
      "the DMZ host",
    ]);
    const removed = withNetwork(config, { remove: "iot" });
    expect(removed.networks.map((n) => n.id)).toEqual(["lan"]);
    expect(removed.firewall.policies).toEqual([{ from: "lan", to: "wan", action: "block" }]);
    const renamed = withNetwork(config, iot({ name: "Things" }));
    expect(renamed.networks.map((n) => n.name)).toEqual(["Default", "Things"]);
    expect(withNetwork(siteConfig(), iot()).networks).toHaveLength(2);
  });
});

describe("the networks list", () => {
  it("counts the pool's leases and the online clients", () => {
    const leases = [
      {
        ip: "10.0.30.100",
        mac: null,
        hostname: null,
        client_id: null,
        device: null,
        expires_in_s: 1,
      },
      {
        ip: "10.0.30.100",
        mac: null,
        hostname: null,
        client_id: null,
        device: null,
        expires_in_s: 1,
      },
      {
        ip: "10.0.30.40",
        mac: null,
        hostname: null,
        client_id: null,
        device: null,
        expires_in_s: 1,
      },
      {
        ip: "10.0.0.120",
        mac: null,
        hostname: null,
        client_id: null,
        device: null,
        expires_in_s: 1,
      },
    ];
    const clients = [
      client("02:00:00:00:00:01", { ips: ["10.0.30.40"] }),
      client("02:00:00:00:00:02", { ips: ["10.0.30.120"], online: false }),
      client("02:00:00:00:00:03", { ips: ["fd00::1", "10.0.30.101"] }),
    ];
    expect(useOf(iot(), leases, clients)).toEqual({
      subnet: "10.0.30.0/24",
      gateway: "10.0.30.1",
      pool: { start: "10.0.30.100", end: "10.0.30.249", size: 150, leased: 1 },
      clients: 2,
      reservations: 1,
    });
    expect(useOf(iot({ ipv4: null }), leases, clients)).toEqual({ clients: 0, reservations: 0 });
  });

  it("orders the networks and picks each one's warnings", () => {
    const guest = iot({ id: "guest", name: "Guest", purpose: "guest", vlan: 40 });
    const flat = iot({ id: "lab", name: "Lab", purpose: "custom", vlan: null });
    expect(sortNetworks([flat, guest, iot(), ...lan()]).map((n) => n.id)).toEqual([
      "lan",
      "iot",
      "guest",
      "lab",
    ]);
    const issues = [
      { path: "networks/iot/ipv4/address", severity: "warning" as const, message: "a" },
      { path: "networks/iotx", severity: "warning" as const, message: "b" },
    ];
    expect(warningsOf(issues, "iot")).toEqual(["a"]);
  });
});

describe("local DNS records", () => {
  it("checks records and keeps the controller's name", () => {
    const { records, errors } = recordsOf(
      [
        { kind: "host", name: "nas", value: "10.0.0.5" },
        { kind: "host", name: "nas6", value: "fd00::5" },
        { kind: "cname", name: "files", value: "nas.lan" },
      ],
      "lan",
    );
    expect(errors).toEqual([{}, {}, {}]);
    expect(records).toEqual([
      { kind: "host", name: "nas", ip: "10.0.0.5" },
      { kind: "host", name: "nas6", ip: "fd00::5" },
      { kind: "cname", name: "files", target: "nas.lan" },
    ]);
    expect(
      recordsOf(
        [
          { kind: "host", name: "uniwrt.lan", value: "10.0.0.5" },
          { kind: "host", name: "bad name", value: "nope" },
          { kind: "cname", name: "x", value: "a..b" },
        ],
        "lan",
      ).errors,
    ).toEqual([
      { name: "uniwrt.lan always points at the controller" },
      { name: "Not a DNS name", value: "Not an IP address" },
      { value: "Not a DNS name" },
    ]);
    expect(
      recordErrorsOfIssues(
        [
          { path: "services/dns_records/1/target", message: "t" },
          { path: "services", message: "o" },
        ],
        2,
      ),
    ).toEqual({ errors: [{}, { value: "t" }], other: ["o"] });
  });
});
