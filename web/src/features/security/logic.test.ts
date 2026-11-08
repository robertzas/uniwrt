import { describe, expect, it } from "vitest";
import type { Network } from "../../api/gen/Network";
import type { PortForward } from "../../api/gen/PortForward";
import { siteConfig } from "../../test/site";
import {
  forwardDraft,
  forwardErrorsOfIssues,
  forwardOf,
  hostError,
  hostPorts,
  parsePorts,
  withForward,
} from "./forwards";
import { cellOf, gridOf, gridsDiffer, policiesOf, seededGrid, WAN, withCell } from "./matrix";

const lan = siteConfig().networks[0] as Network;
const network = (id: string, purpose: Network["purpose"], subnet: number): Network => ({
  ...lan,
  id,
  name: id.toUpperCase(),
  purpose,
  vlan: subnet,
  ipv4: { address: `10.0.${subnet}.1/24`, dhcp: null },
});
const iot = network("iot", "iot", 30);
const guest = network("guest", "guest", 40);
const networks = [lan, iot, guest];

const forward = (extra: Partial<PortForward> = {}): PortForward => ({
  name: "Web",
  enabled: true,
  proto: "tcp",
  ports: { first: 443, last: 443 },
  to: "192.168.1.20",
  to_port: null,
  source: null,
  reflection: true,
  ...extra,
});

describe("the matrix", () => {
  it("is seeded by purpose: the default network reaches all, the others the internet", () => {
    const grid = seededGrid(networks);
    expect(grid.lan).toEqual({ iot: true, guest: true, [WAN]: true });
    expect(grid.iot).toEqual({ lan: false, guest: false, [WAN]: true });
    // IoT → LAN is blocked, but the LAN opens connections to IoT, so its replies pass.
    expect(cellOf(grid, iot, "lan")).toEqual({ allowed: false, repliesOnly: true, changed: false });
    expect(cellOf(grid, iot, "guest").repliesOnly).toBe(false);
    expect(cellOf(grid, iot, WAN)).toEqual({ allowed: true, repliesOnly: false, changed: false });
  });

  it("applies the policies, the last one on a cell winning, and marks the changed cells", () => {
    const grid = gridOf(networks, [
      { from: "lan", to: "guest", action: "established" },
      { from: "guest", to: WAN, action: "block" },
      { from: "iot", to: "lan", action: "block" },
      { from: "iot", to: "lan", action: "allow" },
    ]);
    expect(cellOf(grid, lan, "guest")).toEqual({
      allowed: false,
      repliesOnly: false,
      changed: true,
    });
    expect(cellOf(grid, guest, WAN).changed).toBe(true);
    expect(cellOf(grid, iot, "lan")).toEqual({ allowed: true, repliesOnly: false, changed: true });
  });

  it("writes overrides only where a toggled cell differs from its seed", () => {
    const existing = [
      { from: "lan", to: "guest", action: "established" as const },
      { from: "iot", to: "guest", action: "block" as const },
      { from: WAN, to: "lan", action: "block" as const },
    ];
    const saved = gridOf(networks, existing);
    let grid = withCell(saved, "guest", "lan", true);
    grid = withCell(grid, "lan", WAN, false);
    expect(gridsDiffer(grid, saved)).toBe(true);
    expect(gridsDiffer(saved, gridOf(networks, existing))).toBe(false);
    expect(policiesOf(grid, networks, existing)).toEqual([
      // Untouched cells keep their policies as they were, `established` and redundant ones too.
      { from: "lan", to: "guest", action: "established" },
      { from: "lan", to: WAN, action: "block" },
      { from: "iot", to: "guest", action: "block" },
      { from: "guest", to: "lan", action: "allow" },
      { from: WAN, to: "lan", action: "block" },
    ]);
    // Toggled back to its seed, a cell loses its policy.
    expect(policiesOf(withCell(saved, "lan", "guest", true), networks, existing)).toEqual([
      { from: "iot", to: "guest", action: "block" },
      { from: WAN, to: "lan", action: "block" },
    ]);
    // A network the grid lacks (another save added it) keeps its cells.
    const mgmt = network("mgmt", "mgmt", 99);
    const more = [{ from: "mgmt", to: WAN, action: "block" as const }];
    expect(policiesOf(seededGrid(networks), [...networks, mgmt], more)).toEqual(more);
  });
});

