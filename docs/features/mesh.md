# Mesh backhaul (batman-adv)

Read this when working on mesh configuration, backhaul selection, mesh telemetry or mesh health.
Milestones: M2 (read-only telemetry for existing meshes), M5.3 (managed mesh).

## Design (D-013)

- **A layer-2 mesh built on batman-adv.** It runs over **802.11s** mesh-point interfaces (`mode mesh`, `mesh_fwding 0` so batman does the routing, encrypted with SAE using a controller-generated key). It also runs over **wired uplinks** (Ethernet hard interfaces) wherever an MTU of at least 1532 is possible. The result is automatic failover between wired and wireless links, without loops.
- **Fallback** when a wired link can't take MTU 1532: bridge it as a plain trunk and enable **Bridge Loop Avoidance**.
- **Wired backhaul** (D-094): see below.
- **Every network spans the mesh:** `bat0` is a trunk of `br-lan`, carrying the untagged network untagged and the VLANs tagged (D-067).
- **Gateway:** batman `gw_mode server`, with bandwidth advertised from the WAN speed. APs use `gw_mode client` only when there are several gateways; otherwise it is off. DHCP stays central on the gateway.
- **Routing algorithm:** BATMAN_V for new sites (the throughput metric, D-095); BATMAN_IV stays selectable, and stored sites keep theirs.
- **Rendered options:** `bridge_loop_avoidance 1`, `distributed_arp_table 1`, `multicast_mode 1`, `hop_penalty` (higher on wireless), `orig_interval 1000`, `aggregated_ogms 1`, `fragmentation 1`, and `ap_isolation` on guest networks.

## Wired backhaul (M5.3, D-094)

