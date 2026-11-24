import { describe, expect, it } from "vitest";
import type { EventRecord } from "../../../api/gen/EventRecord";
import type { WanMonitorView } from "../../../api/gen/WanMonitorView";
import { band, faultOf, healthOf, outagesOf, rowsOf } from "./monitor";
import { checkTargets, draftOf, isIp, isProbeUrl, newDraft, slug } from "./targets";

const SETTINGS: WanMonitorView["settings"] = {
  enabled: true,
  targets: [
    { id: "first-hop", name: "ISP gateway", probe: { kind: "icmp", host: null } },
    { id: "cloudflare", name: "Cloudflare", probe: { kind: "icmp", host: "1.1.1.1" } },
    {
      id: "isp-dns",
      name: "ISP DNS",
      probe: { kind: "dns", via: { via: "upstreams" }, name: "openwrt.org" },
    },
    {
      id: "web",
      name: "Web",
      probe: { kind: "http", url: "http://connectivitycheck.gstatic.com/generate_204" },
    },
  ],
};

function view(extra: Partial<WanMonitorView> = {}): WanMonitorView {
  return {
    settings: SETTINGS,
    gateway: 1,
    health: { state: "up" },
    since: 1000,
    last_hop: null,
    error: null,
    round: {
      at: 2000,
      route: true,
      first_hop: "100.64.0.1",
      results: [
        { key: "first-hop", sent: 10, rtt_ms: [2, 1, 3, 4], error: null },
        { key: "cloudflare", sent: 10, rtt_ms: [], error: "timeout" },
        { key: "isp-dns@192.0.2.53", sent: 1, rtt_ms: [20], error: null },
        { key: "gone", sent: 0, rtt_ms: [], error: null },
      ],
    },
    ...extra,
  };
}

function event(id: number, at: number, what: EventRecord["what"]): EventRecord {
  return {
    id,
    at,
    last_at: at,
    count: 1,
    level: "critical",
    category: "wan",
    node: 1,
    client: null,
    condition: null,
    what,
  };
}

describe("the monitor's round", () => {
  it("bands the answers as the model does", () => {
    expect(band([])).toBeUndefined();
    expect(band([3, 1, 2])).toEqual([1, 2, 3]);
    expect(band([4, 1, 3, 2])).toEqual([1, 2.5, 4]);
  });

  it("names each series and what it probes", () => {
    const rows = rowsOf(view());
    expect(rows.map((row) => [row.name, row.detail, row.median, row.loss])).toEqual([
      ["ISP gateway", "100.64.0.1", 2.5, 60],
      ["Cloudflare", "1.1.1.1", null, 100],
      ["ISP DNS · 192.0.2.53", "openwrt.org · 192.0.2.53", 20, 0],
      ["gone", "", null, null],
    ]);
    expect(rowsOf(view({ round: null }))).toEqual([]);
  });

  it("says where an outage lies", () => {
    expect(healthOf(view()).tone).toBe("ok");
    const down = view({ health: { state: "down", fault: "isp" } });
    expect(healthOf(down)).toEqual({ tone: "crit", label: "Internet down" });
    expect(faultOf(down)).toBe("isp");
    expect(faultOf(view())).toBeUndefined();
    expect(healthOf(view({ health: null })).tone).toBe("neutral");
  });

  it("pairs outages with their restores, newest first", () => {
    const events = [
      event(5, 9000, { kind: "wan_outage", fault: "dns", last_hop: null }),
      event(4, 5000, { kind: "wan_restored", fault: "line", down_ms: 2000 }),
      event(3, 3000, { kind: "wan_outage", fault: "line", last_hop: "100.64.0.1" }),
      event(2, 1500, { kind: "wan_restored", fault: "isp", down_ms: 1000 }),
      event(1, 1200, { kind: "wan_down", device: "eth1" }),
    ];
    expect(outagesOf(events)).toEqual([
      { start: 9000, end: null, fault: "dns", lastHop: null },
      { start: 3000, end: 5000, fault: "line", lastHop: "100.64.0.1" },
      { start: 500, end: 1500, fault: "isp", lastHop: null },
    ]);
  });
});

describe("the targets' editor", () => {
  it("keeps ids and gives new targets one from their name", () => {
    const drafts = [
      ...SETTINGS.targets.map(draftOf),
      { ...newDraft(), name: "Cloudflare", host: "1.0.0.1" },
      {
        ...newDraft(),
        name: "Ünïcode NAS!",
        kind: "http" as const,
        url: "https://[2001:db8::1]:8443/x",
      },
    ];
    const checked = checkTargets(true, drafts);
    expect("settings" in checked && checked.settings.targets.map((t) => t.id)).toEqual([
      "first-hop",
      "cloudflare",
      "isp-dns",
      "web",
      "cloudflare-2",
      "unicode-nas",
    ]);
    expect("settings" in checked && checked.settings.targets[4]?.probe).toEqual({
      kind: "icmp",
      host: "1.0.0.1",
    });
  });

  it("checks each target as the server does", () => {
    const checked = checkTargets(true, [
      { ...newDraft(), name: "", host: "not a host" },
      { ...newDraft(), name: "!!!", kind: "dns", lookup: "123", via: "server", server: "0.0.0.0" },
      { ...newDraft(), name: "Web", kind: "http", url: "ftp://example.org" },
      { ...newDraft(), name: "Fine", host: "2001:db8::1" },
    ]);
    expect("errors" in checked && checked.errors).toEqual([
      { name: "1–48 characters", host: "Not an address or a host name" },
      {
        name: "Give it a name with letters or digits",
        lookup: "Not a name to look up",
        server: "Not an IP address",
      },
      { url: "An http:// or https:// URL with a host" },
      {},
    ]);
  });

  it("reads addresses, URLs and names", () => {
    expect(isIp("192.0.2.1")).toBe(true);
    expect(isIp("256.0.0.1")).toBe(false);
    expect(isIp("2001:db8::1")).toBe(true);
    expect(isIp("2001:db8::g")).toBe(false);
    expect(isProbeUrl("http://example.org")).toBe(true);
    expect(isProbeUrl("http://example.org:8080/generate_204?x")).toBe(true);
    expect(isProbeUrl("https://[::1]/")).toBe(true);
    expect(isProbeUrl("http://")).toBe(false);
    expect(isProbeUrl("http://exa mple.org")).toBe(false);
    expect(slug("  ISP -- DNS  ")).toBe("isp-dns");
    expect(slug("a".repeat(30))).toHaveLength(24);
  });
});