describe("port forwards", () => {
  it("parses ports and ranges", () => {
    expect(parsePorts("443")).toEqual({ first: 443, last: 443 });
    expect(parsePorts(" 8000-8100 ")).toEqual({ first: 8000, last: 8100 });
    expect(parsePorts("8000:8100")).toEqual({ first: 8000, last: 8100 });
    for (const bad of ["", "0", "65536", "90-80", "1-2-3", "http"]) {
      expect(parsePorts(bad)).toBeUndefined();
    }
    expect(hostPorts(forward({ ports: { first: 8000, last: 8010 }, to_port: 80 }))).toEqual({
      first: 80,
      last: 90,
    });
  });

  it("takes a host on a network, not the gateway, the subnet or its broadcast", () => {
    expect(hostError(networks, "192.168.1.20")).toBeUndefined();
    expect(hostError(networks, "10.0.30.7")).toBeUndefined();
    expect(hostError(networks, "192.168.1")).toBe("Not an IPv4 address");
    expect(hostError(networks, "172.16.0.5")).toBe("Not on any of your networks");
    expect(hostError(networks, "192.168.1.1")).toBe("The gateway's own address");
    expect(hostError(networks, "10.0.30.255")).toBe("Not a host's address in 10.0.30.0/24");
  });

  it("checks a draft as the server does, then builds the forward", () => {
    const others = [
      forward(),
      forward({ name: "Off", enabled: false, ports: { first: 80, last: 90 } }),
    ];
    const draft = {
      ...forwardDraft(),
      name: " web",
      ports: "440-450",
      to: "10.0.30.1",
      toPort: "70000",
      source: "1.2.3",
    };
    expect(forwardOf(draft, others, networks).errors).toEqual({
      name: "A name has no spaces around it",
      to: "The gateway's own address",
      to_port: "The host's ports are 1 to 65535",
      source: "An address or a subnet, such as 203.0.113.0/24",
    });
    expect(forwardOf({ ...draft, name: "dmz" }, others, networks).errors.name).toBe(
      "The DMZ host has this name",
    );
    expect(forwardOf({ ...draft, name: "WEB" }, others, networks).errors.name).toBe(
      "Another forward has this name",
    );
    const fixed = { ...draft, name: "Game", to: "10.0.30.20", toPort: "", source: "" };
    // Enabled forwards may not share a port and protocol; an off one doesn't count.
    expect(forwardOf(fixed, others, networks).errors).toEqual({
      ports: "Web already forwards these ports",
    });
    const udp = forwardOf(
      { ...fixed, proto: "udp", toPort: "440", source: "203.0.113.9" },
      others,
      networks,
    );
    expect(udp.forward).toEqual(
      forward({
        name: "Game",
        proto: "udp",
        ports: { first: 440, last: 450 },
        to: "10.0.30.20",
        to_port: null,
        source: "203.0.113.9/32",
      }),
    );
    expect(forwardDraft(udp.forward)).toMatchObject({
      ports: "440-450",
      toPort: "",
      source: "203.0.113.9/32",
    });
  });

  it("maps the server's issues and adds, replaces or deletes forwards by name", () => {
    const issues = [
      { path: "firewall/forwards/1/to", message: "on none" },
      { path: "firewall/forwards/0/name", message: "other row" },
      { path: "firewall/dmz", message: "dmz" },
    ];
    expect(forwardErrorsOfIssues(issues, 1)).toEqual({
      errors: { to: "on none" },
      other: ["other row", "dmz"],
    });
    const config = withForward(siteConfig(), undefined, forward());
    expect(config.firewall.forwards).toEqual([forward()]);
    const added = withForward(config, undefined, forward({ name: "Two" }));
    const renamed = withForward(added, "Web", forward({ name: "One" }));
    expect(renamed.firewall.forwards.map((f) => f.name)).toEqual(["One", "Two"]);
    expect(withForward(renamed, "Two", null).firewall.forwards.map((f) => f.name)).toEqual(["One"]);
    // A forward another save removed comes back as edited.
    expect(withForward(siteConfig(), "Gone", forward()).firewall.forwards).toEqual([forward()]);
  });
});
