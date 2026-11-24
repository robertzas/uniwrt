import { describe, expect, it } from "vitest";
import type { Ddns } from "../../../api/gen/Ddns";
import { siteConfig } from "../../../test/site";
import { ddnsDraft, ddnsErrorsOfIssues, ddnsOf, ddnsStatus, newId, withDdns } from "./ddns";
import {
  dohHost,
  ISP_UPSTREAM,
  modeOf,
  presetResolver,
  resolverDraft,
  resolverName,
  upstreamDraft,
  upstreamErrorsOfIssues,
  upstreamOf,
} from "./upstream";

describe("upstream DNS", () => {
  it("reads a DoH URL's host as the model does", () => {
    expect(dohHost("https://dns.quad9.net/dns-query")).toBe("dns.quad9.net");
    expect(dohHost("https://1.1.1.1/dns-query")).toBe("1.1.1.1");
    expect(dohHost("https://[2606:4700::1111]:443/dns-query")).toBe("2606:4700::1111");
    expect(dohHost("https://doh.example:8443")).toBe("doh.example");
    for (const bad of [
      "http://dns.google/dns-query",
      "https://",
      "https://localhost/dns-query",
      "https://dns.google:0/x",
      "https://dns.google/a b",
      "https://dns.google/a'b",
      "https://[nope]/x",
    ]) {
      expect(dohHost(bad), bad).toBeUndefined();
    }
  });

  it("recognizes presets, NextDNS with its profile, and other URLs", () => {
    expect(resolverDraft({ url: "https://dns.nextdns.io/abc123", bootstrap: [] })).toMatchObject({
      preset: "nextdns",
      profile: "abc123",
    });
    expect(
      resolverDraft({ url: "https://dns.google/dns-query", bootstrap: ["8.8.8.8", "8.8.4.4"] }),
    ).toMatchObject({ preset: "google", bootstrap: "8.8.8.8, 8.8.4.4" });
    expect(resolverName("https://doh.example/dns-query")).toBe("doh.example");
    expect(resolverName("https://cloudflare-dns.com/dns-query")).toBe("Cloudflare");
    // A fresh draft starts DoH from one resolver, so a switch saves as it stands.
    expect(upstreamDraft(ISP_UPSTREAM).resolvers.map((r) => r.preset)).toEqual(["quad9"]);
  });

  it("reads the modes the card edits, and leaves layers and DoT to the API (until M9.0b2)", () => {
    const doh = { protocol: "doh" as const, url: "https://1.1.1.1/dns-query", bootstrap: [] };
    const dot = {
      protocol: "dot" as const,
      name: "dns.quad9.net",
      addresses: ["9.9.9.9"],
      port: 853,
      spki: [],
    };
    expect(modeOf(ISP_UPSTREAM)).toBe("isp");
    expect(modeOf({ ...ISP_UPSTREAM, isp: false, servers: ["9.9.9.9"] })).toBe("custom");
    expect(modeOf({ ...ISP_UPSTREAM, isp: false, strict: true, encrypted: [doh] })).toBe("doh");
    expect(modeOf({ ...ISP_UPSTREAM, servers: ["9.9.9.9"] })).toBeUndefined();
    expect(modeOf({ ...ISP_UPSTREAM, encrypted: [doh] })).toBeUndefined();
    expect(modeOf({ ...ISP_UPSTREAM, isp: false, strict: true, encrypted: [dot] })).toBeUndefined();
  });

  it("checks custom servers", () => {
    expect(
      upstreamOf({ mode: "custom", servers: [" 9.9.9.9", "2620:fe::fe"], resolvers: [] }),
    ).toMatchObject({
      upstream: { encrypted: [], servers: ["9.9.9.9", "2620:fe::fe"], isp: false, strict: false },
    });
    const bad = upstreamOf({
      mode: "custom",
      servers: ["9.9.9.9", "nope", "9.9.9.9"],
      resolvers: [],
    });
    expect(bad.upstream).toBeUndefined();
    expect(bad.errors.servers).toEqual([undefined, "Not an IP address", "Named twice"]);
    expect(upstreamOf({ mode: "custom", servers: [], resolvers: [] }).errors.list).toBe(
      "Name 1 to 4 DNS servers",
    );
  });

  it("checks DoH resolvers and their bootstrap servers", () => {
    const nextdns = { ...presetResolver("nextdns"), profile: "abc123" };
    const ok = upstreamOf({
      mode: "doh",
      servers: [],
      resolvers: [presetResolver("quad9"), nextdns],
    });
    expect(ok.upstream).toEqual({
      encrypted: [
        {
          protocol: "doh",
          url: "https://dns.quad9.net/dns-query",
          bootstrap: ["9.9.9.9", "149.112.112.112"],
        },
        {
          protocol: "doh",
          url: "https://dns.nextdns.io/abc123",
          bootstrap: ["45.90.28.0", "45.90.30.0"],
        },
      ],
      servers: [],
      isp: false,
      strict: true,
    });
    const bad = upstreamOf({
      mode: "doh",
      servers: [],
      resolvers: [
        presetResolver("nextdns"),
        { preset: "custom", url: "https://doh.example/q", profile: "", bootstrap: "1.1.1.1" },
        { preset: "custom", url: "https://1.1.1.1/dns-query", profile: "", bootstrap: "" },
        { preset: "custom", url: "http://x.example", profile: "", bootstrap: "1.1.1.1 x" },
      ],
    });
    expect(bad.errors.resolvers).toEqual([
      { url: "Enter your NextDNS profile ID: letters and digits" },
      { bootstrap: "Name 2 to 4 IP addresses" },
      {},
      {
        url: "An https:// URL, such as https://dns.quad9.net/dns-query",
        bootstrap: "x is not an IP address",
      },
    ]);
  });

  it("maps the server's issues onto rows", () => {
    const { errors, other } = upstreamErrorsOfIssues([
      { path: "services/upstream/encrypted", message: "list" },
      { path: "services/upstream/encrypted/1/bootstrap/0", message: "loopback" },
      { path: "services/upstream/servers/2", message: "twice" },
      { path: "wan/proto", message: "elsewhere" },
    ]);
    expect(errors.list).toBe("list");
    expect(errors.resolvers[1]).toEqual({ bootstrap: "loopback" });
    expect(errors.servers[2]).toBe("twice");
    expect(other).toEqual(["elsewhere"]);
  });
});

