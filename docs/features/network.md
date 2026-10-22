# Networks, addressing & gateway services

Read this when working on IPAM, networks/VLANs, DHCP/DNS, IPv6, the firewall, VPN or other gateway services.
Milestones: M4.2 (IPAM, networks), M4.3 (firewall), M8.1–M8.3 (services).

## Address plan (IPAM)

The site address space defaults to **10.0.0.0/16** and is configurable at first run (any RFC 1918 range, plus a ULA for IPv6).
How the space is carved is open decision O-1. The default gives each network a /24 whose third octet is the VLAN ID:

| Network | VLAN | Subnet | Gateway | Notes |
|---|---|---|---|---|
| Default (LAN) | untagged | 10.0.0.0/24 | 10.0.0.1 | .2–.31 infrastructure (nodes, controller) · .32–.99 static reservations · .100–.249 DHCP pool |
| IoT | 30 | 10.0.30.0/24 | 10.0.30.1 | Internet allowed; blocked from the LAN |
| Guest | 40 | 10.0.40.0/24 | 10.0.40.1 | Internet only; client isolation |
| Management (optional) | 99 | 10.0.99.0/24 | 10.0.99.1 | Moves nodes off the user LAN |

IPAM guarantees:
- No overlapping subnets; gateways and pools sit inside their subnet; reservations are unique.
- No collision with the WAN side, including CGNAT 100.64.0.0/10 and ISP modem subnets.
- 192.168.1.0/24 stays free for adopting factory-default devices.

## Networks

Fields:
- name; purpose (default | iot | guest | mgmt | custom); VLAN
- IPv4: subnet, DHCP pool, lease time, options, reservations
- IPv6: off | ULA | delegated (with a hint) | both; RA and DHCPv6 modes
- inter-network policy (see Firewall) and client isolation
- mDNS reflection membership and IGMP snooping
- which nodes and ports carry the network

Rendering:
- **Gateway:** one static `interface` per network; `bridge-vlan` on a DSA bridge or `switch_vlan` on swconfig; dnsmasq `dhcp` sections; odhcpd for IPv6.
- **APs:** layer 2 only. A bridge per VLAN, tagged over the uplink or mesh (batman carries `bat0.<vid>`). Management uses a DHCP client on the default network.

## DHCP & local DNS

- dnsmasq on the gateway: pools, reservations (created with "Fix IP" on the Clients page) and options (NTP, DNS, domain, PXE). Lease events stream to the controller and feed the client inventory.
- Local DNS: `<hostname>.lan` is automatic, plus custom A/AAAA/CNAME records. `uniwrt.lan` always points at the controller.
- Upstream DNS [M8.1]: the ISP's, custom servers, or DoH/DoT (https-dns-proxy or stubby). Per-network DNS overrides. Blocklists (ads, trackers, malware) compiled into dnsmasq, with per-client bypass and query statistics.

## IPv6

- The gateway gets a delegated prefix (DHCPv6-PD) or uses 6rd, then hands out a /64 per network (`ip6assign`, with `ip6hint` set to the VLAN ID in hex).
- An `fdxx::/48` ULA is always present for stable local addressing.
- RA/SLAAC by default; stateful DHCPv6 optional.
- The IPv6 firewall mirrors the IPv4 policy.

## Firewall policy [M4.3]

- One zone per network, plus wan.
- Inter-network policy is a matrix (from → to: allow | block | allow-established), seeded by purpose:
  - LAN → everything: allow.
  - IoT → LAN: block, except established. IoT → internet: allow.
  - Guest → internet only; Guest → gateway: DHCP and DNS only.
- Port forwards (DNAT) with NAT reflection, a DMZ host, and UPnP/NAT-PMP (miniupnpd) with a live list of mappings.
- Custom traffic rules (protocols, ports, IP sets), time-based rules for parental schedules [M8.3], and IP/geo blocklists using nft sets (backlog).
- Rendering: fw4 UCI (`zone`, `forwarding`, `rule`, `redirect`, `ipset`), validated with `fw4 check` in the system harness.

## VPN [M8.2]

- A WireGuard server on the gateway. Peers get auto-assigned addresses (default 10.0.250.0/24), a QR code or `.conf` download, and full or split tunnel routes.
- The DDNS hostname is the endpoint. Handshake and traffic stats are shown per peer.
- Site-to-site peers; optional management of the Tailscale package.

## Other services [M8.1–M8.3]

- DDNS (ddns-scripts providers) and an NTP server on the gateway.
- mDNS reflection across selected networks; IGMP snooping and multicast-to-unicast.
- Wake-on-LAN, sent from whichever node shares the client's segment.
- Scheduled reboots and LED schedules (LEDs off at night).
- A per-node package manager UI (install, remove, upgrade). Packages required by the node's role are locked.
