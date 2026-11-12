import { describe, expect, it } from "vitest";
import type { Network } from "../../../api/gen/Network";
import type { NodeFacts } from "../../../api/gen/NodeFacts";
import { facts } from "../../../test/fixtures";
import { siteConfig } from "../../../test/site";
import { carriesOf, portDraft, portsOf, profileOf, untaggedOf, withPorts } from "./ports";

const networks: Network[] = [
  ...siteConfig().networks,
  { ...(siteConfig().networks[0] as Network), id: "iot", name: "IoT", purpose: "iot", vlan: 30 },
  {
    ...(siteConfig().networks[0] as Network),
    id: "guest",
    name: "Guest",
    purpose: "guest",
    vlan: 40,
  },
];

describe("port profiles", () => {
  it("lists a DSA board's LAN ports with their links, then profiles' unknown ports", () => {
    const node = facts(1);
    const device = (name: string, carrier: boolean, speed: number | null) =>
      ({ name, carrier, speed_mbps: speed }) as NodeFacts["devices"][number];
    node.devices = [device("lan1", true, 1000), device("lan2", false, null)];
    expect(
      portsOf(node, [{ port: "lan9", native: null, tagged: { kind: "none" }, backhaul: true }]),
    ).toEqual([
      { port: "lan1", present: true, link: { carrier: true, speed: 1000 } },
      { port: "lan2", present: true, link: { carrier: false, speed: null } },
      { port: "lan9", present: false },
    ]);
  });

  it("lists a swconfig switch's user ports by number", () => {
    const node = facts(1);
    node.board.layout.switch = {
      name: "switch0",
      lan_ports: [1, 2],
      wan_ports: [5],
      lan_cpu: { num: 0, device: "eth0" },
      wan_cpu: null,
    };
    expect(portsOf(node, []).map((row) => row.port)).toEqual(["switch0:1", "switch0:2"]);
    expect(portsOf(null, [])).toEqual([]);
  });

  it("turns rows into profiles: a trunk needs none, the native network is never tagged", () => {
    const untagged = untaggedOf(networks);
    expect(untagged).toBe("lan");
    const trunk = portDraft(undefined, untagged);
    expect(profileOf("lan1", trunk, untagged)).toBeNull();
    expect(carriesOf(trunk, untagged, networks)).toBe("Trunk");
    // A trunk kept out of the wired backhaul needs a profile to say so.
    expect(profileOf("lan1", { ...trunk, backhaul: false }, untagged)).toEqual({
      port: "lan1",
      native: "lan",
      tagged: { kind: "all" },
      backhaul: false,
    });
    const access = { native: "iot", tagged: "none" as const, only: [], backhaul: true };
    expect(profileOf("lan1", access, untagged)).toEqual({
      port: "lan1",
      native: "iot",
      tagged: { kind: "none" },
      backhaul: true,
    });
    expect(carriesOf(access, untagged, networks)).toBe("Access: IoT");
    const some = { native: "iot", tagged: "only" as const, only: ["iot", "guest"], backhaul: true };
    expect(profileOf("lan1", some, untagged)?.tagged).toEqual({
      kind: "only",
      networks: ["guest"],
    });
    expect(carriesOf(some, untagged, networks)).toBe("Custom");
    expect(
      carriesOf({ native: "", tagged: "none", only: [], backhaul: true }, untagged, networks),
    ).toBe("Carries nothing");
    const profile = profileOf("lan1", some, untagged);
    expect(profile && portDraft(profile, untagged)).toEqual({
      native: "iot",
      tagged: "only",
      only: ["guest"],
      backhaul: true,
    });
  });

  it("saves profiles on the node's entry, adding one when it has none", () => {
    const profile = {
      port: "lan1",
      native: "iot",
      tagged: { kind: "none" as const },
      backhaul: true,
    };
    const added = withPorts(siteConfig(), 3, [profile]);
    expect(added.nodes).toEqual([
      { node: 3, hostname: null, managed: [], radios: [], ports: [profile], raw: [] },
    ]);
    expect(withPorts(added, 3, []).nodes[0]?.ports).toEqual([]);
  });
});
