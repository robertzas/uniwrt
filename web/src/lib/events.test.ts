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
