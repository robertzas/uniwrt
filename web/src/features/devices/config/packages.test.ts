import { describe, expect, it } from "vitest";
import { strings } from "../../../lib/strings";
import { siteConfig } from "../../../test/site";
import { changeOf, managedOf, withManaged } from "./packages";

describe("managed packages", () => {
  it("adds a node's entry, then changes only its set, in the server's order", () => {
    const config = siteConfig();
    expect(managedOf(config, 2)).toEqual([]);
    const chosen = withManaged(config, 2, ["firewall", "network"]);
    expect(config.nodes).toEqual([]);
    expect(chosen.nodes).toEqual([
      { node: 2, hostname: null, managed: ["network", "firewall"], radios: [], ports: [], raw: [] },
    ]);
    const named = {
      ...chosen,
      nodes: [{ ...chosen.nodes[0], hostname: "attic" }],
    } as typeof chosen;
    const again = withManaged(named, 2, ["system"]);
    expect(again.nodes).toEqual([
      { node: 2, hostname: "attic", managed: ["system"], radios: [], ports: [], raw: [] },
    ]);
    expect(managedOf(again, 2)).toEqual(["system"]);
  });

  it("says what a choice changes, in words", () => {
    const change = changeOf(["network", "dhcp"], ["dhcp", "wireless", "system"]);
    expect(change).toEqual({ added: ["system", "wireless"], removed: ["network"] });
    const message = strings.device.packages.message;
    expect(message("gw", change.added, change.removed)).toBe(
      "Manage system and wireless on gw; stop managing network",
    );
    expect(message("gw", [], ["dhcp", "firewall", "network"])).toBe(
      "Stop managing dhcp, firewall and network on gw",
    );
    expect(message("gw", ["network"], [])).toBe("Manage network on gw");
  });
});
