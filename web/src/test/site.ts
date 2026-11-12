import type { RevisionMeta } from "../api/gen/RevisionMeta";
import type { SiteConfig } from "../api/gen/SiteConfig";
import type { SiteView } from "../api/gen/SiteView";

/** Demo mode's first revision (the simulated network), as `GET /api/v1/site` answers it. */
export function siteConfig(): SiteConfig {
  return {
    schema: 2,
    site: {
      name: "Demo home",
      country: "US",
      timezone: "UTC",
      domain: "lan",
      ntp_servers: [
        "0.openwrt.pool.ntp.org",
        "1.openwrt.pool.ntp.org",
        "2.openwrt.pool.ntp.org",
        "3.openwrt.pool.ntp.org",
      ],
    },
    addressing: {
      ipv4: "192.168.0.0/16",
      ula: null,
    },
    networks: [
      {
        id: "lan",
        name: "Default",
        purpose: "default",
        vlan: null,
        ipv4: {
          address: "192.168.1.1/24",
          dhcp: {
            start: "192.168.1.100",
            end: "192.168.1.249",
            lease_time_s: 43200,
            reservations: [],
            options: { dns: [], ntp: [], domain: null, boot: null, custom: [] },
          },
        },
        ipv6: "delegated",
        ipv6_clients: "slaac_stateless",
        isolation: false,
        mdns: true,
        igmp_snooping: false,
      },
    ],
    wlans: [
      {
        id: "home",
        ssid: "uniwrt-demo",
        enabled: true,
        network: "lan",
        security: "wpa2_wpa3",
        passphrase: "config/00000000000000000000000000000001",
        bands: ["2g", "5g"],
        nodes: {
          scope: "all",
        },
        hidden: false,
        isolation: false,
        max_clients: null,
        schedule: null,
        roaming: { fast: null, hints: true, steer: true },
      },
    ],
    mesh: {
      enabled: true,
      mesh_id: "uniwrt-demo-mesh",
      key: "config/00000000000000000000000000000002",
      band: "5g",
      routing: "batman_iv",
      wired: true,
      wired_vlan: 4094,
    },
    radio: {
      bands: [
        {
          band: "2g",
          channel: "auto",
          width: 20,
          tx_power: "auto",
        },
        {
          band: "5g",
          channel: "auto",
          width: 80,
          tx_power: "auto",
        },
        {
          band: "6g",
          channel: "auto",
          width: 160,
          tx_power: "auto",
        },
      ],
    },
    steering: { profile: "off" },
    wan: {
      proto: {
        kind: "dhcp",
        vendor_class: null,
        client_id: null,
        send_options: [],
        hostname: null,
      },
      vlan: null,
      pcp: null,
      mtu: null,
      mac: null,
      ipv6: { mode: "dhcpv6", reqprefix: null },
      preset: null,
      sqm: null,
      failover: null,
    },
    firewall: {
      policies: [],
      forwards: [],
      dmz: null,
      upnp: { enabled: true, networks: ["lan"] },
    },
    services: {
      dns_records: [],
    },
    nodes: [],
  };
}

export function revisionMeta(id: number, extra: Partial<RevisionMeta> = {}): RevisionMeta {
  return {
    id,
    parent: id > 1 ? id - 1 : null,
    kind: id > 1 ? "edit" : "defaults",
    restored_from: null,
    author_id: id > 1 ? 1 : null,
    author: id > 1 ? "admin" : null,
    message: `Revision ${id}`,
    created_at: 1_700_000_000_000 + id * 60_000,
    schema: 2,
    changes: 1,
    ...extra,
  };
}

export function siteView(id = 1, config = siteConfig()): SiteView {
  return { revision: revisionMeta(id), config, secrets: [], issues: [] };
}
