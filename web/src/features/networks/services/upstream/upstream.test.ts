import { describe, expect, it } from "vitest";
import type { Upstream } from "../../../../api/gen/Upstream";
import { dohHost, isSpkiPin, upstreamErrorsOfIssues, upstreamOf } from "./check";
import {
  type DotDraft,
  describeUpstream,
  ISP_UPSTREAM,
  moveResolver,
  presetResolver,
  type ResolverDraft,
  resolverDraft,
  resolverName,
  type UpstreamDraft,
  upstreamDraft,
  withResolver,
} from "./draft";

const PIN = "yioEpqeR4WtDwE9YxNVnCEkTxIjx6EEIwFSQW+lJsbc=";

const LAYERED: Upstream = {
  encrypted: [
    {
      protocol: "doh",
      url: "https://dns.nextdns.io/abc123",
      bootstrap: ["45.90.28.0", "45.90.30.0"],
    },
    { protocol: "dot", name: "dns.quad9.net", addresses: ["9.9.9.9"], port: 853, spki: [PIN] },
    {
      protocol: "dot",
      name: "dot.example",
      addresses: ["192.0.2.1", "2001:db8::1"],
      port: 8853,
      spki: [],
    },
    { protocol: "doh", url: "https://1.1.1.1/dns-query", bootstrap: [] },
  ],
  servers: ["9.9.9.9", "2620:fe::fe"],
  isp: true,
  strict: false,
};

const draft = (next: Partial<UpstreamDraft>): UpstreamDraft => ({
  ...upstreamDraft(ISP_UPSTREAM),
  ...next,
});
const names = (list: ResolverDraft[]) => list.map((r) => `${r.preset}/${r.protocol}`);

