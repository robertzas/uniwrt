import { describe, expect, it } from "vitest";
import type { Network } from "../../api/gen/Network";
import type { SiteConfig } from "../../api/gen/SiteConfig";
import type { Tailscale } from "../../api/gen/Tailscale";
import type { TailscaleFacts } from "../../api/gen/TailscaleFacts";
import type { TailscaleStatus } from "../../api/gen/TailscaleStatus";
import { siteConfig } from "../../test/site";
import { peerText } from "./TailscalePeers";
import {
  AUTH_KEY,
  isAuthKey,
  tailscaleDraft,
  tailscaleErrorsOfIssues,
  tailscaleOf,
  tailscaleState,
  unapprovedText,
  withTailscale,
} from "./tailscale";

const KEY = "tskey-auth-k1234CNTRL-abcdef";

function config(): SiteConfig {
  const c = siteConfig();
  const lan = c.networks[0] as Network;
  c.networks.push(
    { ...lan, id: "iot", name: "IoT", purpose: "iot", ipv4: null },
    {
      ...lan,
      id: "guest",
      name: "Guest",
      purpose: "guest",
      ipv4: { address: "10.0.30.1/24", dhcp: null },
    },
  );
  return c;
}

const saved: Tailscale = {
  enabled: true,
  routes: ["lan"],
  exit_node: false,
  hostname: null,
  auth_key: "config/00000000000000000000000000000001",
};

function daemon(over: Partial<TailscaleFacts> = {}): TailscaleFacts {
  return {
    state: "running",
    version: "1.80.3",
    tailnet: "example.com",
    dns_suffix: "tail1234.ts.net",
    dns_name: "gw.tail1234.ts.net",
    addresses: ["100.101.102.103"],
    online: true,
    approved_routes: [],
    health: [],
    peer_count: 0,
    peers: [],
    prefs: {
      want_running: true,
      logged_out: false,
      advertise_routes: [],
      hostname: null,
      accept_dns: false,
      accept_routes: false,
      netfilter_off: true,
    },
    ...over,
  };
}

function status(over: Partial<TailscaleStatus> = {}): TailscaleStatus {
  return {
    online: true,
    collected_at: 1,
    daemon: daemon(),
    prefs_applied: true,
    unapproved_routes: [],
    ...over,
  };
}

describe("Tailscale's settings", () => {
  it("starts a new setup on the default networks, logging in through the URL", () => {
    expect(tailscaleDraft(null, config())).toMatchObject({
      enabled: true,
      routes: ["lan"],
      exitNode: false,
      login: "url",
      keySaved: false,
    });
    expect(tailscaleDraft(saved, config())).toMatchObject({ login: "key", keySaved: true });
  });

  it("checks auth keys as the model does", () => {
    expect(isAuthKey(KEY)).toBe(true);
    expect(isAuthKey("tskey-")).toBe(false);
    expect(isAuthKey("tskey-a b")).toBe(false);
    expect(isAuthKey("abc")).toBe(false);
  });

  it("saves routes in the networks' order and only those with a subnet", () => {
    const c = config();
    const draft = { ...tailscaleDraft(null, c), routes: ["guest", "iot", "lan"] };
    const { edit } = tailscaleOf(draft, null, c);
    expect(edit).toEqual({
      tailscale: {
        enabled: true,
        routes: ["lan", "guest"],
        exit_node: false,
        hostname: null,
        auth_key: null,
      },
    });
  });

  it("sends a new auth key, keeps a saved one, and clears it for the URL", () => {
    const c = config();
    const draft = tailscaleDraft(saved, c);
    expect(tailscaleOf(draft, saved, c).edit?.secrets).toBeUndefined();
    expect(tailscaleOf({ ...draft, authKey: ` ${KEY} ` }, saved, c).edit?.secrets).toEqual({
      [AUTH_KEY]: KEY,
    });
    expect(tailscaleOf({ ...draft, login: "url" }, saved, c).edit?.secrets).toEqual({
      [AUTH_KEY]: null,
    });
    expect(tailscaleOf({ ...draft, login: "url" }, saved, c).edit?.tailscale.auth_key).toBe(
      saved.auth_key,
    );
  });

  it("refuses a bad name, a bad or missing key, and UDP 41641 taken", () => {
    const c = config();
    const draft = tailscaleDraft(null, c);
    expect(tailscaleOf({ ...draft, hostname: "my gw" }, null, c).errors.hostname).toBeDefined();
    expect(tailscaleOf({ ...draft, login: "key" }, null, c).errors.authKey).toMatch(/Paste/);
    expect(
      tailscaleOf({ ...draft, login: "key", authKey: "nope" }, null, c).errors.authKey,
    ).toMatch(/tskey-/);
    const forward = {
      name: "Game",
      enabled: true,
      proto: "tcp_udp" as const,
      ports: { first: 41000, last: 42000 },
      to: "192.168.1.20",
      to_port: null,
      source: null,
      reflection: false,
    };
    c.firewall.forwards = [forward];
    expect(tailscaleOf(draft, null, c).errors.port).toMatch(/Game/);
    c.firewall.forwards = [{ ...forward, proto: "tcp" }];
    expect(tailscaleOf(draft, null, c).errors.port).toBeUndefined();
    // Off, Tailscale takes no port.
    c.firewall.forwards = [forward];
    expect(tailscaleOf({ ...draft, enabled: false }, null, c).errors).toEqual({});
  });

  it("drops the zone's policies with Tailscale", () => {
    const c = config();
    c.vpn.tailscale = saved;
    c.firewall.policies = [
      { from: "tailscale", to: "lan" },
      { from: "lan", to: "tailscale" },
      { from: "guest", to: "lan" },
    ] as unknown as SiteConfig["firewall"]["policies"];
    expect(withTailscale(c, saved).firewall.policies).toHaveLength(3);
    const removed = withTailscale(c, null);
    expect(removed.vpn.tailscale).toBeNull();
    expect(removed.firewall.policies).toEqual([{ from: "guest", to: "lan" }]);
  });

  it("puts the server's issues on their fields", () => {
    const { errors, other } = tailscaleErrorsOfIssues([
      { path: "vpn/tailscale/hostname", message: "bad name" },
      { path: "vpn/tailscale/routes/1", message: "no subnet" },
      { path: AUTH_KEY, message: "bad key" },
      { path: "vpn/tailscale", message: "port taken" },
    ]);
    expect(errors).toEqual({ hostname: "bad name", routes: "no subnet", authKey: "bad key" });
    expect(other).toEqual(["port taken"]);
  });
});

