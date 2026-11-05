import { describe, expect, it } from "vitest";
import { importPreview } from "../../../test/import";
import { byPackage, fateCounts, sortIssues, stateCounts } from "./preview";

describe("an import preview in words", () => {
  it("counts the nodes by state and their sections by fate", () => {
    const preview = importPreview();
    expect(stateCounts(preview)).toEqual({ imported: 2, failed: 0, unread: 1 });
    const gateway = preview.nodes[0];
    if (!gateway) throw new Error("fixture");
    expect(fateCounts(gateway.sections)).toEqual({ mapped: 2, raw: 1, dropped: 0 });
    expect(byPackage(gateway.sections).map((g) => [g.package, g.sections.length])).toEqual([
      ["network", 2],
      ["firewall", 1],
    ]);
  });

  it("keeps errors site-wide and gives each node its warnings, once per message", () => {
    const sorted = sortIssues([
      { path: "networks/lan/ipv4/address", severity: "warning", message: "Factory subnet" },
      { path: "nodes/2/raw/0", severity: "warning", message: "Ignored: wireless" },
      { path: "nodes/2/raw/1", severity: "warning", message: "Ignored: wireless" },
      { path: "nodes/2/raw/2", severity: "warning", message: "Ignored: dhcp" },
      { path: "nodes/12", severity: "warning", message: "Not adopted" },
      { path: "nodes/2/hostname", severity: "error", message: "Not a host name" },
    ]);
    expect(sorted.errors.map((i) => i.path)).toEqual(["nodes/2/hostname"]);
    expect(sorted.warnings.map((i) => i.message)).toEqual(["Factory subnet"]);
    expect(sorted.byNode.get(2)).toEqual([
      { message: "Ignored: wireless", count: 2 },
      { message: "Ignored: dhcp", count: 1 },
    ]);
    expect(sorted.byNode.get(12)).toEqual([{ message: "Not adopted", count: 1 }]);
  });
});
