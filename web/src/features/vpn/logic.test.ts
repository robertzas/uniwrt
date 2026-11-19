import { describe, expect, it } from "vitest";
import type { VpnPeer } from "../../api/gen/VpnPeer";
import type { WireGuard } from "../../api/gen/WireGuard";
import { siteConfig } from "../../test/site";
import {
  endpointText,
  freeAddress,
  isKey,
  peerDraft,
  peerErrorsOfIssues,
  peerId,
  peerOf,
  peerState,
  serverDraft,
  serverOf,
  withServer,
} from "./wireguard";

const KEY = `${"A".repeat(42)}E=`;
const OTHER = `${"B".repeat(42)}E=`;

const phone: VpnPeer = {
  id: "phone",
  name: "Robin's phone",
  enabled: true,
  address: "10.0.250.2",
  public_key: KEY,
  private_key: "config/00000000000000000000000000000002",
  preshared_key: "config/00000000000000000000000000000003",
  role: { kind: "client", full_tunnel: true },
};

function server(peers: VpnPeer[] = [phone]): WireGuard {
  return {
    enabled: true,
    port: 51820,
    address: "10.0.250.1/24",
    endpoint: { kind: "host", host: "vpn.example.com" },
    dns: true,
    private_key: "config/00000000000000000000000000000001",
    public_key: OTHER,
    peers,
  };
}

describe("WireGuard keys and ids", () => {
  it("knows a key as wg prints it", () => {
    expect(isKey(KEY)).toBe(true);
    expect(isKey(KEY.slice(1))).toBe(false);
    expect(isKey(`${"A".repeat(43)}?`)).toBe(false);
  });

  it("makes a peer's id from its name, unique", () => {
    expect(peerId("Robin's phone", new Set())).toBe("robins_phone");
    expect(peerId("Living room laptop", new Set())).toBe("living_room");
    expect(peerId("Supercalifragilistic", new Set())).toBe("supercalifra");
    expect(peerId("Laptop", new Set(["laptop"]))).toBe("laptop_2");
    expect(peerId("2nd site", new Set())).toBe("p2nd_site");
    expect(peerId("!!!", new Set())).toBe("p");
  });

  it("gives the lowest free address of the tunnel", () => {
    expect(freeAddress(server())).toBe("10.0.250.3");
    expect(freeAddress(server([]))).toBe("10.0.250.2");
    expect(freeAddress({ ...server([]), address: "10.0.250.2/30" })).toBe("10.0.250.1");
  });
});

describe("The server's draft", () => {
  it("starts at the defaults with the first enabled DDNS name", () => {
    const ddns = [
      { id: "off", enabled: false, hostname: "a.example.com" },
      { id: "home", enabled: true, hostname: "home.example.com" },
    ] as never;
    expect(serverDraft(null, ddns)).toMatchObject({
      port: "51820",
      address: "10.0.250.1/24",
      endpoint: "ddns",
      ddns: "home",
      dns: true,
    });
    expect(serverDraft(null, []).endpoint).toBe("host");
  });

  it("checks what the model checks", () => {
    const config = siteConfig();
    const draft = { ...serverDraft(null, []), host: "vpn.example.com" };
    expect(serverOf({ ...draft, port: "0" }, null, config).errors.port).toBeDefined();
    expect(serverOf({ ...draft, address: "10.0.250.0/24" }, null, config).errors.address).toMatch(
      /host of 10.0.250.0\/24/,
    );
    expect(serverOf({ ...draft, address: "8.8.8.1/24" }, null, config).errors.address).toMatch(
      /private/,
    );
    expect(serverOf({ ...draft, address: "192.168.0.1/16" }, null, config).errors.address).toMatch(
      /overlaps/,
    );
    expect(serverOf({ ...draft, address: "10.0.0.1/8" }, null, config).errors.address).toMatch(
      /prefix/,
    );
    expect(serverOf({ ...draft, host: "nohost" }, null, config).errors.endpoint).toBeDefined();
    expect(serverOf({ ...draft, endpoint: "ddns", ddns: "x" }, null, config).errors.endpoint).toBe(
      "Choose a DDNS name.",
    );
    config.firewall.forwards = [
      {
        name: "Game",
        enabled: true,
        proto: "udp",
        ports: { first: 51000, last: 52000 },
      } as never,
    ];
    expect(serverOf(draft, null, config).errors.port).toBe(
      "The port forward Game takes UDP port 51820.",
    );
  });

  it("keeps the peers and keys of the server it edits", () => {
    const current = server();
    const draft = { ...serverDraft(current, []), port: "443" };
    const { server: saved } = serverOf(draft, current, siteConfig());
    expect(saved).toEqual({ ...current, port: 443 });
  });

  it("drops the zone's policies with the server", () => {
    const config = withServer(siteConfig(), server());
    config.firewall.policies = [
      { from: "vpn", to: "wan", action: "block" },
      { from: "lan", to: "vpn", action: "block" },
      { from: "guest", to: "wan", action: "block" },
    ];
    expect(withServer(config, null).firewall.policies).toEqual([
      { from: "guest", to: "wan", action: "block" },
    ]);
  });

  it("names the endpoint peers dial", () => {
    expect(endpointText(server(), [])).toBe("vpn.example.com:51820");
    expect(endpointText({ ...server(), endpoint: { kind: "host", host: "2001:db8::1" } }, [])).toBe(
      "[2001:db8::1]:51820",
    );
    expect(
      endpointText({ ...server(), endpoint: { kind: "ddns", id: "home" } }, [
        { id: "home", hostname: "home.example.com" } as never,
      ]),
    ).toBe("home.example.com:51820");
  });
});