describe("Tailscale's state", () => {
  const state = (s: TailscaleStatus | undefined, ts: Tailscale = saved) => tailscaleState(ts, s);

  it("follows the site and the gateway's report", () => {
    expect(state(status(), { ...saved, enabled: false }).text).toBe("Off");
    expect(state(undefined).text).toMatch(/No report/);
    expect(state(status({ online: false })).tone).toBe("warn");
    expect(state(status({ daemon: null })).text).toMatch(/apply/);
    expect(state(status({ daemon: daemon({ state: null }) })).text).toMatch(/isn't answering/);
  });

  it("offers a login while logged out and a logout once in", () => {
    const out = daemon({
      state: "needs_login",
      prefs: { ...(daemon().prefs as NonNullable<TailscaleFacts["prefs"]>), logged_out: true },
    });
    expect(state(status({ daemon: out }))).toMatchObject({ login: true, logout: false });
    expect(state(status({ daemon: out })).text).toMatch(/saved auth key/);
    expect(state(status({ daemon: out }), { ...saved, auth_key: null }).text).toMatch(/log in/);
    expect(state(status())).toMatchObject({
      tone: "ok",
      text: "Connected",
      login: false,
      logout: true,
    });
    expect(state(status({ daemon: daemon({ online: false }) })).tone).toBe("warn");
    expect(state(status({ daemon: daemon({ state: "needs_machine_auth" }) })).text).toMatch(
      /approval/,
    );
  });

  it("names the exit node among routes awaiting approval", () => {
    expect(unapprovedText(["10.0.30.0/24", "0.0.0.0/0"])).toBe("10.0.30.0/24, the exit node");
  });

  it("says how a peer is reached", () => {
    const peer = {
      name: "laptop",
      dns_name: null,
      os: "linux",
      addresses: ["100.64.0.2"],
      online: true,
      active: true,
      exit_node_option: false,
      direct: "198.51.100.7:41641",
      relay: "fra",
      last_seen: null,
      rx_bytes: 0,
      tx_bytes: 0,
    };
    expect(peerText(peer, 0)).toBe("Direct · 198.51.100.7:41641");
    expect(peerText({ ...peer, direct: null }, 0)).toBe("Relayed · fra");
    expect(peerText({ ...peer, online: false, last_seen: 0 }, 120_000)).toMatch(/^Seen 2/);
    expect(peerText({ ...peer, online: false }, 0)).toBe("Offline");
  });
});