- **Which cables:** a direct cable between two adopted, managed nodes that the topology sees (no client or switch on it), on a trunk port of each, unless the site turns it off (`mesh.wired`) or a port's profile opts out (`backhaul: false`). A swconfig switch's ports can't carry it yet.
- **How:** batman-adv runs over a VLAN of its own on the cable, `mesh.wired_vlan` (4094 by default; no network may use it): `<port>.<vid>` is the hard interface `uw_bh_<port>`, and the port and the VLAN device take MTU 1532.
- **Which cables, in the apply** (D-097; `uniwrt_jobs::apply::cables`): the topology's Ethernet links between two managed nodes in the apply (batman-adv's link on `<port>.<vid>` counts as the port's), each port on one such link only, both ports accepted by render (`uniwrt_render::backhaul_ports`, the same checks render refuses with), and not a cable that failed the probe. What a node holds counts too (`uniwrt_render::held_backhauls`: a `uw_bh_<port>` interface, an overlay while the port is still in the bridge): a cable held alone at both ends stays so, unprobed; a port held alone whose far end isn't seen stays so (the far end may be away, and a trunk would cut it off); an overlay without a cable seen, or a port render no longer accepts (wired links off, the port opted out), goes back to a trunk, and the review says so. Brownfield links that run batman-adv on the bare port are raw interfaces that claim the port, so the apply leaves them as they are (owner's choice).
- **Make-before-break:** the apply stages each cable. *Overlay:* the port stays a trunk and the VLAN's hard interface joins `bat0` beside it, so the cable carries the LAN whichever end comes over first. On a node without a mesh radio, `bat0` stays out of the bridge while all its cables are overlays, so the trunk alone carries the LAN (D-098). *Exclusive:* once batman-adv sees the neighbor over the VLAN and the MTU probe passes, the port carries the VLAN alone and leaves the bridge (and `bat0` joins it): about 4 s without LAN between the two nodes. A cable that fails the probe goes back to a plain trunk.
- **Between two nodes that mesh over Wi-Fi too** (D-098): their `bat0` is bridged at every stage, so the overlay joins their LANs twice, and batman-adv's bridge loop avoidance holds each as a backbone gateway of the other's LAN until that times out. When the cable goes exclusive, traffic between the two nodes' networks pauses for about a minute (lab: ~50 s), inside the apply's verify windows; the review's note on the cable says so. Clearing it on the node costs as much (re-creating BLA's own backbone entry drops multicast for 30 s by design).
- **The backhaul round** (D-097): after the first transaction (and the gateway's Settle and WAN), the apply's **Backhaul** step waits, up to the verify window, for batman-adv at each end to hear a neighbor on `<port>.<vid>`, then runs the probe from both ends. A pass makes the cable exclusive; a failure (no neighbor heard, a device below 1532, no answer at 1532) sends it back to a trunk and is **remembered** in the store (`backhaul_failures`, both ends and why): later applies leave that cable a trunk while the same two ports face each other (owner's choice; a new port pair is tried). A probe that can't be made (a login lost, the agent not answering) keeps the overlay, and the next apply probes it again. Every node with a probed cable then applies in one transaction (`uniwrt-<job>-backhaul`); a failure there rolls back that transaction alone and ends the apply partial (D-081).
- **The probe** (the agent's `mtu` op, D-096): BusyBox's ping of IPv6's all-nodes group on the VLAN device, from its link-local address, with packets of 1532 bytes; it answers the device's MTU and the link-local addresses that replied at that size. A device below 1532 isn't pinged (the kernel would fragment the requests, and they would pass). A pass shows the peer receives 1532 bytes from this end (its reply may come back fragmented), so the apply probes from both ends.
- **Firewall:** on the gateway the backhauls' interfaces form a zone of their own, `uw_bh` (input and forward rejected, no forwardings), with one rule accepting the probe's ICMPv6 (echo request and reply, neighbour solicitation and advertisement; conntrack doesn't tie a reply to a request sent to a group). batman-adv's frames aren't IP, so the firewall never sees them. APs run no firewall.
- A node meshing only over cables (no radio for the mesh) still runs `bat0`.
- **Demo:** the sim's ap-living and ap-office share a plain cable (`eth1` to `eth0`, bridged at both ends): managing both and applying brings it over, probe and all (`crates/server/src/demo_backhaul_tests.rs`). The gateway's link to ap-living runs batman-adv on the bare ports, so it is brownfield and stays as it is.

```
config device
	option name 'lan1'
	option mtu '1532'

config device
	option type '8021q'
	option ifname 'lan1'
	option vid '4094'
	option name 'lan1.4094'
	option mtu '1532'

config interface 'uw_bh_lan1'
	option proto 'batadv_hardif'
	option master 'bat0'
	option device 'lan1.4094'

# firewall, on the gateway
config zone
	option name 'uw_bh'
	list network 'uw_bh_lan1'
	option input 'REJECT'
	option output 'ACCEPT'
	option forward 'REJECT'

config rule
	option name 'Allow-uw_bh-MTU-probe'
	option src 'uw_bh'
	option proto 'icmp'
	list icmp_type 'echo-request'
	list icmp_type 'echo-reply'
	list icmp_type 'neighbour-solicitation'
	list icmp_type 'neighbour-advertisement'
	option family 'ipv6'
	option target 'ACCEPT'
```

## Backhaul radio selection

- **Tri-band nodes:** a dedicated 5 GHz (high band) radio carries the mesh, and the AP uses the others.
- **Dual-band nodes:** the mesh shares the 5 GHz radio with the AP, so the planner treats the mesh as one channel group. The UI recommends wired backhaul or tri-band hardware where airtime is tight. A 2.4 GHz mesh is an optional last resort.
- **6 GHz backhaul** when every peer supports it and the regulatory domain allows it (the cleanest spectrum).
- `mesh_rssi_threshold` (for example, −80 dBm) avoids weak peer links.

## Rendering sketch (one AP)

```
config wifi-iface 'uw_mesh_5g'
	option device 'radio1'
	option mode 'mesh'
	option mesh_id 'uw-<siteid>'
	option encryption 'sae'
	option key '<from vault>'
	option mesh_fwding '0'
	option mesh_rssi_threshold '-80'
	option network 'uw_mesh5'

config interface 'uw_mesh5'
	option proto 'batadv_hardif'
	option master 'bat0'
	option mtu '1532'

config interface 'uw_uplink'
	option proto 'batadv_hardif'
	option master 'bat0'
	option device 'wan'

config interface 'bat0'
	option proto 'batadv'
	option routing_algo 'BATMAN_IV'
	option bridge_loop_avoidance '1'
	option distributed_arp_table '1'
	option gw_mode 'off'
```

`bat0` and the LAN ports join `br-lan` as trunks. Exact option names are verified against each target OpenWrt version in the system harness (M3.3b).

What M3.3 renders (`crates/render`, D-067): `bat0` as above (`gw_mode server` on the gateway, `off` on APs; `ap_isolation` when the untagged network isolates), the `uw_mesh` hard interface (MTU 1532), and the `uw_mesh` mesh point on the mesh radio, which needs batman-adv and hostapd's 802.11s (`mesh`) on the node. The mesh's band is the configured one, else 5 GHz; a node with two radios of that band gives the mesh the one that reaches the highest channel and keeps its WLANs off it. Every peer needs one channel, so the mesh radio takes the band's fixed channel, else a default (1, 36 or 37) with a warning; a radio override can't move it. Wired links stay plain trunks with Bridge Loop Avoidance unless the apply stages them as a wired backhaul (above, M5.3); backhaul radio selection and key rotation are not rendered yet.

## Mesh lifecycle operations

- **Join:** a new node receives mesh credentials during adoption on the bench, then can be placed wirelessly.
- **Credential rotation (job):** two phases. First, add a second mesh interface with the new key on every node and wait for links on it. Then remove the old one.
- **Channel change:** synchronized cutover (see wifi.md).
- **Node removal:** drain, then remove; batman purges the originator.

## Telemetry & health

From the agent (the `batman` set: batctl's JSON queries `mj`, `hj`, `oj`, `nj`, `gwj`, `tgj` and `tlj`, D-035; the `stations` set for mesh peers):
- `batctl o`: originators, last seen, best next hop, TQ or throughput
- `batctl n`: neighbors per hard interface
- `batctl tg`: the global translation table (client MAC → originator node), which shows which node each wired or wireless client sits behind
- `batctl gwl` (gateways) and `batctl s` (counters)
- `iw dev <mesh> station dump`: signal, tx bitrate, expected throughput, peer-link state

Derived health (`uniwrt-topo`'s `health/` and `LinkWatch`, D-099; served in `GET v1/topology` and the WS topic `topology`):
- **per-link score** (`TopoEdge.score`, 0–100, with its `weakest` factor) for every backhaul link (between nodes, switches or peers): the lowest of down (0), flapping (≥ 2 drops in 30 min: −30 per drop after the first), the weaker end's signal (over the air), batman-adv's TQ, and throughput (log scale, 10 Mbit/s → 0, 300 → 100; a cable only below 100 Mbit/s). `drops` lists when the link went down or vanished while both ends were online, within the window; the engine's `LinkWatch` keeps them across builds (in memory)
- **hops to the gateway** (`TopoNode.gateway_hops`, and `wireless_hops` of them over the air) along the tree view, a cable through an unmanaged switch counting once
- **single points of failure** ("AP3 depends only on AP2"): dominators from the gateways over the links that are up give each vertex's `depends_on` (the nearest node, switch or peer it can't lose) and each `critical` link (one some node, switch or peer can't lose); "what if X goes down" is every vertex whose `depends_on` chain reaches X
- on-demand backhaul throughput tests with iperf3 [M8.3]

Visuals (M5.3 health c1, observability.md's Topology): links colored by their score, a best-path tree vs an all-links view, the details panel's health rows, **Weak spots** (the single points of failure and critical links) and **Show what goes down** (a node's loss and what it cuts off). Still to come: the device page's mesh card with link-history sparklines (health c2).

## Existing meshes (observe mode)

Read-only telemetry works with any existing batman-adv or 802.11s mesh. WDS and relayd setups appear as wired or wireless links where they can be detected. Brownfield import maps an existing batman config into the model when it fits; anything else is kept as raw overrides.

## Packages

`kmod-batman-adv`, `batctl-default` (`batctl-full` for the `batctl tp` throughput meter), full `wpad` (for mesh SAE), and `iperf3` for tests.
