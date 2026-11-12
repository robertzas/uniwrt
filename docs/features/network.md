# Networks, addressing & gateway services

Read this when working on IPAM, networks/VLANs, DHCP/DNS, IPv6, the firewall, VPN or other gateway services.
Milestones: M4.2 (IPAM, networks, firewall), M8.1 (services), M8.2 (VPN).

## Address plan (IPAM)

The site address space defaults to **10.0.0.0/16** and is configurable at first run (any RFC 1918 range, plus a ULA for IPv6).
Each network gets a /24 whose third octet is its VLAN ID (D-077); a VLAN with no third octet in the space takes the first free /24. The suggestions (`uniwrt_model::site::ipam`, `GET v1/ipam/network`, `GET v1/ipam/reservation`) only propose: any subnet that validates may be typed.

| Network | VLAN | Subnet | Gateway | Notes |
|---|---|---|---|---|
| Default (LAN) | untagged | 10.0.0.0/24 | 10.0.0.1 | .2–.31 infrastructure (nodes, controller) · .32–.99 static reservations · .100–.249 DHCP pool |
| IoT | 30 | 10.0.30.0/24 | 10.0.30.1 | Internet allowed; blocked from the LAN |
| Guest | 40 | 10.0.40.0/24 | 10.0.40.1 | Internet only; client isolation |
| Management (optional) | 99 | 10.0.99.0/24 | 10.0.99.1 | Moves nodes off the user LAN |

Usual VLANs: IoT 30, Guest 40, Management 99, others 50, 60, … (the next free one when taken). "Fix IP" keeps the client's current address when no reservation has it, else takes the first free one of .32–.99.

IPAM guarantees:
- No overlapping subnets; gateways and pools sit inside their subnet; reservations are unique (validation).
- No collision with the WAN side: CGNAT 100.64.0.0/10 and a static WAN's subnet (validation), and the subnet the gateway's uplinks hold now, such as an ISP modem's (render refuses the plan).
- 192.168.1.0/24 stays free for adopting factory-default devices (a warning).

## Networks

Fields:
- name; purpose (default | iot | guest | mgmt | custom); VLAN
- IPv4: subnet, DHCP pool, lease time, reservations, options (DNS and NTP servers, domain, PXE boot, custom `<code>,<value>`)
- IPv6: off | ULA | delegated | both (the hint is the VLAN ID); how clients get addresses: SLAAC only, SLAAC with stateless DHCPv6 (default), SLAAC and stateful DHCPv6 (as OpenWrt ships), stateful DHCPv6 only (Android gets no address)
- inter-network policy (see Firewall) and client isolation
- mDNS reflection membership and IGMP snooping

Port profiles (per node, D-077): an Ethernet port carries one network untagged (its native network, or none) and tagged ones (all with a VLAN, none, or a list), and says whether the mesh's wired backhaul may take it over (`backhaul`, default yes; mesh.md, Wired backhaul). A port without a profile is a trunk: the untagged network untagged, every VLAN tagged. Ports are named by device (`lan2`, `eth1`) or, on a swconfig switch, by number (`switch0:3`). The untagged network is never tagged.

Rendering:
- **Every node [M3.3]:** one bridge, `br-lan`, carries every network (D-067). With VLAN networks it filters VLANs: a `bridge-vlan` per network (the untagged network gets the lowest free VLAN ID inside the bridge), each network's interface on `br-lan.<vid>`, Ethernet ports as their profiles say (trunks by default), `bat0` always a trunk, Wi-Fi joining through its interface. A swconfig switch gets a `switch_vlan` per network, tagged on the CPU port, its user ports as their profiles say, and the CPU port's `<cpu>.<vid>` devices join the bridge as access ports. A port that carries nothing leaves the bridge (on a switch, every VLAN). Profiles naming a port the node lacks, its WAN port or one the backup line or a raw interface takes are left out with a warning; on an AP, a linked port that would drop the node's own network warns (the apply's rollback catches a lost uplink).
- **Gateway:** one static `interface` per network (`list ipaddr`, `ip6assign 64`, `ip6hint` = VLAN ID in hex, `ip6class` `local` for ULA-only or the delegating interface for delegated-only); dnsmasq `dhcp` sections with the pool as `start`/`limit` and the options as `list dhcp_option` (6, 42, 15, then custom), PXE as a `boot` section keyed by `networkid`; odhcpd per the clients mode (`dhcpv6`, `ra_slaac`, `ra_flags`); reservations as `host`, records as `domain` and `cname`.
- **APs:** layer 2 only. Every port is bridged, the WAN port too. Management uses a DHCP client on the management network, else the default one; the other networks are `proto none`.
- Bridge-wide in v1: IGMP snooping is on when any network asks for it. Client isolation renders on the network's WLANs (`isolate`) and, for the untagged network, as batman's `ap_isolation`; wired isolation and per-VLAN batman isolation come later. mDNS reflection needs M8.1's services.

## DHCP & local DNS

- dnsmasq on the gateway: pools, reservations (created with "Fix IP" on the Clients page) and options (NTP, DNS, domain, PXE). Lease events stream to the controller and feed the client inventory.
- Local DNS: `<hostname>.lan` is automatic, plus custom A/AAAA/CNAME records. `uniwrt.<domain>` points at the controller: an apply reads the controller's address from the gateway's `SSH_CONNECTION` and renders a `domain` record `uniwrt` when that address sits in one of the networks; a custom record can't take the name.
- Upstream DNS [M8.1]: the ISP's, custom servers, or DoH/DoT (https-dns-proxy or stubby). Per-network DNS overrides. Blocklists (ads, trackers, malware) compiled into dnsmasq, with per-client bypass and query statistics.

