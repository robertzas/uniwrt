import { describe, expect, it } from "vitest";
import type { RawSection } from "../../../api/gen/RawSection";
import type { SiteConfig } from "../../../api/gen/SiteConfig";
import type { UciOwner } from "../../../api/gen/UciOwner";
import type { UciTreePackage } from "../../../api/gen/UciTreePackage";
import type { UciTreeSection } from "../../../api/gen/UciTreeSection";
import { strings } from "../../../lib/strings";
import { deltaOf, draftOf, newOption } from "./draft";
import {
  addSection,
  deleteSection,
  dropRaw,
  editSection,
  type RawEdit,
  type SectionDelta,
  secretPaths,
  withRaw,
} from "./edits";
import { isMaskedOption, matchingSections, searchWords, sectionLabel } from "./uci";

const e = strings.device.uci.errors;
const MASK = "••••••••";

function section(
  type: string,
  name: string | null,
  options: Record<string, string | string[]>,
  extra: Partial<UciTreeSection> = {},
): UciTreeSection {
  return {
    type,
    name,
    options: Object.entries(options).map(([n, value]) => ({
      name: n,
      value: isMaskedOption(n) ? (Array.isArray(value) ? value.map(() => MASK) : MASK) : value,
      secret: isMaskedOption(n),
    })),
    overrides: [],
    key: null,
    ...extra,
  };
}

function pkg(name: string, owner: UciOwner, sections: UciTreeSection[]): UciTreePackage {
  return { name, owner, sections, error: null };
}

function raw(extra: Partial<RawSection>): RawSection {
  return {
    package: "network",
    type: "route",
    name: null,
    options: {},
    merge: false,
    unset: [],
    remove: false,
    matching: {},
    ...extra,
  };
}

const delta = (d: Partial<SectionDelta>): SectionDelta => ({
  set: {},
  secrets: {},
  unset: [],
  ...d,
});

function ok(edit: RawEdit | string): RawEdit {
  if (typeof edit === "string") throw new Error(edit);
  return edit;
}

const routes = pkg("network", "patched", [
  section("interface", "lan", { proto: "static" }),
  section("route", null, { target: "10.9.0.0/16", gateway: "192.168.1.2" }),
  section("route", null, { target: "10.8.0.0/16", gateway: "192.168.1.2" }),
]);

