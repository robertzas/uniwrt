import { describe, expect, it } from "vitest";
import { applyPlan } from "../test/plan";
import { diffCounts, optionLines, renamedFrom, sectionChange, sectionLabel } from "./uciDiff";

describe("a UCI diff in words", () => {
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
