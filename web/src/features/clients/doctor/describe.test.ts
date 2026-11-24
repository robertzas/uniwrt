import { describe, expect, it } from "vitest";
import type { DoctorProblem } from "../../../api/gen/DoctorProblem";
import { chipText, entryText, evidenceText, fixText } from "./describe";

const nodeName = (id: number) => (id === 2 ? "Living room" : `#${id}`);
const names = { node: nodeName, client: () => "Anna", pattern: () => undefined };

describe("the doctor's words", () => {
  it("states each problem's evidence", () => {
    const cases: [DoctorProblem, string][] = [
      [
        { kind: "sticky", signal_dbm: -80, node: 2, minutes: 25, declined: 1 },
        "-80 dBm for 25 min while other access points carry its network, at Living room, declined 1 suggestion to move",
      ],
      [
        { kind: "auth_failures", failures: 4, ssid: "Home", connected: false },
        "4 failed handshakes, on Home, it hasn't got on since",
      ],
      [
        { kind: "dhcp_failures", failures: 3, reason: "pool_full" },
        "3 DHCP failures, the latest: the pool was full",
      ],
      [{ kind: "no_address", minutes: 7 }, "Online over Wi-Fi for 7 min without an address"],
      [{ kind: "dns_failures", failed: 0, queries: 0 }, "0 of 0 lookups failed (0 %)"],
      [
        { kind: "stuck_on2g", signal_dbm: -55, ssid: null },
        "-55 dBm on 2.4 GHz, though it has used 5 GHz",
      ],
      [
        { kind: "fast_roaming", ssid: "Home", short_sessions: 3, failures: 0 },
        "on Home, it never fast-roamed, 3 sessions ended within a minute",
      ],
      [{ kind: "flapping", disconnects: 8 }, "Disconnected 8 times in the last hour"],
      [
        { kind: "weak_signal", signal_dbm: -83, node: null, band: null },
        "-83 dBm by the median of the last 15 minutes",
      ],
    ];
    for (const [problem, text] of cases) expect(evidenceText(problem, nodeName)).toBe(text);
  });

  it("links each fix to where it is made", () => {
    expect(fixText({ fix: "turn_off_fast_roaming", ssid: "Home" }, nodeName)).toEqual({
      text: "Turn off 802.11r for Home",
      link: { href: "/wifi", label: "Wi-Fi" },
    });
    expect(fixText({ fix: "widen_dhcp_pool" }, nodeName).link?.href).toBe("/networks");
    expect(fixText({ fix: "add_access_point" }, nodeName).link?.href).toBe("/devices");
    expect(fixText({ fix: "reconnect" }, nodeName).reconnect).toBe(true);
    expect(fixText({ fix: "update_client" }, nodeName).link).toBeUndefined();
  });

  it("names the worst issue on the chip", () => {
    expect(chipText([])).toBeUndefined();
    const issue = {
      severity: "critical" as const,
      at: 0,
      fixes: [],
      problem: { kind: "flapping" as const, disconnects: 9 },
    };
    expect(chipText([issue])).toBe("Keeps disconnecting");
    expect(chipText([issue, issue, issue])).toBe("Keeps disconnecting and 2 more");
  });

  it("caps a DNS group's domains and marks DHCP trouble", () => {
    const domains = ["a", "b", "c", "d", "e", "f", "g"].map((d) => `${d}.example`);
    const dns = entryText({ kind: "dns", at: 0, failed: 9, domains }, names);
    expect(dns.detail).toBe("a.example, b.example, c.example, d.example, e.example and 2 more");
    const nak = entryText(
      {
        kind: "dhcp",
        at: 0,
        node: 1,
        step: "nak",
        interface: null,
        ip: null,
        detail: "wrong network",
      },
      names,
    );
    expect(nak).toEqual({
      title: "Address refused",
      detail: "wrong network",
      tone: "warn",
      node: 1,
    });
    const log = entryText(
      { kind: "log", at: 0, node: null, host: "ap9", program: "hostapd", message: "something" },
      names,
    );
    expect(log).toEqual({
      title: "something",
      detail: "hostapd · ap9",
      tone: "neutral",
      node: null,
    });
  });
});
