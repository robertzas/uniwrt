import { describe, expect, it } from "vitest";
import { entityLabel, entityPage, groupEntities, type Names, parseEntity } from "./entities";

const names: Names = {
  node: (id) => ({ 1: "Gateway", 2: "Attic" })[id],
  client: (mac) => (mac === "aa:bb:cc:dd:ee:01" ? "Anna's phone" : undefined),
};

describe("entities", () => {
  it("parse every kind, and nothing else", () => {
    expect(parseEntity("site")).toEqual({ kind: "site" });
    expect(parseEntity("node:2")).toEqual({ kind: "node", node: 2 });
    expect(parseEntity("node:2/dev:br-lan")).toEqual({ kind: "device", node: 2, name: "br-lan" });
    expect(parseEntity("node:2/radio:radio1")).toEqual({ kind: "radio", node: 2, name: "radio1" });
    expect(parseEntity("link:node:1~node:2~mesh")).toEqual({
      kind: "link",
      a: "node:1",
      b: "node:2",
      medium: "mesh",
    });
    expect(parseEntity("client:aa:bb:cc:dd:ee:01")).toEqual({
      kind: "client",
      mac: "aa:bb:cc:dd:ee:01",
    });
    expect(parseEntity("node:x")).toBeUndefined();
    expect(parseEntity("link:node:1")).toBeUndefined();
  });

  it("are named as the user knows them and link to their pages", () => {
    const label = (text: string) => {
      const entity = parseEntity(text);
      return entity && entityLabel(entity, names);
    };
    expect(label("site")).toBe("Whole network");
    expect(label("node:2/radio:radio1")).toBe("Attic · radio1");
    expect(label("node:7")).toBe("Node 7");
    expect(label("link:node:1~node:2~mesh")).toBe("Gateway ↔ Attic · Mesh");
    expect(label("link:node:1~peer:aa:aa:aa:aa:aa:aa~mesh")).toBe(
      "Gateway ↔ aa:aa:aa:aa:aa:aa · Mesh",
    );
    expect(label("client:aa:bb:cc:dd:ee:01")).toBe("Anna's phone");
    expect(label("client:aa:bb:cc:dd:ee:02")).toBe("aa:bb:cc:dd:ee:02");
    expect(entityPage({ kind: "radio", node: 2, name: "radio1" })).toBe("/devices/2");
    expect(entityPage({ kind: "client", mac: "aa" })).toBe("/clients/aa");
    expect(entityPage({ kind: "site" })).toBeUndefined();
  });

  it("group by kind, each node followed by its radios and interfaces, and filter", () => {
    const listing = [
      "client:aa:bb:cc:dd:ee:01",
      "node:2/dev:eth0",
      "node:1",
      "node:2",
      "node:2/radio:radio0",
      "link:node:1~node:2~mesh",
      "site",
      "bogus",
    ].map((entity) => ({ entity, metrics: [] }));
    const groups = groupEntities(listing, names);
    expect(groups.map((g) => g.kind)).toEqual(["site", "node", "link", "client"]);
    expect(groups[1]?.rows.map((r) => r.text)).toEqual([
      "node:2",
      "node:2/radio:radio0",
      "node:2/dev:eth0",
      "node:1",
    ]);
    expect(groups[1]?.rows.map((r) => r.nested)).toEqual([false, true, true, false]);
    const filtered = groupEntities(listing, names, "anna");
    expect(filtered.map((g) => g.rows.map((r) => r.text))).toEqual([["client:aa:bb:cc:dd:ee:01"]]);
    expect(
      groupEntities(listing, names, "radios").flatMap((g) => g.rows.map((r) => r.text)),
    ).toEqual(["node:2/radio:radio0"]);
  });
});