describe("A peer's draft", () => {
  const config = siteConfig();

  it("leaves a new peer's keys to the save", () => {
    const wg = server();
    const draft = { ...peerDraft(undefined, wg), name: "Laptop" };
    expect(draft.address).toBe("10.0.250.3");
    const { peer, secrets } = peerOf(draft, undefined, wg, config);
    expect(peer).toEqual({
      id: "laptop",
      name: "Laptop",
      enabled: true,
      address: "10.0.250.3",
      public_key: "",
      private_key: null,
      preshared_key: null,
      role: { kind: "client", full_tunnel: true },
    });
    expect(secrets).toBeUndefined();
  });

  it("clears the private key of a peer that brings its own", () => {
    const wg = server();
    const own = `${"C".repeat(42)}E=`;
    const draft = { ...peerDraft(phone, wg), ownKey: true, publicKey: own };
    const { peer, secrets } = peerOf(draft, phone, wg, config);
    expect(peer).toMatchObject({ public_key: own, private_key: null });
    expect(secrets).toEqual({ "vpn/wireguard/peers/phone/private_key": null });
    // Back to keys made by uniwrt: the save makes a new pair.
    const back = { ...phone, public_key: own, private_key: null };
    expect(peerDraft(back, wg).ownKey).toBe(true);
    const again = peerOf({ ...peerDraft(back, wg), ownKey: false }, back, wg, config);
    expect(again.peer).toMatchObject({ public_key: "", private_key: null });
  });

  it("checks names, addresses, keys and a site's networks", () => {
    const wg = server();
    const base = peerDraft(undefined, wg);
    const errors = (draft: Partial<typeof base>) =>
      peerOf({ ...base, name: "x", ...draft }, undefined, wg, config).errors;
    expect(errors({ name: " " }).name).toBeDefined();
    expect(errors({ name: "robin's PHONE" }).name).toMatch(/Another peer/);
    expect(errors({ address: "10.0.250.1" }).address).toMatch(/taken/);
    expect(errors({ address: "10.0.250.2" }).address).toMatch(/taken/);
    expect(errors({ address: "10.0.251.2" }).address).toMatch(/10.0.250.0\/24/);
    expect(errors({ ownKey: true, publicKey: "nope" }).publicKey).toBeDefined();
    expect(errors({ ownKey: true, publicKey: OTHER }).publicKey).toMatch(/Another peer/);
    expect(errors({ kind: "site", networks: "" }).networks).toMatch(/1 to 16/);
    expect(errors({ kind: "site", networks: "192.168.50.1/24" }).networks).toBe(
      "Write 192.168.50.1/24 as 192.168.50.0/24.",
    );
    expect(errors({ kind: "site", networks: "10.0.250.0/25" }).networks).toMatch(/tunnel/);
    expect(errors({ kind: "site", networks: "192.168.50.0/24, 192.168.50.0/25" }).networks).toMatch(
      /this site/,
    );
    expect(errors({ kind: "site", networks: "192.168.50.0/24", host: "x" }).host).toBeDefined();
  });

  it("makes a site with its networks and endpoint", () => {
    const wg = server();
    const draft = {
      ...peerDraft(undefined, wg),
      name: "Cabin",
      kind: "site" as const,
      networks: "192.168.50.0/24\n192.168.51.0/24",
      host: "cabin.example.com",
      port: "51821",
    };
    expect(peerOf(draft, undefined, wg, config).peer?.role).toEqual({
      kind: "site",
      networks: ["192.168.50.0/24", "192.168.51.0/24"],
      endpoint: { host: "cabin.example.com", port: 51821 },
    });
    expect(peerOf({ ...draft, host: "" }, undefined, wg, config).peer?.role).toMatchObject({
      endpoint: null,
    });
  });

  it("maps the server's issues onto the fields", () => {
    const mapped = peerErrorsOfIssues(
      [
        { path: "vpn/wireguard/peers/cabin/role/networks/1", message: "overlaps" },
        { path: "vpn/wireguard/peers/cabin/name", message: "Two peers" },
        { path: "vpn/wireguard/port", message: "port" },
      ],
      "cabin",
    );
    expect(mapped).toEqual({
      errors: { networks: "overlaps", name: "Two peers" },
      other: ["port"],
    });
  });
});

describe("A peer's state", () => {
  const tunnel = { running: true, online: true };
  const status = {
    id: "phone",
    on_gateway: true,
    connected: true,
    handshake_at: 1_000_000,
    endpoint: "203.0.113.9:4500",
    rx_bytes: 1_500_000,
    tx_bytes: 20_000,
  };

  it("reads the gateway's report", () => {
    expect(peerState(phone, status, tunnel, 1_060_000)).toEqual({
      tone: "ok",
      text: "Connected · handshake 1m ago",
      traffic: "↓ 1.5 MB · ↑ 20 KB",
      endpoint: "203.0.113.9:4500",
    });
    expect(
      peerState(phone, { ...status, connected: false }, tunnel, 1_000_000 + 3 * 3600_000).text,
    ).toBe("Last handshake 3h ago");
    expect(
      peerState(phone, { ...status, connected: false, handshake_at: null }, tunnel, 0).text,
    ).toBe("Never connected");
    expect(peerState(phone, { ...status, on_gateway: false }, tunnel, 0).tone).toBe("info");
    expect(peerState(phone, status, { ...tunnel, running: false }, 0).text).toBe("Tunnel down");
    expect(peerState({ ...phone, enabled: false }, status, tunnel, 0).text).toBe("Off");
    expect(peerState(phone, undefined, undefined, 0).text).toBe("No report yet");
  });
});