describe("upstream DNS", () => {
  it("reads a DoH URL's host and a key pin as the model does", () => {
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
    expect(isSpkiPin(PIN)).toBe(true);
    for (const bad of ["", PIN.slice(0, -1), `${PIN.slice(0, 42)}b=`, `${PIN.slice(0, 42)}A-`]) {
      expect(isSpkiPin(bad), bad).toBe(false);
    }
  });

  it("recognizes presets of both protocols, NextDNS with its profile, and other resolvers", () => {
    expect(
      resolverDraft({ protocol: "doh", url: "https://dns.nextdns.io/abc123", bootstrap: [] }),
    ).toMatchObject({
      preset: "nextdns",
      profile: "abc123",
    });
    expect(
      resolverDraft({
        protocol: "doh",
        url: "https://dns.google/dns-query",
        bootstrap: ["8.8.8.8", "8.8.4.4"],
      }),
    ).toMatchObject({ preset: "google", bootstrap: "8.8.8.8, 8.8.4.4" });
    const dot = { protocol: "dot" as const, addresses: ["1.1.1.1"], port: 853, spki: [] };
    expect(resolverDraft({ ...dot, name: "cloudflare-dns.com" }).preset).toBe("cloudflare");
    expect(resolverDraft({ ...dot, name: "abc123.dns.nextdns.io" })).toMatchObject({
      preset: "nextdns",
      profile: "abc123",
    });
    // Another port is another server.
    expect(resolverDraft({ ...dot, name: "dns.quad9.net", port: 8853 }).preset).toBe("custom");
    expect(
      resolverName({ protocol: "doh", url: "https://doh.example/dns-query", bootstrap: [] }),
    ).toBe("doh.example");
    expect(resolverName({ ...dot, name: "dot.example" })).toBe("dot.example");
    expect(resolverName({ ...dot, name: "dns.quad9.net" })).toBe("Quad9");
  });

  it("edits a layered upstream and saves it as it was", () => {
    const edited = upstreamDraft(LAYERED);
    expect(names(edited.encrypted)).toEqual([
      "nextdns/doh",
      "quad9/dot",
      "custom/dot",
      "custom/doh",
    ]);
    expect(upstreamOf(edited)).toEqual({
      upstream: LAYERED,
      errors: { servers: [undefined, undefined], resolvers: [{}, {}, {}, {}] },
    });
    expect(describeUpstream(LAYERED)).toBe(
      "NextDNS (DoH), Quad9 (DoT), dot.example (DoT), 1.1.1.1 (DoH) → 9.9.9.9, 2620:fe::fe → the ISP's",
    );
    expect(describeUpstream({ ...LAYERED, strict: true })).toBe(
      "NextDNS (DoH), Quad9 (DoT), dot.example (DoT), 1.1.1.1 (DoH) only",
    );
    expect(describeUpstream(ISP_UPSTREAM)).toBe("the ISP's");
  });

  it("keeps the DoT resolvers together when adding and moving", () => {
    const doh = presetResolver("doh", "quad9");
    const google = presetResolver("dot", "google");
    const adguard = presetResolver("dot", "adguard");
    let list = withResolver(withResolver([google], doh), adguard);
    expect(names(list)).toEqual(["google/dot", "adguard/dot", "quad9/doh"]);
    // Within the DoT ones: a swap.
    expect(names(moveResolver(list, 0, 1))).toEqual(["adguard/dot", "google/dot", "quad9/doh"]);
    // Past a DoH resolver: all of them.
    list = moveResolver(list, 1, 1);
    expect(names(list)).toEqual(["quad9/doh", "google/dot", "adguard/dot"]);
    expect(names(moveResolver(list, 0, 1))).toEqual(["google/dot", "adguard/dot", "quad9/doh"]);
    expect(moveResolver(list, 0, -1)).toEqual(list);
    // A document with them apart is edited with them together.
    const apart = upstreamDraft({
      ...ISP_UPSTREAM,
      encrypted: [LAYERED.encrypted[1], LAYERED.encrypted[0], LAYERED.encrypted[2]].flatMap((r) =>
        r ? [r] : [],
      ),
    });
    expect(names(apart.encrypted)).toEqual(["quad9/dot", "custom/dot", "nextdns/doh"]);
  });

  it("checks the layers as a whole", () => {
    expect(upstreamOf(draft({ isp: false })).errors.layers).toBe(
      "Choose where queries go: encrypted resolvers, DNS servers or the ISP's",
    );
    expect(upstreamOf(draft({ strict: true })).errors.strict).toBe(
      "Encrypted only needs an encrypted resolver",
    );
    // Strict keeps the plain layers.
    expect(
      upstreamOf(
        draft({ encrypted: [presetResolver("dot", "quad9")], servers: ["9.9.9.9"], strict: true }),
      ).upstream,
    ).toMatchObject({ servers: ["9.9.9.9"], isp: true, strict: true });
    const bad = upstreamOf(draft({ servers: ["9.9.9.9", "nope", "9.9.9.9"] }));
    expect(bad.upstream).toBeUndefined();
    expect(bad.errors.servers).toEqual([undefined, "Not an IP address", "Named twice"]);
    expect(
      upstreamOf(draft({ servers: ["1.1.1.1", "1.0.0.1", "8.8.8.8", "8.8.4.4", "9.9.9.9"] })).errors
        .serverList,
    ).toBe("At most 4 DNS servers");
  });

  it("checks DoH resolvers and their bootstrap servers", () => {
    const bad = upstreamOf(
      draft({
        encrypted: [
          presetResolver("doh", "nextdns"),
          {
            protocol: "doh",
            preset: "custom",
            url: "https://doh.example/q",
            profile: "",
            bootstrap: "1.1.1.1",
          },
          {
            protocol: "doh",
            preset: "custom",
            url: "https://1.1.1.1/dns-query",
            profile: "",
            bootstrap: "",
          },
          {
            protocol: "doh",
            preset: "custom",
            url: "http://x.example",
            profile: "",
            bootstrap: "1.1.1.1 x",
          },
        ],
      }),
    );
    expect(bad.errors.resolvers).toEqual([
      { url: "Enter your NextDNS profile ID: letters and digits", bootstrap: undefined },
      { bootstrap: "Name 2 to 4 IP addresses" },
      { bootstrap: undefined },
      {
        url: "An https:// URL, such as https://dns.quad9.net/dns-query",
        bootstrap: "x is not an IP address",
      },
    ]);
  });

  it("checks DoT resolvers: name, addresses, port and pins", () => {
    const dot = presetResolver("dot", "custom") as DotDraft;
    const bad = upstreamOf(
      draft({
        encrypted: [
          { ...(presetResolver("dot", "nextdns") as DotDraft), profile: "a b" },
          { ...dot, name: "localhost", addresses: "", port: "0", spki: "nope" },
          { ...(presetResolver("dot", "quad9") as DotDraft), spki: `sha256/${PIN}` },
          { ...(presetResolver("dot", "quad9") as DotDraft), addresses: "9.9.9.9, 9.9.9.9" },
        ],
      }),
    );
    expect(bad.errors.resolvers).toEqual([
      { name: "Enter your NextDNS profile ID: letters and digits", addresses: undefined },
      {
        name: "A host name, such as dns.quad9.net",
        port: "A port from 1 to 65535",
        addresses: "Name 1 to 4 IP addresses",
        spki: "nope is not a key pin: a base64 SHA-256, 43 characters and =",
      },
      { addresses: undefined },
      { name: "Named twice", addresses: "Named twice" },
    ]);
    const ok = upstreamOf(
      draft({ encrypted: [{ ...presetResolver("dot", "nextdns"), profile: "abc123" }] }),
    );
    expect(ok.upstream?.encrypted).toEqual([
      {
        protocol: "dot",
        name: "abc123.dns.nextdns.io",
        addresses: ["45.90.28.0", "45.90.30.0"],
        port: 853,
        spki: [],
      },
    ]);
  });

  it("maps the server's issues onto layers and rows", () => {
    const { errors, other } = upstreamErrorsOfIssues([
      { path: "services/upstream", message: "none" },
      { path: "services/upstream/strict", message: "strict" },
      { path: "services/upstream/encrypted", message: "list" },
      { path: "services/upstream/encrypted/1/bootstrap/0", message: "loopback" },
      { path: "services/upstream/encrypted/2", message: "apart" },
      { path: "services/upstream/encrypted/2/spki/1", message: "pin" },
      { path: "services/upstream/servers/2", message: "twice" },
      { path: "wan/proto", message: "elsewhere" },
    ]);
    expect(errors).toMatchObject({ layers: "none", strict: "strict", encrypted: "list" });
    expect(errors.resolvers[1]).toEqual({ bootstrap: "loopback" });
    expect(errors.resolvers[2]).toEqual({ row: "apart", spki: "pin" });
    expect(errors.servers[2]).toBe("twice");
    expect(other).toEqual(["elsewhere"]);
  });
});