## IPv6

- The gateway gets a delegated prefix (DHCPv6-PD) or uses 6rd, then hands out a /64 per network (`ip6assign`, with `ip6hint` set to the VLAN ID in hex).
- An `fdxx::/48` ULA is always present for stable local addressing.
- RA/SLAAC by default; stateful DHCPv6 optional.
- The IPv6 firewall mirrors the IPv4 policy.

## Firewall policy [M4.2] (D-079)

- One zone per network, plus wan.
- Inter-network policy is a matrix (from → to: allow | block), seeded by purpose; `firewall.policies` override cells. Replies to connections the other side opened always pass (fw4 accepts established traffic in every direction), so a blocked cell whose reverse is allowed is "replies only"; the model's `established` action renders as block.
  - LAN and Management → everything: allow.
  - IoT → LAN: block (replies pass). IoT → internet: allow.
  - Guest → internet only; Guest → gateway: DHCP and DNS only (the zones that don't reach the gateway get `Allow-<zone>-DHCP`/`-DNS`, and with IPv6 `-DHCPv6`/`-ND`).
- **Port forwards** (`firewall.forwards`, DNAT): a unique name (not `DMZ`), on or off, TCP, UDP or both, a WAN port or range, the host (an address in a network's subnet, not the gateway's, the subnet's or its broadcast) and its first port when it differs (a range keeps its length), an optional source subnet, and NAT reflection (on by default: hosts on the networks reach the forward through the WAN's address too). Enabled forwards may not share a port and protocol. Forwards behind DS-Lite or an IPv6-only WAN warn: the internet can't reach them over IPv4.
- **DMZ host** (`firewall.dmz`): every connection from the internet that no forward claims goes to one host.
- **UPnP IGD and NAT-PMP/PCP** (`firewall.upnp`, miniupnpd): on for the main LAN on new sites (off on migrated ones, as before), for the networks chosen. Secure mode (a client maps ports only to itself), ports from 1024 only (OpenWrt's stock permissions). The live list of mappings comes from miniupnpd's lease file with the gateway's DHCP leases (the `leases` set, every minute).
- Custom traffic rules (protocols, ports, IP sets) and time-based rules for parental schedules [M8.1]; IP/geo blocklists using nft sets (backlog). IPv6 "forwards" (opening a host's port to the internet) come with custom rules.
- Rendering: fw4 UCI (`zone`, `forwarding`, `rule`, `redirect`; `ipset` later), validated with `fw4 check` in the system harness. Each forward is a `redirect` from `wan` to the zone of the network whose subnet holds the host, with `target`, `dest_port` and `proto` always written; the DMZ is the last redirect (`name 'DMZ'`, `proto 'all'`, as LuCI writes it). UPnP is the `upnpd` package (miniupnpd's stock options and permission rules, `internal_iface` naming the networks), which needs `miniupnpd-nftables` (installed by the apply, D-076) and carries over the `uuid` miniupnpd writes on its first start; untrusted zones it serves get `Allow-<zone>-UPnP` (UDP 1900 and 5351) and `Allow-<zone>-UPnP-HTTP` (TCP 5000).
- Import: named DNAT redirects from `wan` to a host on a network become forwards (`DMZ` without ports the DMZ host); unnamed ones, ranges that change length and the like stay raw sections. miniupnpd's package becomes `upnp` (on unless `enabled '0'`; off without the package).
- UI (M4.2b2): the `/security` page. **The matrix** has a row per network and a column per other network plus Internet (the internet's row stays out: forwards are the way in); a blocked cell whose reverse is allowed reads "Replies only", and cells that differ from the purpose's seed are marked. Admins toggle cells and save them as one revision: cells left as they were keep their policies (an `established` one too), changed cells get a policy only where they differ from the seed, and cells of a network another save added stay. **Forwards** open in a dialog that checks what the server checks (names, ports, the host on a network, clashing enabled forwards, the source as an address or subnet, a bare address becoming a /32) and suggests the clients' addresses; forwards are keyed by name across revisions. **The DMZ host** is set or removed inline. **UPnP**: on or off and its networks (those with IPv4), a hint while the gateway doesn't manage `upnpd`, and the open ports with the client's name. A network can't be deleted while UPnP, a forward or the DMZ host uses it; deleting one drops the policies naming it.

## VPN [M8.2]

- A WireGuard server on the gateway. Peers get auto-assigned addresses (default 10.0.250.0/24), a QR code or `.conf` download, and full or split tunnel routes.
- The DDNS hostname is the endpoint. Handshake and traffic stats are shown per peer.
- Site-to-site peers; optional management of the Tailscale package.

## Other services [M8.1]

- DDNS (ddns-scripts providers) and an NTP server on the gateway.
- mDNS reflection across selected networks; IGMP snooping and multicast-to-unicast.
- Wake-on-LAN, sent from whichever node shares the client's segment.
- Scheduled reboots and LED schedules (LEDs off at night).
- A per-node package manager UI (install, remove, upgrade). Packages required by the node's role are locked.
