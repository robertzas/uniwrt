import { describe, expect, it } from "vitest";
import { applyPlan } from "../../test/plan";
import {
  counts,
  diffCounts,
  impacts,
  longestTimer,
  optionLines,
  renamedFrom,
  sectionChange,
  sectionLabel,
  split,
} from "./plan";

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

  it("describes sections as uci addresses them", () => {
    const [wireless] = applyPlan().nodes[0]?.packages ?? [];
    const section = wireless?.sections[0];
    if (!section || !wireless) throw new Error("fixture");
    expect(sectionChange(section)).toBe("changed");
    expect(renamedFrom(section)).toBe("'default_radio0'");
    expect(sectionLabel({ type: "device", index: 1 })).toBe("@device[1]");
    expect(sectionChange({ before: null, after: section.after, moved: false, options: [] })).toBe(
      "added",
    );
    expect(sectionChange({ ...section, options: [], moved: true })).toBe("moved");
    expect(diffCounts(wireless)).toEqual({ added: 0, removed: 0, changed: 1 });
  });

  it("writes an option's change as lines, lists item by item", () => {
    expect(optionLines({ name: "ssid", before: "OpenWrt", after: "Home" })).toEqual([
      { op: "~", keyword: "option", name: "ssid", value: "Home", before: "OpenWrt" },
    ]);
    expect(optionLines({ name: "ports", before: null, after: ["lan1", "lan2"] })).toEqual([
      { op: "+", keyword: "list", name: "ports", value: "lan1" },
      { op: "+", keyword: "list", name: "ports", value: "lan2" },
    ]);
    expect(
      optionLines({ name: "ports", before: ["lan1", "lan2"], after: ["lan2", "lan3"] }),
    ).toEqual([
      { op: "-", keyword: "list", name: "ports", value: "lan1" },
      { op: "+", keyword: "list", name: "ports", value: "lan3" },
    ]);
    expect(optionLines({ name: "server", before: ["a", "b"], after: ["b", "a"] })).toEqual([
      { op: "~", keyword: "list", name: "server", value: "b a", before: "a b" },
    ]);
    expect(optionLines({ name: "x", before: "1", after: ["1", "2"] }).map((l) => l.op)).toEqual([
      "-",
      "+",
      "+",
    ]);
    expect(optionLines({ name: "gone", before: "1", after: null })).toEqual([
      { op: "-", keyword: "option", name: "gone", value: "1" },
    ]);
  });
});
