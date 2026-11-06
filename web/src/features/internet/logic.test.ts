import { describe, expect, it } from "vitest";
import type { Job } from "../../api/gen/Job";
import type { WanConfig } from "../../api/gen/WanConfig";
import { DEFAULT_WAN, LUMEN, PPPOE } from "../../test/presets";
import { check, comboError, errorsOfIssues } from "./check";
import { draftOf, fallbackOf, fieldsOf, promptedFields, wanOf, withPreset } from "./draft";
import { countriesOf, ipv6Result, offered, rowsOf } from "./summary";

describe("the WAN form", () => {
  it("round-trips the site's WAN", () => {
    const wans: WanConfig[] = [
      DEFAULT_WAN,
      {
        proto: {
          kind: "dhcp",
          vendor_class: "uniwrt",
          client_id: "0102",
          send_options: ["77:4d4f44454d", "90:00ff"],
          hostname: "router",
        },
        vlan: 201,
        pcp: 1,
        mtu: 1500,
        mac: "02:00:00:00:00:01",
        ipv6: { mode: "6rd", prefix: "2602::/24", border_relay: "205.171.2.64", ipv4_mask_len: 0 },
        preset: "us-lumen-ipoe",
        sqm: { download_kbps: 450000, upload_kbps: 95000, overhead: 44, autotune: true },
        failover: { link: { kind: "usb", device: "usb0" }, proto: { kind: "dhcp" } },
      },
      {
        ...DEFAULT_WAN,
        proto: {
          kind: "static",
          address: "203.0.113.10/29",
          gateway: "203.0.113.9",
          dns: ["9.9.9.9"],
        },
        ipv6: { mode: "off" },
      },
      { ...DEFAULT_WAN, proto: { kind: "ds_lite", aftr: "aftr.example.net" } },
      { ...DEFAULT_WAN, proto: { kind: "none" }, ipv6: { mode: "dhcpv6", reqprefix: 56 } },
    ];
    for (const wan of wans) {
      const draft = draftOf(wan);
      expect(check(draft, false)).toEqual({});
      expect(wanOf(draft, wan)).toEqual(wan);
    }
  });

  it("keeps the stored PPPoE password unless a new one is typed", () => {
    const stored: WanConfig = {
      ...DEFAULT_WAN,
      proto: { kind: "pppoe", username: "u", password: "config/aa", service: null },
    };
    const draft = draftOf(stored);
    expect(check(draft, true)).toEqual({});
    expect(check(draft, false).password).toBeDefined();
    expect(wanOf(draft, stored).proto).toMatchObject({ password: "config/aa" });
    expect(wanOf({ ...draft, password: "new" }, stored).proto).toMatchObject({ password: null });
  });

  it("fills a preset, keeping what the user typed for its prompts", () => {
    const typed = { ...draftOf(DEFAULT_WAN), username: "me", password: "pw", vlan: "7" };
    const lumen = withPreset(typed, LUMEN);
    expect(lumen).toMatchObject({ preset: LUMEN.id, proto: "dhcp", vlan: "201", ipv6: "dhcpv6" });
    expect(fallbackOf(LUMEN)).toEqual({
      mode: "6rd",
      prefix: "2602::/24",
      border_relay: "205.171.2.64",
      ipv4_mask_len: 0,
    });
    const pppoe = withPreset(lumen, PPPOE);
    expect(pppoe).toMatchObject({ proto: "pppoe", mtu: "1492", username: "me", vlan: "" });
    expect(fallbackOf(PPPOE)).toBeUndefined();
    expect(promptedFields(PPPOE)).toEqual(["username", "password"]);
    expect(fieldsOf(pppoe)).toEqual([
      "username",
      "password",
      "service",
      "vlan",
      "pcp",
      "mtu",
      "mac",
      "reqprefix",
    ]);
  });

  it("checks what the server checks", () => {
    const base = draftOf(DEFAULT_WAN);
    expect(check({ ...base, vlan: "4095" }, false).vlan).toBeDefined();
    expect(check({ ...base, pcp: "3" }, false).pcp).toBe("A priority needs the ISP's VLAN");
    expect(check({ ...base, mtu: "1200" }, false).mtu).toBe("The MTU is 1280 to 9000");
    expect(check({ ...base, clientId: "abc" }, false).clientId).toBeDefined();
    expect(check({ ...base, sendOptions: "0:x" }, false).sendOptions).toBeDefined();
    const fixed = {
      ...base,
      proto: "static" as const,
      address: "203.0.113.8/29",
      gateway: "198.51.100.1",
      dns: "x",
    };
    expect(check(fixed, false)).toMatchObject({
      address: expect.any(String),
      gateway: expect.any(String),
      dns: expect.any(String),
    });
    const sixrd = {
      ...base,
      ipv6: "6rd" as const,
      sixrdPrefix: "2602::/48",
      borderRelay: "205.171.2.64",
    };
    expect(check(sixrd, false).sixrdPrefix).toContain("64 bits");
    expect(comboError({ ...base, proto: "ds_lite", ipv6: "off" })).toContain("DS-Lite");
    expect(comboError(base)).toBeUndefined();
  });

  it("maps the server's issues to fields", () => {
    const { errors, other } = errorsOfIssues([
      { path: "wan/pcp", message: "A" },
      { path: "wan/proto/send_options/1", message: "B" },
      { path: "wan/ipv6", message: "C" },
    ]);
    expect(errors).toEqual({ pcp: "A", sendOptions: "B" });
    expect(other).toEqual(["C"]);
  });
});

describe("the connection in words", () => {
  it("names the preset and the settings", () => {
    const rows = rowsOf({ ...DEFAULT_WAN, vlan: 201, preset: LUMEN.id }, [LUMEN]);
    expect(rows).toEqual([
      ["ISP", "Lumen / Quantum Fiber / CenturyLink Fiber · IPoE"],
      ["IPv4", "DHCP"],
      ["VLAN", "201"],
      ["IPv6", "DHCPv6 with prefix delegation"],
      ["MAC address", "The gateway's own"],
      ["MTU", "Automatic"],
    ]);
    expect(rowsOf(DEFAULT_WAN, [])[0]).toEqual(["ISP", "Custom settings"]);
  });

  it("offers a country's ISPs and the generic presets", () => {
    expect(countriesOf([LUMEN, PPPOE])).toEqual([{ code: "US", name: "United States" }]);
    expect(offered([LUMEN, PPPOE], "US", "")).toEqual({ isps: [LUMEN], generic: [PPPOE] });
    expect(offered([LUMEN, PPPOE], null, "")).toEqual({ isps: [], generic: [PPPOE] });
    expect(offered([LUMEN, PPPOE], "US", "quantum")).toEqual({ isps: [LUMEN], generic: [] });
  });

  it("reads IPv6's outcome from the test's steps", () => {
    const job = (ipv6: string, sixrd: string) =>
      ({
        steps: [
          { name: "ipv6", state: ipv6 },
          { name: "six_rd", state: sixrd },
        ],
      }) as unknown as Job;
    expect(ipv6Result(job("done", "skipped"))).toBe("ok");
    expect(ipv6Result(job("failed", "done"))).toBe("sixrd");
    expect(ipv6Result(job("failed", "failed"))).toBe("failed");
    expect(ipv6Result(job("skipped", "skipped"))).toBe("off");
  });
});
