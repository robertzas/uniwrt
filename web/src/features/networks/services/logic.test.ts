import { describe, expect, it } from "vitest";
import type { Ddns } from "../../../api/gen/Ddns";
import { siteConfig } from "../../../test/site";
import { ddnsDraft, ddnsErrorsOfIssues, ddnsOf, ddnsStatus, newId, withDdns } from "./ddns";

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
