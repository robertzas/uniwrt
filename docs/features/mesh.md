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
- **Make-before-break:** the apply stages each cable. *Overlay:* the port stays a trunk and the VLAN's hard interface joins `bat0` beside it, so the cable carries the LAN whichever end comes over first (BLA handles the two paths). *Exclusive:* once batman-adv sees the neighbor over the VLAN and the MTU probe passes, the port carries the VLAN alone and leaves the bridge. A cable that fails the probe goes back to a plain trunk.
- A node meshing only over cables (no radio for the mesh) still runs `bat0`.

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

Derived health:
- per-link score: TQ or throughput relative to capacity, signal margin, flapping
- hop count to the gateway
- single-point-of-failure detection ("AP3 depends only on AP2")
- on-demand backhaul throughput tests with iperf3 [M8.3]

Visuals: mesh edges colored by link quality, a best-path tree vs an all-links view, link-history sparklines, and a "what if this node goes down" highlight of articulation points.

## Existing meshes (observe mode)

Read-only telemetry works with any existing batman-adv or 802.11s mesh. WDS and relayd setups appear as wired or wireless links where they can be detected. Brownfield import maps an existing batman config into the model when it fits; anything else is kept as raw overrides.

## Packages

`kmod-batman-adv`, `batctl-default` (`batctl-full` for the `batctl tp` throughput meter), full `wpad` (for mesh SAE), and `iperf3` for tests.