describe("editSection", () => {
  it("merges into a named section by its name", () => {
    const edit = ok(
      editSection([], routes, 0, delta({ set: { ipaddr: "10.0.0.1" }, unset: ["proto"] })),
    );
    expect(edit.raw).toEqual([
      raw({
        type: "interface",
        name: "lan",
        options: { ipaddr: "10.0.0.1" },
        merge: true,
        unset: ["proto"],
      }),
    ]);
  });

  it("finds an anonymous section by the plain options it keeps, never by those it edits", () => {
    const edit = ok(editSection([], routes, 2, delta({ set: { gateway: "192.168.1.3" } })));
    expect(edit.raw[0]).toMatchObject({
      merge: true,
      options: { gateway: "192.168.1.3" },
      matching: { target: "10.8.0.0/16" },
    });
    // Both routes share the gateway: it alone can't tell the second from the first.
    expect(editSection([], routes, 2, delta({ set: { target: "10.7.0.0/16" } }))).toBe(e.notFirst);
  });

  it("uses the type's key when it finds the section", () => {
    const firewall = pkg("firewall", "rendered", [
      section("defaults", null, { input: "REJECT" }, { key: [] }),
      section("zone", null, { name: "lan", input: "ACCEPT" }, { key: ["name"] }),
      section("zone", null, { name: "wan", input: "REJECT" }, { key: ["name"] }),
    ]);
    const zone = ok(editSection([], firewall, 2, delta({ set: { input: "DROP" } })));
    expect(zone.raw[0]).toMatchObject({
      package: "firewall",
      name: null,
      options: { name: "wan", input: "DROP" },
      matching: {},
    });
    const defaults = ok(editSection([], firewall, 0, delta({ set: { input: "DROP" } })));
    expect(defaults.raw[0]).toMatchObject({ options: { input: "DROP" }, matching: {} });
    // Renaming the zone: the key no longer finds it, so its other options do.
    const renamed = ok(editSection([], firewall, 2, delta({ set: { name: "wan2" } })));
    expect(renamed.raw[0]).toMatchObject({
      options: { name: "wan2" },
      matching: { input: "REJECT" },
    });
  });

  it("folds a later edit into the merge that finds the section", () => {
    const merge = raw({
      merge: true,
      options: { metric: "5" },
      matching: { target: "10.8.0.0/16", gateway: "192.168.1.2" },
    });
    const other = raw({ package: "system", type: "system" });
    const read = pkg("network", "patched", [
      routes.sections[0] as UciTreeSection,
      routes.sections[1] as UciTreeSection,
      section(
        "route",
        null,
        { target: "10.8.0.0/16", gateway: "192.168.1.2", metric: "5" },
        {
          overrides: [0],
        },
      ),
    ]);
    const edit = ok(editSection([merge, other], read, 2, delta({ unset: ["gateway"] })));
    expect(edit.raw).toEqual([
      raw({
        merge: true,
        options: { metric: "5" },
        unset: ["gateway"],
        matching: { target: "10.8.0.0/16" },
      }),
      other,
    ]);
  });

  it("edits a whole section in place, removing a patched anonymous one as read first", () => {
    const whole = raw({ options: { target: "10.7.0.0/16", gateway: "10.0.0.2" } });
    const read = pkg("network", "patched", [
      section("route", null, { target: "10.7.0.0/16", gateway: "10.0.0.2" }, { overrides: [0] }),
    ]);
    const edit = ok(editSection([whole], read, 0, delta({ set: { gateway: "10.0.0.3" } })));
    expect(edit.raw).toEqual([
      raw({ remove: true, matching: { target: "10.7.0.0/16", gateway: "10.0.0.2" } }),
      raw({ options: { target: "10.7.0.0/16", gateway: "10.0.0.3" } }),
    ]);
    // Edited again once applied: the earlier removal is replaced, not kept beside a new one.
    const applied = pkg("network", "patched", [
      section("route", null, { target: "10.7.0.0/16", gateway: "10.0.0.3" }, { overrides: [1] }),
    ]);
    const again = ok(editSection(edit.raw, applied, 0, delta({ set: { metric: "2" } })));
    expect(again.raw).toHaveLength(2);
    expect(again.raw[0]?.matching).toEqual({ target: "10.7.0.0/16", gateway: "10.0.0.3" });
    expect(again.raw[1]?.options).toEqual({
      target: "10.7.0.0/16",
      gateway: "10.0.0.3",
      metric: "2",
    });
    // A rendered package is written anew: the whole section alone changes.
    const rendered = { ...read, owner: "rendered" as const };
    const plain = ok(editSection([whole], rendered, 0, delta({ set: { gateway: "10.0.0.3" } })));
    expect(plain.raw).toHaveLength(1);
  });

  it("seals a masked option typed anew in the section that takes the edit", () => {
    const wireless = pkg("wireless", "rendered", [
      section("wifi-iface", "default_radio0", { ssid: "home", key: "x" }),
    ]);
    const edit = ok(editSection([raw({})], wireless, 0, delta({ secrets: { key: "new secret" } })));
    expect(edit.raw[1]).toMatchObject({ name: "default_radio0", merge: true, options: {} });
    expect(edit.secrets).toEqual([[1, "key", "new secret"]]);
    expect(secretPaths(4, edit)).toEqual({ "nodes/4/raw/1/key": "new secret" });
    expect(secretPaths(4, { raw: [], secrets: [] })).toBeUndefined();
  });
});

