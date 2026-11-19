import { describe, expect, it } from "vitest";
import { dnsQueryParams } from "../../../lib/api";
import { dnsStats } from "../../../test/blocklists";
import { siteConfig } from "../../../test/site";
import { barsOf, blockedShare, HOUR, listedOf, peakOf, withListed } from "./dnsStats";

const blocking = (allow: string[] = [], deny: string[] = []) => ({
  ...siteConfig().services.blocking,
  enabled: true,
  allow,
  deny,
});

describe("DNS query statistics", () => {
  it("draws one bar per hour over 24 h, empty hours as zeros", () => {
    const now = Date.UTC(2026, 10, 18, 12, 30);
    const since = now - 24 * HOUR;
    const stats = dnsStats({
      since,
      hours: [
        { hour: Date.UTC(2026, 10, 17, 12), queries: 10, blocked: 2 },
        { hour: Date.UTC(2026, 10, 18, 12), queries: 4, blocked: 0 },
      ],
    });
    const bars = barsOf(stats, now);
    expect(bars).toHaveLength(25);
    expect(bars[0]).toEqual({ start: Date.UTC(2026, 10, 17, 12), queries: 10, blocked: 2 });
    expect(bars[1]).toEqual({ start: Date.UTC(2026, 10, 17, 13), queries: 0, blocked: 0 });
    expect(bars[24]).toEqual({ start: Date.UTC(2026, 10, 18, 12), queries: 4, blocked: 0 });
    expect(peakOf(bars)).toBe(10);
    expect(peakOf([])).toBe(1);
  });

  it("sums the hours of 7 and 30 days by local day", () => {
    const now = new Date(2026, 10, 18, 12).getTime();
    const stats = dnsStats({
      range: "7d",
      since: now - 7 * 24 * HOUR,
      hours: [
        { hour: new Date(2026, 10, 11, 13).getTime(), queries: 5, blocked: 1 },
        { hour: new Date(2026, 10, 11, 23).getTime(), queries: 5, blocked: 1 },
        { hour: new Date(2026, 10, 18, 0).getTime(), queries: 3, blocked: 3 },
      ],
    });
    const bars = barsOf(stats, now);
    expect(bars).toHaveLength(8);
    expect(bars[0]).toEqual({
      start: new Date(2026, 10, 11).getTime(),
      queries: 10,
      blocked: 2,
    });
    expect(bars[7]).toEqual({ start: new Date(2026, 10, 18).getTime(), queries: 3, blocked: 3 });
    expect(bars.slice(1, 7).every((bar) => bar.queries === 0)).toBe(true);
  });

  it("gives the blocked share only with queries", () => {
    expect(blockedShare(0, 0)).toBeUndefined();
    expect(blockedShare(8, 1)).toBe(`${(12.5).toLocaleString(undefined)} %`);
  });

  it("finds a domain on the allow or deny list, the most specific entry winning", () => {
    const lists = blocking(["cdn.ads.example"], ["ads.example"]);
    expect(listedOf(lists, "x.cdn.ads.example")).toBe("allowed");
    expect(listedOf(lists, "track.ads.example")).toBe("denied");
    expect(listedOf(lists, "ADS.example")).toBe("denied");
    expect(listedOf(lists, "badads.example")).toBeUndefined();
  });

  it("moves a domain onto a list and off the other", () => {
    expect(withListed(blocking([], ["a.example"]), "A.example", "allow")).toEqual({
      blocking: blocking(["a.example"], []),
    });
    expect(withListed(blocking(["a.example"]), "a.example", "allow")).toEqual({
      blocking: blocking(["a.example"]),
    });
    expect(withListed(blocking(), "_dns.resolver.arpa", "deny")).toEqual({
      error: "_dns.resolver.arpa is not a domain, such as ads.example.com",
    });
    const full = Array.from({ length: 1_000 }, (_, i) => `d${i}.example`);
    expect(withListed(blocking([], full), "more.example", "deny")).toEqual({
      error: "At most 1,000 domains",
    });
  });

  it("asks for a page of queries with the filter's parts set", () => {
    expect(dnsQueryParams({}, null, 200).toString()).toBe("limit=200");
    expect(
      dnsQueryParams(
        { client: "aa:00:00:00:00:01", outcome: "blocked", domain: " ads " },
        "1700:42",
        50,
      ).toString(),
    ).toBe(
      "client=aa%3A00%3A00%3A00%3A00%3A01&outcome=blocked&domain=ads&before=1700%3A42&limit=50",
    );
  });
});