const CF: Ddns = {
  id: "home",
  enabled: true,
  hostname: "home.example.com",
  provider: { kind: "cloudflare", zone: "example.com" },
  ipv6: false,
  source: "wan",
  secret: "config/00000000000000000000000000000001",
};

describe("DDNS", () => {
  it("derives a free id from the hostname", () => {
    expect(newId("Home.example.com", new Set())).toBe("home");
    expect(newId("home.example.com", new Set(["home"]))).toBe("home_2");
    expect(newId("1st-floor.example.com", new Set())).toBe("d1st_floor");
    expect(newId("averyveryverylongname.example.com", new Set(["averyveryver"]))).toBe(
      "averyveryv_2",
    );
  });

  it("builds a new entry with its secret by path", () => {
    const draft = {
      ...ddnsDraft(undefined),
      hostname: "Home.Example.com",
      zone: "example.com",
      secret: "tok",
    };
    const { ddns, secrets } = ddnsOf(draft, undefined, []);
    expect(ddns).toEqual({ ...CF, secret: null });
    expect(secrets).toEqual({ "services/ddns/home/secret": "tok" });
  });

  it("keeps a stored token only for its provider", () => {
    const same = ddnsOf(ddnsDraft(CF), CF, []);
    expect(same.ddns?.secret).toBe(CF.secret);
    expect(same.secrets).toBeUndefined();
    const moved = ddnsOf({ ...ddnsDraft(CF), provider: "desec" }, CF, []);
    expect(moved.errors.secret).toBe("Another provider needs its own token");
    // A custom URL without [PASSWORD] needs none, and drops the stored one.
    const custom = ddnsOf(
      { ...ddnsDraft(CF), provider: "custom", url: "https://upd.example/?h=[DOMAIN]&ip=[IP]" },
      CF,
      [],
    );
    expect(custom.ddns).toMatchObject({
      provider: { kind: "custom", url: "https://upd.example/?h=[DOMAIN]&ip=[IP]", username: null },
      secret: null,
    });
  });

  it("checks names, zones and accounts", () => {
    const draft = ddnsDraft(undefined);
    expect(ddnsOf({ ...draft, hostname: "home", secret: "x" }, undefined, []).errors.hostname).toBe(
      "A full DNS name, such as home.example.com",
    );
    expect(
      ddnsOf({ ...draft, hostname: "a.other.org", zone: "example.com", secret: "x" }, undefined, [])
        .errors.zone,
    ).toBe("a.other.org is not in the zone example.com");
    expect(
      ddnsOf(
        { ...draft, provider: "duckdns", hostname: "a.b.duckdns.org", secret: "x" },
        undefined,
        [],
      ).errors.hostname,
    ).toBe("A DuckDNS name is <name>.duckdns.org");
    expect(
      ddnsOf({ ...draft, provider: "no_ip", hostname: "a.ddns.net", secret: "x" }, undefined, [])
        .errors.username,
    ).toBe("1 to 128 printable characters without spaces");
    expect(
      ddnsOf(
        { ...draft, hostname: "home.example.com", zone: "example.com", secret: "x" },
        undefined,
        [CF],
      ).errors.hostname,
    ).toBe("Another name already keeps this A record");
    // A disabled entry may wait for its token.
    expect(
      ddnsOf(
        { ...draft, enabled: false, hostname: "h.example.com", zone: "example.com" },
        undefined,
        [],
      ).ddns,
    ).toBeDefined();
  });

  it("saves, replaces and removes entries", () => {
    const config = withDdns(siteConfig(), CF);
    expect(config.services.ddns).toEqual([CF]);
    const edited = withDdns(config, { ...CF, ipv6: true });
    expect(edited.services.ddns).toEqual([{ ...CF, ipv6: true }]);
    expect(withDdns(edited, { remove: "home" }).services.ddns).toEqual([]);
  });

  it("maps issues and reports state", () => {
    expect(
      ddnsErrorsOfIssues(
        [
          { path: "services/ddns/1/provider/zone", message: "zone" },
          { path: "services/ddns/0/hostname", message: "other row" },
        ],
        1,
      ),
    ).toEqual({ errors: { zone: "zone" }, other: ["other row"] });
    expect(ddnsStatus({ ...CF, enabled: false }, []).text).toBe("Off");
    expect(ddnsStatus(CF, undefined).text).toBe("No report from the gateway yet");
    expect(
      ddnsStatus(CF, [{ id: "home", address: null, updated_at: null, running: false }]),
    ).toEqual({ tone: "warn", text: "Updater not running" });
    expect(
      ddnsStatus(CF, [{ id: "home", address: "203.0.113.7", updated_at: 0, running: true }]),
    ).toMatchObject({ tone: "ok", text: "Registered 203.0.113.7" });
  });
});
