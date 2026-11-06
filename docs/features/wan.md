# WAN & ISP setup

Read this when working on the gateway's internet connection: ISP presets, WAN types, IPv6, connectivity tests, SQM or failover.
Milestones: M4.1 (presets, WAN, wizard, SQM, failover), M4.3 (first-run).

## Wizard flow

On `/internet` (ui.md, Internet): Country (the site's to start) → ISP (searchable; the generic presets are always listed, under the country's ISPs) → only the fields that preset needs (credentials, static IPs, a VLAN) → **Test** (the connectivity test below; nothing is saved) → **Save** as the site's next revision → **Review and apply**, a transaction that rolls back automatically if the gateway's WAN or DNS health check fails. "Save without testing" skips the test (a gateway in observe mode can't be tested).
An Advanced section exposes every field (how IPv4 and IPv6 arrive, VLAN, priority, MTU, MAC clone, DHCP options, the PPPoE service, the AFTR, the requested prefix, 6rd's values); presets only pre-fill them. The page warns when the gateway doesn't manage `network`: applies would leave its WAN alone.

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

## Connectivity test (runs before saving)

The wizard's **Test** runs the `wan_test` job (`crates/jobs/src/wan_test/`, D-075): it tries the WAN on the gateway (which must be managed) and **always rolls it back**, so nothing is saved until the user chooses to. Only the WAN's sections of the gateway's own `network` change during the trial: interfaces named `wan…`, the devices on the WAN port and, on swconfig, the WAN's switch VLAN. The dead-man timer for uplink changes (300 s) covers it if the controller loses the gateway. `POST /api/v1/wan/test` starts it (admins, audited; 422 with the WAN's issues, 409 while the gateway is in observe mode), and the job's steps report live over the `jobs` topic:

1. Link on the WAN port (or its VLAN device).
2. Address: a DHCP lease, a PPP session, the static address or the DS-Lite tunnel. netifd's `AUTH_FAILED` ends the wait at once.
3. Gateway: a default route, then a ping of the next hop (a gateway that ignores pings doesn't fail it).
4. DNS: `openwrt.org` through the ISP's resolver and 9.9.9.9; one is enough.
5. HTTPS: `wget --spider https://openwrt.org/` (skipped where wget is missing).
6. IPv6: a delegated prefix (DHCPv6) or the tunnel's prefix (6rd), then a ping of 2620:fe::fe.
7. 6rd (with a preset's `auto` only): when DHCPv6 delegated no prefix, a second trial with the preset's 6rd. The wizard saves whichever worked.

The test fails (`no_internet`) when IPv4 doesn't reach the internet (steps 1–5); the failed step's hint is the message, a preset's own (`[hints]`) before the generic one, for example "No DHCP offer on VLAN 201. Is the ONT in transparent bridge mode?". IPv6 failing fails only its step. In demo mode the simulated ISP behaves like Lumen (DHCP on VLAN 201, a /56 by DHCPv6, a 6rd relay at 205.171.2.64; PPPoE finds no server), so a wrong preset fails there too.

## SQM (bufferbloat) [M4.1b] (D-076)

`wan.sqm` turns on cake through sqm-scripts: one queue `wan` on the WAN's layer-3 device (`pppoe-wan` for PPPoE, `ds-wan` for DS-Lite), `piece_of_cake.qos`, per-host fairness behind NAT (`nat dual-dsthost ingress` / `nat dual-srchost`), ECN on ingress, and the link layer's overhead when set (Ethernet on fiber 38, with PPPoE 46, DOCSIS 18). Rates are in kbit/s, 64 kbit/s to 10 Gbit/s. A gateway that lacks sqm-scripts gets it at the next apply (the install step, below); with SQM off, a gateway that holds the package keeps it without a queue and the service off.

**Speed test** (`POST /api/v1/wan/speed-test`, admins, audited; `{"bypass_sqm": true}` needs a managed gateway): the `speed_test` job runs `node-agent/speedtest.sh` on the gateway, sent on stdin once per phase, with stock tools only: idle (10 pings to 1.1.1.1, five a second) → download → upload (four uclient-fetch streams to Cloudflare's speed test for 12 s, pinging once a second) → record. The rate comes from the WAN device's byte counters, the first 2 s left out; latency under load is the 90th percentile, a lost reply counting as 1 s. With `bypass_sqm` SQM stops for each load phase and starts again however the phase ends. Results go to the history (`GET /api/v1/wan/speed-tests?limit=`, the newest 500 kept), with what the newest test of the line suggests. The other devices' traffic counts too: test when the line is quiet.

**Bufferbloat grade** (Waveform's scale) on how much latency grows under load, the worse direction: A+ under 5 ms, A under 30, B under 60, C under 200, D under 400, F beyond.

**Autotune:** "Measure the line" runs a bypassed test and offers 90 % of its download and 95 % of its upload as the rates, saved and applied through review like any change. With `autotune` on, the controller re-measures the line once a week between 03:00 and 06:00 the site's standard time; when the suggestion is more than 10 % away from the shaped rates it raises `sqm_rates_drifted` (category `wan`) and the page offers the new rates. Nothing changes without review.

## Failover [M4.1b] (D-076)

`wan.failover` adds a backup line: a port of the gateway taken out of the LAN (an LTE router, a second ISP; on a VLAN when tagged; not a port of a swconfig switch), or a phone or modem tethered over USB (RNDIS or CDC Ethernet, usually `usb0`; the install step adds the drivers), by DHCP or static. It renders `wanb` (metric 20; `wan` gets 10), adds it to the WAN zone, and mwan3: both lines tracked against the health targets the connectivity test also uses (9.9.9.9, 1.1.1.1) by ping every 5 s, offline after 3 lost and online after 3 answered, members in a `failover` policy for all IPv4 (`last_resort default`: both checks failing keeps the routing table). IPv6 stays on the primary line. An IPv6-only WAN has no failover.

**WAN status timeline:** the Internet page charts the `wan` events (the default route lost and back, address changes) with mwan3track's `is offline` / `is online` lines, which the log patterns `mwan3-offline` and `mwan3-online` make events.

## Install step (D-076)

Render names the feed packages a configuration needs (`Rendered::requires`); the planner compares them with the node's package database and lists the missing ones as the node's `installs`. The apply installs them after planning and before staging (`opkg update` + `opkg install`, or `apk update` + `apk add`; 5 minutes each at most), then stops and disables the services they bring, so the node runs as before until its transaction switches them on. A failed install fails the apply with nothing else changed. A rollback leaves the packages installed with their services off.

## Monitoring hooks (observability.md)

WAN uptime timeline, ISP outages, latency/jitter/loss to several targets (smokeping-style), public IP changes, DHCP/PPP renewals, speed-test history.