describe("deleteSection", () => {
  it("drops the overrides and removes the section by what finds it", () => {
    const merge = raw({ type: "interface", name: "lan", merge: true, options: { mtu: "1400" } });
    const read = pkg("network", "patched", [
      section("interface", "lan", { proto: "static", mtu: "1400" }, { overrides: [0] }),
      ...routes.sections.slice(1),
    ]);
    expect(ok(deleteSection([merge], read, 0)).raw).toEqual([
      raw({ type: "interface", name: "lan", remove: true }),
    ]);
    expect(ok(deleteSection([], read, 2)).raw).toEqual([
      raw({ remove: true, matching: { target: "10.8.0.0/16", gateway: "192.168.1.2" } }),
    ]);
  });

  it("only drops an anonymous section a rendered package has from the site", () => {
    const whole = raw({ options: { target: "10.7.0.0/16" } });
    const read = pkg("network", "rendered", [
      section("route", null, { target: "10.7.0.0/16" }, { overrides: [0] }),
    ]);
    expect(ok(deleteSection([whole], read, 0)).raw).toEqual([]);
  });

  it("doesn't find a rendered section by what the dropped merges set", () => {
    const merge = raw({
      merge: true,
      options: { metric: "5" },
      matching: { target: "10.9.0.0/16" },
    });
    const read = pkg("network", "rendered", [
      section("route", null, { target: "10.9.0.0/16", metric: "5" }, { overrides: [0] }),
    ]);
    expect(ok(deleteSection([merge], read, 0)).raw[0]?.matching).toEqual({
      target: "10.9.0.0/16",
    });
  });
});

describe("addSection, dropRaw and withRaw", () => {
  it("adds a whole section with its secrets, and drops by index", () => {
    const added = addSection(
      [raw({})],
      "uhttpd",
      "uhttpd",
      "lab",
      delta({ set: { listen_http: ["0.0.0.0:8080"] }, secrets: { key: "k" } }),
    );
    expect(added.raw[1]).toEqual(
      raw({
        package: "uhttpd",
        type: "uhttpd",
        name: "lab",
        options: { listen_http: ["0.0.0.0:8080"] },
      }),
    );
    expect(added.secrets).toEqual([[1, "key", "k"]]);
    expect(dropRaw(added.raw, [0]).raw).toEqual([added.raw[1]]);
  });

  it("refuses to save over overrides changed meanwhile", () => {
    const config = { nodes: [] } as unknown as SiteConfig;
    const saved = withRaw(config, 3, [], [raw({})]);
    expect(saved.nodes[0]).toMatchObject({ node: 3, raw: [raw({})] });
    expect(() => withRaw(saved, 3, [], [])).toThrow(e.changed);
  });
});

describe("deltaOf", () => {
  const read = section("wifi-iface", null, {
    ssid: "home",
    key: "x",
    r0kh: ["a", "b"],
    hidden: "0",
  });

  it("sets what changed, seals what was typed and keeps a masked option left empty", () => {
    const drafts = draftOf(read).map((d) =>
      d.name === "ssid" ? { ...d, text: "lab" } : d.name === "hidden" ? { ...d, deleted: true } : d,
    );
    expect(deltaOf(read, drafts)).toEqual({
      delta: { set: { ssid: "lab" }, secrets: {}, unset: ["hidden"] },
      errors: {},
    });
    const typed = drafts.map((d) => (d.name === "key" ? { ...d, text: "s3cret" } : d));
    expect(deltaOf(read, typed).delta.secrets).toEqual({ key: "s3cret" });
  });

  it("checks new options: a name, once, one value for a secret, items for a list", () => {
    const drafts = [
      ...draftOf(read),
      { ...newOption(), name: "bad-name", text: "1" },
      { ...newOption(), name: "ssid", text: "x" },
      { ...newOption(), name: "sae_password", text: "" },
      { ...newOption(), name: "psk_extra", list: true, text: "a" },
      { ...newOption(), name: "dns", list: true, text: "" },
      { ...newOption(), name: "dns2", list: true, text: "1.1.1.1\n\n9.9.9.9" },
    ];
    const { delta, errors } = deltaOf(read, drafts);
    expect(errors).toEqual({
      4: e.optionName,
      5: e.optionTwice("ssid"),
      6: e.secretMissing,
      7: e.secretList,
      8: e.emptyList,
    });
    expect(delta.set).toEqual({ dns2: ["1.1.1.1", "9.9.9.9"] });
  });
});

describe("search and labels", () => {
  it("labels anonymous sections as uci does and searches every word", () => {
    expect(sectionLabel(routes, 2)).toBe("@route[1]");
    expect(sectionLabel(routes, 0)).toBe("lan");
    expect(matchingSections(routes, searchWords("10.8 route"))).toEqual([2]);
    expect(matchingSections(routes, searchWords("NETWORK"))).toEqual([0, 1, 2]);
    expect(isMaskedOption("auth_token")).toBe(true);
    expect(isMaskedOption("ssid")).toBe(false);
  });
});
