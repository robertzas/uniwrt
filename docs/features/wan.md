# WAN & ISP setup

Read this when working on the gateway's internet connection: ISP presets, WAN types, IPv6, connectivity tests, SQM or failover.
Milestones: M4.1 (presets, WAN, wizard), M4.4 (first-run), M4.5 (SQM, failover).

## Wizard flow

Country → ISP (searchable; "Other / generic" is always available) → connection variant (for example, IPoE vs PPPoE) → only the fields that preset needs (credentials, static IPs) → **Test** → **Apply**. Apply is a transaction that rolls back automatically if the internet health check fails.
An advanced drawer exposes every field (VLAN, priority, MTU, MAC clone, DHCP options, IPv6 mode); presets only pre-fill them.

## Preset format

One file per preset at `data/isp/<country>-<isp>-<variant>.toml`. Adding a preset needs no code change; `cargo test` loads and validates every file.

```toml
id = "us-lumen-ipoe"
name = "Lumen / Quantum Fiber / CenturyLink Fiber (IPoE)"
country = "US"
status = "community"            # verified | community | unverified
sources = ["<link to the confirmation>"]
notes = "Replaces the ISP router. Leave the ONT/SmartNID in bridge mode or remove it."

[ipv4]
proto = "dhcp"
vlan = 201
pcp = 0                          # 802.1p priority on egress
mtu = 1500

[ipv6]
mode = "auto"                    # try DHCPv6-PD first, then fall back to 6rd
reqprefix = "auto"

[ipv6.sixrd]
prefix = "2602::/24"
border_relay = "205.171.2.64"
ipv4_mask_len = 0
```

Fields:
- `proto`: dhcp | pppoe | static | dhcpv6 | 6rd | dslite | map-e
- `vlan`, `pcp`, `mtu`, `mac_clone` (bool or prompt)
- `dhcp` {vendorclass, clientid, sendopts, hostname}
- `pppoe` {service, plus username/password prompts}
- `ipv6` {mode, reqprefix, sixrd}
- `prompts[]` (fields the user must fill in) and `checks[]` (extra tests)

## Seed presets

| Preset | Notes |
|---|---|
| `generic-dhcp`, `generic-dhcp-vlan` | Cable and most fiber |
| `generic-pppoe`, `generic-pppoe-vlan` | DSL and many fiber ISPs; MTU 1492, MSS clamping |
| `generic-static`, `generic-6rd`, `generic-dslite` | |
| `us-lumen-ipoe` | VLAN 201 + DHCPv4. IPv6 is mostly 6rd (2602::/24, BR 205.171.2.64, mask 0); some markets have native dual stack. Community-sourced. |
| `us-centurylink-pppoe` | Legacy: VLAN 201 + PPPoE, IPv6 over 6rd as above |

Candidates (shipped only with a verification source): AT&T Fiber (IP passthrough vs 802.1X), Google Fiber, Verizon Fios, Xfinity, Spectrum, Frontier, Ziply, BT/EE, Sky UK, Deutsche Telekom, Bell Canada, Telus, Orange FR.

## Rendering on the gateway

- **VLAN-tagged WAN:** a `config device` of type `8021q` on the WAN port named in `board.json` (vid 201 for Lumen). `interface wan` uses device `wan.201`, and `wan6` runs `dhcpv6` on top.
- **802.1p priority:** `egress_qos_mapping` on the 8021q device, for ISPs that require it.
- **6rd:** `proto 6rd` with `peeraddr`, `ip6prefix`, `ip6prefixlen` and `ip4prefixlen`; tunnel MTU 1480.
- **PPPoE:** `proto pppoe` with credentials from the vault and `ipv6 auto`; fw4 `mtu_fix` handles MSS clamping.
- **MAC clone:** `macaddr` on the WAN device.

Snippets: [openwrt/REFERENCE.md](../openwrt/REFERENCE.md).

## Connectivity test (runs before confirm)

Each step reports live:
1. Link up, then VLAN carrier.
2. DHCP lease or PPP session.
3. Default route; ping the upstream gateway.
4. DNS resolution through 2 resolvers.
5. HTTPS connectivity check.
6. IPv6: prefix delegated or 6rd tunnel up, then ping6.

A failure produces a precise hint, for example "No DHCP offer on VLAN 201. Is the ONT in bridge mode?"

## SQM (bufferbloat) [M4.5]

cake on WAN through sqm-scripts. Autotune runs a speed test from the gateway and sets shaping to 90–95% of the measured rates, then re-checks periodically. The dashboard shows a bufferbloat grade (latency under load).

## Failover and multi-WAN [M4.5, optional]

mwan3 profiles: failover from the primary line to a backup (an LTE/5G modem or USB tethering) with health checks, and a WAN status timeline.

## Monitoring hooks (observability.md)

WAN uptime timeline, ISP outages, latency/jitter/loss to several targets (smokeping-style), public IP changes, DHCP/PPP renewals, speed-test history.
