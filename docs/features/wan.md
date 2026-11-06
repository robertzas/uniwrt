# WAN & ISP setup

Read this when working on the gateway's internet connection: ISP presets, WAN types, IPv6, connectivity tests, SQM or failover.
Milestones: M4.1 (presets, WAN, wizard, SQM, failover), M4.3 (first-run).

## Wizard flow

Country → ISP (searchable; "Other / generic" is always available) → connection variant (for example, IPoE vs PPPoE) → only the fields that preset needs (credentials, static IPs) → **Test** → **Apply**. Apply is a transaction that rolls back automatically if the internet health check fails.
An advanced drawer exposes every field (VLAN, priority, MTU, MAC clone, DHCP options, IPv6 mode); presets only pre-fill them.

## Preset format

One file per preset at `crates/isp/data/<country>-<isp>-<variant>.toml`, or `generic-<kind>.toml` (D-025, D-075). `build.rs` embeds every file in the directory, so adding a preset needs no code change; `cargo test -p uniwrt-isp` loads and checks every file: its `id` is its file name, an ISP's preset names its country and cites sources, its prompts cover what it needs, and filled in it passes the site's validation.

```toml
id = "us-lumen-ipoe"
name = "Lumen / Quantum Fiber / CenturyLink Fiber"
variant = "IPoE"
country = "US"                   # unset: a generic preset, offered in every country
status = "community"             # verified | community | unverified
sources = ["<link to the confirmation>"]
notes = "Replaces the ISP router. Put the ONT in transparent bridge mode."
prompts = []                     # what the user fills in, in order

[ipv4]
proto = "dhcp"                   # dhcp | pppoe | static | dslite | none (IPv6 only)
vlan = 201
# pcp = 0                        # 802.1p priority on egress (needs vlan)
# mtu = 1500
# service = "…"                  # PPPoE's service name
# aftr = "…"                     # DS-Lite's AFTR; unset, DHCPv6 announces it

# [ipv4.dhcp]                    # vendor_class, client_id (hex), send_options ["77:…"], hostname

[ipv6]
mode = "auto"                    # off | dhcpv6 | 6rd | auto (DHCPv6-PD, else 6rd)
# reqprefix = 56                 # unset: whatever the ISP delegates

[ipv6.sixrd]
prefix = "2602::/24"
border_relay = "205.171.2.64"
ipv4_mask_len = 0

[hints]                          # per connectivity step: link, address, gateway, dns, https, ipv6
address = "No DHCP offer on VLAN 201. Is the ONT in transparent bridge mode?"
```

- `prompts`: `username`, `password`, `address`, `gateway`, `dns`, `vlan`, `mac` (a MAC to clone), `aftr`, `sixrd`. PPPoE needs `username` and `password`; static needs `address`, `gateway` and `dns`; `6rd` without `[ipv6.sixrd]` needs `sixrd`.
- `auto` (D-075): the connectivity test tries DHCPv6-PD first and, if no prefix arrives, the preset's 6rd; the wizard saves whichever worked, so the site's WAN always holds a concrete mode.
- MAP-E waits for M8.1's install-then-render (it needs the `map` package; D-075).

The site's `WanConfig` (schema v2) holds the result: `proto` (DHCP with its client options, static, PPPoE with its service, DS-Lite with an optional AFTR, or none), `vlan`, `pcp`, `mtu`, `mac`, `ipv6` (`off`, `dhcpv6` with `reqprefix`, or `6rd`), and `preset`, the preset it was last filled from.

## Seed presets

| Preset | Notes |
|---|---|
| `generic-dhcp`, `generic-dhcp-vlan` | Cable and most fiber |
| `generic-pppoe`, `generic-pppoe-vlan` | DSL and many fiber ISPs; MTU 1492, MSS clamping |
| `generic-static`, `generic-6rd`, `generic-dslite` | |
| `us-lumen-ipoe` | VLAN 201 + DHCPv4 (Quantum Fiber and newer CenturyLink Fiber). IPv6 `auto`: native where offered, else 6rd (2602::/24, BR 205.171.2.64, mask 0). Community-sourced. |
| `us-centurylink-pppoe` | Legacy: VLAN 201 + PPPoE, IPv6 over 6rd as above |

Candidates (shipped only with a verification source): AT&T Fiber (IP passthrough vs 802.1X), Google Fiber, Verizon Fios, Xfinity, Spectrum, Frontier, Ziply, BT/EE, Sky UK, Deutsche Telekom, Bell Canada, Telus, Orange FR.

## Rendering on the gateway

`crates/render/src/wan.rs`; the delegated prefix always arrives on `wan6`.
- **VLAN-tagged WAN:** a `config device` of type `8021q` on the WAN port named in `board.json` (vid 201 for Lumen). `interface wan` uses device `wan.201`. On a swconfig switch the switch tags the VLAN on the CPU port's `<cpu>.<vid>`.
- **802.1p priority:** `list egress_qos_mapping '<prio>:<pcp>'` for every skb priority 0–7 on the 8021q device (a warning on swconfig, where the switch tags).
- **MTU and MAC clone:** `mtu` and `macaddr` on the WAN device; PPPoE takes its MTU on the interface (pppd).
- **DHCP client options:** `vendorid`, `clientid`, `sendopts` (space-separated) and `hostname` on `interface wan`.
- **PPPoE:** `proto pppoe` with credentials from the vault, `service`, and `ipv6 '1'`; `wan6` runs `dhcpv6` on `@wan` (so `reqprefix` applies); fw4 `mtu_fix` handles MSS clamping.
- **DHCPv6:** `wan6` with `proto dhcpv6` on the WAN device and `reqprefix`.
- **6rd:** `wan6` with `proto 6rd`, `peeraddr`, `ip6prefix`, `ip6prefixlen`, `ip4prefixlen`, `tunlink 'wan'` and the WAN's MTU less 20 (1480).
- **DS-Lite:** `wan6` by DHCPv6 on the WAN device; with an AFTR, `interface wan` is `proto dslite` (`peeraddr`, `tunlink 'wan6'`, `encaplimit 'ignore'`); without one, `wan6` sets `iface_dslite 'wan'` and `zone_dslite 'wan'` and netifd adds `wan` when DHCPv6 announces the AFTR.
- **IPv6 only:** `wan6` alone.

Import reads all of these back, and a 6rd interface under another name (`wan6rd`) when there is no `wan6`, renamed to `wan6`.

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

## SQM (bufferbloat) [M4.1]

cake on WAN through sqm-scripts. Autotune runs a speed test from the gateway and sets shaping to 90–95% of the measured rates, then re-checks periodically. The dashboard shows a bufferbloat grade (latency under load).

## Failover and multi-WAN [M4.1, optional]

mwan3 profiles: failover from the primary line to a backup (an LTE/5G modem or USB tethering) with health checks, and a WAN status timeline.

## Monitoring hooks (observability.md)

WAN uptime timeline, ISP outages, latency/jitter/loss to several targets (smokeping-style), public IP changes, DHCP/PPP renewals, speed-test history.
