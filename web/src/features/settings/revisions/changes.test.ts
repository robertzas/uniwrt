import { describe, expect, it } from "vitest";
import type { RevisionMeta } from "../../../api/gen/RevisionMeta";
import { counts, entry, isSecret, merge, pathWords, shown, title } from "./changes";

const SECRET_A = `config/${"a".repeat(32)}`;
const SECRET_B = `config/${"b".repeat(32)}`;
const names = (id: number) => (id === 2 ? "Attic AP" : undefined);

const meta = (id: number, extra: Partial<RevisionMeta> = {}): RevisionMeta => ({
  id,
  parent: id > 1 ? id - 1 : null,
  kind: "edit",
  restored_from: null,
  author_id: 1,
  author: "admin",
  message: "",
  created_at: 0,
  schema: 1,
  changes: 1,
  ...extra,
});

describe("revision changes", () => {
  it("knows a vault reference from other strings", () => {
    expect(isSecret(SECRET_A)).toBe(true);
    expect(isSecret("config/abc")).toBe(false);
    expect(isSecret(`config/${"A".repeat(32)}`)).toBe(false);
    expect(isSecret("hunter22")).toBe(false);
    expect(isSecret(7)).toBe(false);
    // A raw option's sealed value (D-073).
    expect(isSecret({ secret: SECRET_A })).toBe(true);
    expect(isSecret({ secret: "hunter22" })).toBe(false);
    expect(isSecret({ secret: SECRET_A, other: 1 })).toBe(false);
    expect(shown({ private_key: { secret: SECRET_A } })).toEqual({
      kind: "fields",
      fields: [{ path: ["private_key"], value: { kind: "secret" } }],
    });
  });

  it("names devices in paths and keeps other segments", () => {
    expect(pathWords(["networks", "iot", "vlan"], names)).toEqual(["networks", "iot", "vlan"]);
    expect(pathWords(["nodes", "2", "hostname"], names)).toEqual(["nodes", "Attic AP", "hostname"]);
    expect(pathWords(["nodes", "9", "hostname"], names)).toEqual(["nodes", "device 9", "hostname"]);
  });

  it("shows scalars, lists and items in words", () => {
    expect(shown(20)).toEqual({ kind: "text", text: "20" });
    expect(shown(true)).toEqual({ kind: "text", text: "on" });
    expect(shown(null)).toBeNull();
    expect(shown(["a", "b"])).toEqual({ kind: "text", text: "a, b" });
    expect(shown([])).toEqual({ kind: "text", text: "none" });
    expect(shown({ id: "iot", vlan: 20, ipv4: { address: "10.0.20.1/24" }, tags: [] })).toEqual({
      kind: "fields",
      fields: [
        { path: ["id"], value: { kind: "text", text: "iot" } },
        { path: ["vlan"], value: { kind: "text", text: "20" } },
        { path: ["ipv4", "address"], value: { kind: "text", text: "10.0.20.1/24" } },
        { path: ["tags"], value: { kind: "text", text: "none" } },
      ],
    });
    // An item's key and name come first, the rest as the server sent them.
    const order = shown({ ipv6: "ula", name: "IoT", vlan: 20, id: "iot" });
    expect(order?.kind === "fields" && order.fields.map((f) => f.path[0])).toEqual([
      "id",
      "name",
      "ipv6",
      "vlan",
    ]);
    // List items by key, else by position.
    expect(shown([{ mac: "aa:bb", ip: "10.0.0.5" }, { x: 1 }])).toEqual({
      kind: "fields",
      fields: [
        { path: ["aa:bb", "mac"], value: { kind: "text", text: "aa:bb" } },
        { path: ["aa:bb", "ip"], value: { kind: "text", text: "10.0.0.5" } },
        { path: ["2", "x"], value: { kind: "text", text: "1" } },
      ],
    });
  });

  it("never shows a secret's reference, alone, in an item or in a list", () => {
    const wlan = shown({ id: "home", passphrase: SECRET_A });
    expect(JSON.stringify(wlan)).not.toContain(SECRET_A);
    expect(wlan).toEqual({
      kind: "fields",
      fields: [
        { path: ["id"], value: { kind: "text", text: "home" } },
        { path: ["passphrase"], value: { kind: "secret" } },
      ],
    });
    expect(shown([SECRET_A])).toEqual({ kind: "text", text: "secret" });
    const changed = entry(
      { path: ["wlans", "home", "passphrase"], op: "changed", before: SECRET_A, after: SECRET_B },
      names,
    );
    expect(changed).toEqual({
      path: ["wlans", "home", "passphrase"],
      op: "changed",
      before: null,
      after: null,
      secretChanged: true,
    });
    const set = entry(
      { path: ["mesh", "key"], op: "changed", before: null, after: SECRET_B },
      names,
    );
    expect(set.before).toEqual({ kind: "text", text: "none" });
    expect(set.after).toEqual({ kind: "secret" });
    expect(set.secretChanged).toBe(false);
  });

  it("leaves out the side an addition or removal lacks", () => {
    const added = entry({ path: ["networks", "iot"], op: "added", before: null, after: {} }, names);
    expect(added.before).toBeNull();
    expect(added.after).toEqual({ kind: "text", text: "none" });
    const removed = entry(
      { path: ["site", "domain"], op: "removed", before: "lan", after: null },
      names,
    );
    expect(removed.before).toEqual({ kind: "text", text: "lan" });
    expect(removed.after).toBeNull();
  });

  it("counts by kind and titles revisions without a message", () => {
    expect(
      counts([
        { path: ["a"], op: "added", before: null, after: 1 },
        { path: ["b"], op: "changed", before: 1, after: 2 },
        { path: ["c"], op: "changed", before: 1, after: 2 },
      ]),
    ).toEqual({ added: 1, removed: 0, changed: 2 });
    expect(title(meta(1, { kind: "defaults", author: null }))).toBe("The defaults");
    expect(title(meta(4, { kind: "restore", restored_from: 2 }))).toBe("Restored revision 2");
    expect(title(meta(5))).toBe("No message");
    expect(title(meta(6, { message: "Guest VLAN" }))).toBe("Guest VLAN");
  });

  it("merges pages newest first without repeats", () => {
    const merged = merge(
      [meta(3), meta(2), meta(1)],
      [meta(5), meta(4), meta(3, { message: "x" })],
    );
    expect(merged.map((r) => r.id)).toEqual([5, 4, 3, 2, 1]);
    expect(merged[2]?.message).toBe("x");
    expect(merge([meta(5), meta(4)], [meta(3), meta(2)]).map((r) => r.id)).toEqual([5, 4, 3, 2]);
  });
});
