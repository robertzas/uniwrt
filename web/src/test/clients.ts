import type { Client } from "../api/gen/Client";
import type { ClientDetail } from "../api/gen/ClientDetail";
import type { ClientSample } from "../api/gen/ClientSample";
import type { ClientSession } from "../api/gen/ClientSession";
import type { EventRecord } from "../api/gen/EventRecord";
import type { Steered } from "../lib/steering";

/* Made-up clients for tests: a phone on node 2's 5 GHz AP, unless `extra` says otherwise. */

export const NOW = 1_800_000_000_000;

export function client(mac: string, extra: Partial<Client> = {}): Client {
  return {
    mac,
    alias: null,
    hostname: "Annas-iPhone",
    vendor: "Apple",
    kind: "phone",
    randomized: false,
    blocked: null,
    group: null,
    ips: ["192.168.1.40"],
    online: true,
    connection: {
      node: 2,
      upstream: "node:2",
      upstream_name: "ap-living",
      port: "phy1-ap0",
      medium: "wifi",
      since: NOW - 3_600_000,
      band: "5g",
      ssid: "Home",
      channel: 36,
      signal_dbm: -58,
      noise_dbm: -95,
      rx_rate: null,
      tx_rate: {
        kbps: 864_000,
        standard: "he",
        mcs: 9,
        nss: 2,
        width: 80,
        guard_interval_ns: 800,
      },
      standard: "he",
      capabilities: ["11k", "11r", "11v"],
      capacity_mbps: 700,
      traffic: { at: NOW, down_bytes: 1e9, up_bytes: 1e8, down_bps: 2_400_000, up_bps: 120_000 },
    },
    first_seen: NOW - 86_400_000,
    last_seen: NOW,
    experience: {
      score: 92,
      parts: [
        { factor: "snr", score: 100, weight: 35, value: 37 },
        { factor: "retries", score: 71, weight: 20, value: 15 },
      ],
    },
    wifi: { standard: "he", bands: ["2g", "5g"], capabilities: ["11k", "11r", "11v"] },
    ...extra,
  };
}

export function session(start: number, extra: Partial<ClientSession> = {}): ClientSession {
  return {
    mac: "f0:18:98:00:00:01",
    node: 2,
    port: "phy1-ap0",
    medium: "wifi",
    band: "5g",
    ssid: "Home",
    start,
    end: null,
    ended_by: null,
    ...extra,
  };
}

export function sample(at: number, signal: number): ClientSample {
  return {
    at,
    signal_dbm: signal,
    snr_db: signal + 95,
    tx_kbps: 864_000,
    rx_kbps: 576_000,
    down_bps: 2_000_000,
    up_bps: 100_000,
    score: 90,
  };
}

export function detail(mac = "f0:18:98:00:00:01"): ClientDetail {
  return {
    client: client(mac),
    samples: [sample(NOW - 10_000, -60), sample(NOW - 5_000, -58)],
    sessions: [
      session(NOW - 7_200_000, {
        node: 1,
        port: "phy0-ap0",
        band: "2g",
        end: NOW - 3_600_000,
        ended_by: "roamed",
      }),
      session(NOW - 3_600_000),
    ],
  };
}

/** A steering event at node 2's 5 GHz AP: by default the client agreed to move to node 1. */
export function steered(
  at: number,
  what: Partial<Steered["what"]> = {},
  extra: Partial<Omit<EventRecord, "what">> = {},
): Steered {
  return {
    id: at,
    at,
    last_at: at,
    count: 1,
    level: "info",
    category: "client",
    node: 2,
    client: "f0:18:98:00:00:01",
    condition: null,
    what: {
      kind: "client_steered",
      action: "transition_accepted",
      iface: "phy1-ap0",
      reason: null,
      signal_dbm: null,
      min_signal_dbm: null,
      to: { node: 1, iface: "phy1-ap0" },
      to_signal_dbm: null,
      status: null,
      ...what,
    },
    ...extra,
  };
}
