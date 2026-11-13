import { describe, expect, it } from "vitest";
import { applyPlan } from "../../test/plan";
import { counts, impacts, longestTimer, overwrites, split } from "./plan";

describe("the apply plan in words", () => {
  it("counts, splits and times the nodes", () => {
    const plan = applyPlan();
    expect(counts(plan)).toEqual({ change: 2, unchanged: 1, skipped: 1, refused: 0 });
    const { acted, alone } = split(plan);
    expect(acted.map((n) => n.name)).toEqual(["Attic AP", "Gateway"]);
    expect(alone.map((n) => n.name)).toEqual(["Hall AP", "Shed AP"]);
    expect(longestTimer(plan)).toBe(300);
  });

  it("sums up the impact, disruptive packages first", () => {
    const list = impacts(applyPlan());
    expect(list.map((i) => i.kind)).toEqual([
      "network",
      "wireless",
      "dhcp",
      "service",
      "address",
      "warnings",
    ]);
    expect(list[0]).toEqual({
      kind: "network",
      text: "The network restarts on Gateway: traffic pauses for a few seconds.",
      disruptive: true,
    });
    expect(list[1]?.text).toBe("Wi-Fi restarts on Attic AP: clients reconnect within seconds.");
    expect(list[3]?.text).toBe("dnsmasq stops on Attic AP, and no longer starts at boot.");
    expect(list[4]?.text).toBe("Gateway moves from 192.168.1.1 to 10.0.0.1; uniwrt follows it.");
    expect(list[5]?.text).toBe("1 warning below.");
  });

  it("tells radios that move live from Wi-Fi that restarts", () => {
    const plan = applyPlan();
    const attic = plan.nodes.find((n) => n.name === "Attic AP");
    if (!attic) throw new Error("no Attic AP");
    attic.air = true;
    attic.services = [];
    const list = impacts(plan);
    expect(list.map((i) => i.kind)).not.toContain("wireless");
    expect(list.find((i) => i.kind === "channels")).toEqual({
      kind: "channels",
      text: "Radio channels move on Attic AP, one device at a time: clients follow the switch, or reconnect where a radio can't announce it.",
      disruptive: false,
    });
  });

  it("names a few devices and counts many", () => {
    const plan = applyPlan();
    for (const node of plan.nodes) {
      node.action = "change";
      node.packages = [{ package: "system", sections: [] }];
    }
    expect(impacts(plan)[0]?.text).toBe("System settings change on 4 devices.");
    plan.nodes.splice(3);
    expect(impacts(plan)[0]?.text).toBe("System settings change on Attic AP, Gateway and Hall AP.");
  });

  it("names the drift an apply overwrites: drifted packages that change", () => {
    const plan = applyPlan();
    const gateway = plan.nodes[1];
    const hall = plan.nodes[2];
    if (!gateway || !hall) throw new Error("the plan's nodes");
    gateway.drift = ["network", "firewall"];
    hall.drift = ["system"];
    expect(overwrites(gateway)).toEqual(["network"]);
    expect(overwrites(hall)).toEqual([]);
    const drift = impacts(plan).find((i) => i.kind === "drift");
    expect(drift?.text).toBe("Overwrites a change made by hand on Gateway.");
    expect(impacts(plan).at(-1)?.kind).toBe("warnings");
  });
});
