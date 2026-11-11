import { describe, expect, it } from "vitest";
import { DEFAULT_WAN } from "../../test/presets";
import { siteConfig } from "../../test/site";
import {
  addressDraftOf,
  CORE_PACKAGES,
  checkAddresses,
  checkWifi,
  mainNetwork,
  moves,
  setupSecrets,
  type WifiDraft,
  withSetup,
  wlanBands,
} from "./plan";

const PLAN = { space: "10.0.0.0/16", router: "10.0.0.1" };
const WIFI: WifiDraft = {
  enabled: true,
  ssid: "Maple Street",
  passphrase: "a long passphrase",
  country: "US",
};

describe("the address plan", () => {
  it("starts from the site's space and its main network", () => {
    expect(addressDraftOf(siteConfig())).toEqual({
      space: "192.168.0.0/16",
      router: "192.168.1.1",
    });
  });

  it("takes a private space and a router address inside it, outside the pool", () => {
    expect(checkAddresses(PLAN)).toEqual({});
    expect(checkAddresses({ space: "10.0.0.0", router: "10.0.0.1" }).space).toMatch(/CIDR/);
    expect(checkAddresses({ space: "10.0.0.0/28", router: "10.0.0.1" }).space).toMatch(
      /\/8 and \/24/,
    );
    expect(checkAddresses({ space: "8.8.0.0/16", router: "8.8.0.1" }).space).toMatch(/private/);
    expect(checkAddresses({ ...PLAN, router: "router" }).router).toMatch(/IPv4/);
    expect(checkAddresses({ ...PLAN, router: "10.1.0.1" }).router).toMatch(/inside 10.0.0.0\/16/);
    expect(checkAddresses({ ...PLAN, router: "10.0.3.0" }).router).toMatch(/\.0 or \.255/);
    expect(checkAddresses({ ...PLAN, router: "10.0.0.120" }).router).toMatch(/DHCP pool/);
    expect(checkAddresses({ ...PLAN, router: "10.0.7.254" })).toEqual({});
  });

  it("makes the main network the /24 around the router", () => {
    expect(mainNetwork({ space: "10.20.0.9/16", router: "10.20.7.254" })).toEqual({
      space: "10.20.0.0/16",
      address: "10.20.7.254/24",
      subnet: "10.20.7.0/24",
      start: "10.20.7.100",
      end: "10.20.7.249",
    });
  });

  it("knows when the router has to move", () => {
    expect(moves("192.168.1.1", PLAN)).toBe(true);
    expect(moves("10.0.0.1", PLAN)).toBe(false);
    expect(moves("router.lan", PLAN)).toBe(true);
  });
});

describe("the first Wi-Fi network", () => {
  it("needs a name, a passphrase and a country, unless it is left out", () => {
    expect(checkWifi(WIFI)).toEqual({});
    expect(
      Object.keys(checkWifi({ ...WIFI, ssid: " ", passphrase: "short", country: "" })),
    ).toEqual(["ssid", "passphrase", "country"]);
    expect(checkWifi({ ...WIFI, ssid: "é".repeat(17) }).ssid).toBeDefined();
    expect(checkWifi({ ...WIFI, country: "XX" }).country).toBeDefined();
    expect(checkWifi({ enabled: false, ssid: "", passphrase: "", country: "" })).toEqual({});
  });

  it("goes on 2.4 and 5 GHz only", () => {
    expect(wlanBands(["5g", "6g", "2g", "5g"])).toEqual(["2g", "5g"]);
    expect(wlanBands(["6g"])).toEqual([]);
  });
});

describe("the site the wizard saves", () => {
  const choices = {
    gateway: 7,
    wan: { ...DEFAULT_WAN, preset: "us-lumen-ipoe" },
    addresses: PLAN,
    wifi: WIFI,
    bands: ["2g", "5g"] as const,
    timezone: "America/Denver",
  };

  it("carries every choice in one document", () => {
    const base = siteConfig();
    const site = withSetup(base, choices);
    expect(site.addressing.ipv4).toBe("10.0.0.0/16");
    const lan = site.networks.find((n) => n.purpose === "default");
    expect(lan?.ipv4?.address).toBe("10.0.0.1/24");
    expect(lan?.ipv4?.dhcp).toMatchObject({ start: "10.0.0.100", end: "10.0.0.249" });
    expect(site.site).toMatchObject({ country: "US", timezone: "America/Denver" });
    expect(site.wan.preset).toBe("us-lumen-ipoe");
    // The gateway manages what a router needs; the feed packages wait.
    expect(site.nodes.find((n) => n.node === 7)?.managed).toEqual([...CORE_PACKAGES].sort());
    // The fixture's `home` WLAN is replaced, keeping its secret's reference.
    const home = site.wlans.filter((w) => w.id === "home");
    expect(home).toHaveLength(1);
    expect(home[0]).toMatchObject({
      ssid: "Maple Street",
      network: "lan",
      security: "wpa2_wpa3",
      bands: ["2g", "5g"],
      passphrase: base.wlans[0]?.passphrase,
    });
    expect(base.addressing.ipv4).toBe("192.168.0.0/16");
  });

  it("leaves Wi-Fi, the country and the time zone alone when they aren't chosen", () => {
    const base = { ...siteConfig(), wlans: [] };
    const site = withSetup(base, { ...choices, wifi: undefined, timezone: undefined });
    expect(site.wlans).toEqual([]);
    expect(site.site).toEqual(base.site);
    const off = withSetup(base, { ...choices, wifi: { ...WIFI, enabled: false } });
    expect(off.wlans).toEqual([]);
  });

  it("adds to what a node manages already, and drops reservations left outside", () => {
    const base = siteConfig();
    const lan = base.networks[0];
    lan?.ipv4?.dhcp?.reservations.push({
      mac: "02:00:00:00:00:01",
      ip: "192.168.1.20",
      name: null,
    });
    base.nodes = [{ node: 7, hostname: "gw", managed: ["sqm"], radios: [], ports: [], raw: [] }];
    const site = withSetup(base, choices);
    expect(site.nodes).toHaveLength(1);
    expect(site.nodes[0]).toMatchObject({ hostname: "gw" });
    expect(site.nodes[0]?.managed).toEqual([
      "dhcp",
      "firewall",
      "network",
      "sqm",
      "system",
      "wireless",
    ]);
    expect(site.networks[0]?.ipv4?.dhcp?.reservations).toEqual([]);
  });

  it("sends the secrets that were typed", () => {
    expect(setupSecrets(undefined, undefined)).toBeUndefined();
    expect(setupSecrets({ "wan/password": "pw" }, WIFI)).toEqual({
      "wan/password": "pw",
      "wlans/home/passphrase": "a long passphrase",
    });
    expect(setupSecrets(undefined, { ...WIFI, enabled: false })).toBeUndefined();
  });
});
