import { describe, expect, it } from "vitest";
import type { EventRecord } from "../../api/gen/EventRecord";
import { siteView } from "../../test/site";
import {
  failoverDraft,
  failoverOf,
  kbpsOf,
  mbps,
  sqmDraft,
  sqmOf,
  stretches,
  withManaged,
} from "./uplink";

describe("SQM and failover forms", () => {
  it("reads rates in Mbit/s and keeps a custom overhead", () => {
    expect(kbpsOf("450")).toBe(450_000);
    expect(kbpsOf("0,5")).toBe(500);
    expect(kbpsOf("0.01")).toBeUndefined();
    expect(kbpsOf("")).toBeUndefined();
    expect(mbps(95_000)).toBe("95");
    const sqm = { download_kbps: 85_000, upload_kbps: 9_500, overhead: 22, autotune: false };
    const draft = sqmDraft(sqm);
    expect(draft).toMatchObject({ on: true, down: "85", up: "9.5", overhead: "none" });
    expect(sqmOf(draft, sqm).sqm).toEqual(sqm);
    expect(sqmOf({ ...draft, overhead: "docsis" }, sqm).sqm?.overhead).toBe(18);
    expect(sqmOf({ ...draft, on: false }, sqm).sqm).toBeNull();
  });

  it("round-trips a backup line and checks its fields", () => {
    const port = {
      link: { kind: "port" as const, port: "lan4", vlan: 300 },
      proto: {
        kind: "static" as const,
        address: "192.0.2.10/24",
        gateway: "192.0.2.1",
        dns: ["192.0.2.1", "9.9.9.9"],
      },
    };
    expect(failoverOf(failoverDraft(port)).failover).toEqual(port);
    const bad = {
      ...failoverDraft(null),
      on: true,
      port: "",
      vlan: "5000",
      proto: "static" as const,
    };
    expect(failoverOf(bad).errors).toEqual({
      port: "Required",
      vlan: "A VLAN ID is 1 to 4094",
      address: "Required",
      gateway: "Required",
    });
  });

  it("manages SQM's and mwan3's packages on a gateway that manages network", () => {
    const config = siteView(1).config;
    config.nodes = [
      { node: 1, hostname: null, managed: ["network"], radios: [], ports: [], raw: [] },
    ];
    config.wan.sqm = { download_kbps: 1000, upload_kbps: 1000, overhead: null, autotune: false };
    expect(withManaged(config, 1).nodes[0]?.managed).toEqual(["network", "sqm"]);
    expect(withManaged(config, 2).nodes[0]?.managed).toEqual(["network"]);
  });
});

describe("the WAN timeline", () => {
  const event = (at: number, what: EventRecord["what"]): EventRecord => ({
    id: at,
    at,
    last_at: at,
    count: 1,
    level: "notice",
    category: "wan",
    node: 1,
    client: null,
    condition: null,
    what,
  });

  it("pairs outages and failovers, clipping to the range", () => {
    const log = (pattern: string, text: string) =>
      ({
        kind: "log_pattern",
        pattern,
        log: 1,
        host: null,
        program: "mwan3track",
        text,
      }) as EventRecord["what"];
    const events = [
      event(50, { kind: "wan_up", device: "eth1", down_ms: 100 }),
      event(200, { kind: "wan_down", device: "eth1" }),
      event(300, log("mwan3-offline", "Interface wan (eth1) is offline")),
      event(310, log("mwan3-offline", "Interface wanb (usb0) is offline")),
      event(400, log("mwan3-online", "Interface wan (eth1) is online")),
    ];
    expect(stretches(events, 0, 1000)).toEqual([
      { kind: "down", start: 0, end: 50 },
      { kind: "down", start: 200, end: 1000 },
      { kind: "backup", start: 300, end: 400 },
    ]);
  });
});
