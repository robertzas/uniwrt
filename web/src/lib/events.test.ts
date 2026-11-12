import { describe, expect, it } from "vitest";
import type { EventRecord } from "../api/gen/EventRecord";
import { admits, describeEvent, logsAround, type Names } from "./events";

const names: Names = {
  node: (id) => ({ 1: "Gateway", 2: "Attic AP" })[id] ?? `#${id}`,
  client: (mac) => (mac === "f0:18:98:00:00:01" ? "Pixel" : mac),
  pattern: (id) => (id === "dfs-radar" ? "Radar was detected on a DFS channel" : undefined),
};

function event(what: EventRecord["what"], extra: Partial<EventRecord> = {}): EventRecord {
  return {
    id: 1,
    at: 1_000_000,
    last_at: 1_000_000,
    count: 1,
    level: "warning",
    category: "system",
    node: 2,
    client: null,
    condition: null,
    what,
    ...extra,
  };
}

describe("events in words", () => {
  it("names nodes, peers, clients and durations", () => {
    expect(describeEvent(event({ kind: "node_online", down_ms: 125_000 }), names)).toEqual({
      title: "Attic AP is back online",
      detail: "Down for 2m 5s",
    });
    expect(
      describeEvent(
        event({
          kind: "node_offline",
          reasons: [{ kind: "unreachable", error: "timeout", retry_at: 0 }],
        }),
        names,
      ),
    ).toEqual({ title: "Attic AP went offline", detail: "Unreachable: timeout" });
    expect(
      describeEvent(event({ kind: "mesh_link_weak", link: "l", peer: 1, quality: 32 }), names)
        .title,
    ).toBe("Mesh link Gateway ↔ Attic AP is weak");
    const roam = event(
      {
        kind: "client_roamed",
        from: { node: 1, iface: "phy1-ap0" },
        to: { node: 2, iface: "phy1-ap0" },
        fast: true,
      },
      { client: "f0:18:98:00:00:01" },
    );
    expect(describeEvent(roam, names)).toEqual({
      title: "Pixel roamed from Gateway to Attic AP",
      detail: "Fast transition (802.11r)",
    });
    const switched = event(
      {
        kind: "client_roamed",
        from: { node: 2, iface: "phy0-ap0" },
        to: { node: 2, iface: "phy1-ap0" },
        fast: false,
      },
      { client: "f0:18:98:00:00:01" },
    );
    expect(describeEvent(switched, names)).toEqual({
      title: "Pixel switched access points on Attic AP",
      detail: "phy0-ap0 → phy1-ap0",
    });
    const steered = (what: Partial<Extract<EventRecord["what"], { kind: "client_steered" }>>) =>
      describeEvent(
        event(
          {
            kind: "client_steered",
            action: "assoc_denied",
            iface: "phy0-ap0",
            reason: null,
            signal_dbm: null,
            min_signal_dbm: null,
            to: null,
            to_signal_dbm: null,
            status: null,
            ...what,
          },
          { node: 1, client: "f0:18:98:00:00:01" },
        ),
        names,
      );
    expect(
      steered({
        reason: "better_candidate",
        signal_dbm: -71,
        to: { node: 2, iface: "phy1-ap0" },
        to_signal_dbm: -52,
      }),
    ).toEqual({
      title: "Gateway turned Pixel away",
      detail: "Signal -71 dBm · Better served by Attic AP (-52 dBm) · phy0-ap0",
    });
    expect(steered({ action: "transition_accepted", to: { node: 2, iface: "phy1-ap0" } })).toEqual({
      title: "Pixel agreed to move from Gateway to Attic AP",
      detail: "phy0-ap0",
    });
    expect(steered({ action: "transition_rejected", status: 7 }).detail).toBe(
      "Declined: no suitable access point · phy0-ap0",
    );
    expect(
      describeEvent(
        event(
          {
            kind: "client_steered",
            action: "signal_kick",
            iface: "phy0-ap0",
            reason: null,
            signal_dbm: -91,
            min_signal_dbm: -85,
            to: null,
            to_signal_dbm: null,
            status: null,
          },
          { node: 1, client: null },
        ),
        names,
      ),
    ).toEqual({
      title: "Gateway disconnected a client (weak signal)",
      detail: "Signal -91 dBm, under -85 dBm · phy0-ap0",
    });
    const joined = event(
      { kind: "client_joined", medium: "wifi", place: "phy1-ap0", ssid: "home", band: "5g" },
      { client: "f0:18:98:00:00:01" },
    );
    expect(describeEvent(joined, names).detail).toBe("home · 5 GHz · phy1-ap0");
    const unplaced = event(
      { kind: "new_client", vendor: "Roku", hostname: null },
      { node: null, client: "f0:18:98:00:00:01" },
    );
    expect(describeEvent(unplaced, names)).toEqual({ title: "New device Pixel", detail: "Roku" });
    expect(describeEvent(event({ kind: "node_rebooted", up_s: null }), names)).toEqual({
      title: "Attic AP rebooted",
    });
  });

  it("explains log patterns, and falls back to their id", () => {
    const radar = event({
      kind: "log_pattern",
      pattern: "dfs-radar",
      log: 9,
      host: null,
      program: "hostapd",
      text: "DFS-RADAR-DETECTED freq=5500",
    });
    expect(describeEvent(radar, names)).toEqual({
      title: "Attic AP: Radar was detected on a DFS channel",
      detail: "hostapd: DFS-RADAR-DETECTED freq=5500",
    });
    const other = event(
      {
        kind: "log_pattern",
        pattern: "ppp-down",
        log: 3,
        host: "switch",
        program: null,
        text: "x",
      },
      { node: null },
    );
    expect(describeEvent(other, names).title).toBe("switch: Log: ppp-down");
    expect(
      describeEvent(event({ kind: "controller_started", version: "1.2.3" }, { node: null }), names)
        .title,
    ).toBe("Controller started (1.2.3)");
  });

  it("tells radios moving and their channel congested", () => {
    const moved = {
      kind: "channel_changed",
      radio: "radio1",
      band: "5g",
      from: 52,
      to: 36,
    } as const;
    expect(describeEvent(event({ ...moved, width: 80, radar: false }), names)).toEqual({
      title: "Attic AP radio1 moved from channel 52 to 36",
      detail: "5 GHz · 80 MHz",
    });
    expect(describeEvent(event({ ...moved, width: null, radar: true }), names).detail).toBe(
      "Radar on its DFS channel made it move",
    );
    expect(
      describeEvent(event({ kind: "channel_congested", radio: "radio1", busy_percent: 86 }), names),
    ).toEqual({ title: "Attic AP radio1: channel congested", detail: "86% busy" });
    expect(
      describeEvent(
        event({
          kind: "channel_clear",
          radio: "radio1",
          busy_percent: 40,
          congested_ms: 1_200_000,
        }),
        names,
      ).detail,
    ).toBe("40% busy, after 20m congested");
  });

  it("tells WLANs turned off and on by their schedules, and when a device won't", () => {
    const turned = { kind: "wlan_scheduled", wlan: "guest", ssid: "Guests" } as const;
    expect(describeEvent(event({ ...turned, on: false }, { node: null }), names).title).toBe(
      "Guests turned off by its schedule",
    );
    expect(describeEvent(event({ ...turned, on: true }, { node: null }), names).title).toBe(
      "Guests turned on by its schedule",
    );
    const failed = { kind: "wlan_schedule_failed", phy: "phy1", error: "no hostapd" } as const;
    expect(describeEvent(event(failed), names)).toEqual({
      title: "Attic AP: couldn't follow the WLAN schedules on phy1",
      detail: "no hostapd",
    });
    expect(describeEvent(event({ kind: "wlan_schedule_ok", phy: "phy1" }), names).title).toBe(
      "Attic AP: follows the WLAN schedules on phy1 again",
    );
  });

  it("tells kicks, blocks and a device that won't keep blocked clients off", () => {
    const pixel = { client: "f0:18:98:00:00:01", node: null };
    expect(
      describeEvent(event({ kind: "client_kicked", ban_secs: 600, by: "admin" }, pixel), names),
    ).toEqual({ title: "Pixel was kicked off the Wi-Fi", detail: "By admin · Kept off for 10m" });
    expect(
      describeEvent(event({ kind: "client_kicked", ban_secs: 0, by: null }, pixel), names),
    ).toEqual({ title: "Pixel was kicked off the Wi-Fi" });
    expect(describeEvent(event({ kind: "client_blocked", by: "admin" }, pixel), names)).toEqual({
      title: "Pixel was blocked",
      detail: "By admin",
    });
    expect(describeEvent(event({ kind: "client_unblocked", by: null }, pixel), names).title).toBe(
      "Pixel was unblocked",
    );
    expect(describeEvent(event({ kind: "client_bans_failed", error: "refused" }), names)).toEqual({
      title: "Attic AP: couldn't keep blocked clients off",
      detail: "refused",
    });
    expect(describeEvent(event({ kind: "client_bans_ok" }), names).title).toBe(
      "Attic AP: keeps blocked clients off again",
    );
    expect(
      describeEvent(event({ kind: "neighbor_reports_failed", error: "refused" }), names),
    ).toEqual({
      title: "Attic AP: couldn't take its roaming neighbors",
      detail: "refused",
    });
    expect(describeEvent(event({ kind: "neighbor_reports_ok" }), names).title).toBe(
      "Attic AP: takes its roaming neighbors again",
    );
  });

  it("links to the logs around it, on its device", () => {
    const url = new URL(
      logsAround(event({ kind: "node_adopted" }, { last_at: 1_060_000 })),
      "http://x",
    );
    expect(url.pathname).toBe("/insights/logs");
    expect(url.searchParams.get("nodes")).toBe("2");
    expect(Number(url.searchParams.get("from"))).toBe(1_000_000 - 300_000);
    expect(Number(url.searchParams.get("to"))).toBe(1_060_000 + 300_000);
  });

  it("filters as the store does", () => {
    const e = event({ kind: "wan_down", device: "eth1" }, { level: "critical", category: "wan" });
    const none = { categories: [], min: null, nodes: [], clients: [], from: null, to: null };
    expect(admits(none, e)).toBe(true);
    expect(admits({ ...none, min: "warning" }, e)).toBe(true);
    expect(admits({ ...none, categories: ["mesh"] }, e)).toBe(false);
    expect(admits({ ...none, nodes: [1] }, e)).toBe(false);
    expect(admits({ ...none, clients: ["f0:18:98:00:00:01"] }, e)).toBe(false);
    expect(admits({ ...none, from: 1_000_001 }, e)).toBe(false);
    expect(admits({ ...none, to: 1_000_000 }, e)).toBe(false);
  });
});
