//! The cables between nodes other than the gateway's batman-adv uplink ([`Node::wired`]): plain
//! Ethernet that each end bridges into its LAN until the controller brings it over to the wired
//! backhaul (features/mesh.md, Wired backhaul; D-094). Each end's `/etc/config/network` decides
//! what its port does ([`Wiring`], which the fleet reads from the node's disk every second), so a
//! cable carries the LAN while both ends bridge it, and batman-adv while both run a hard
//! interface on the same VLAN of it. A VLAN device takes at most its port's MTU, and a probe
//! (`mtu`) is answered by the far end's VLAN device when both take the probe's size.

use std::{
    collections::{BTreeMap, BTreeSet},
    net::Ipv6Addr,
};

use uniwrt_core::MacAddr;

use super::{Node, World};

/// A direct cable: each end's node and port.
pub(crate) struct Cable {
    pub(crate) ends: [(usize, &'static str); 2],
}

/// batman-adv's hard interface on a VLAN of a port.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct VlanHardif {
    /// The interface's name in `network`.
    pub(crate) name: String,
    pub(crate) port: String,
    pub(crate) vid: u16,
}

impl VlanHardif {
    pub(crate) fn dev(&self) -> String {
        format!("{}.{}", self.port, self.vid)
    }
}

/// What a node's `network` does with its ports.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Wiring {
    /// The ports of `br-lan`.
    pub(crate) bridged: BTreeSet<String>,
    /// batman-adv's hard interfaces on VLAN devices (`<port>.<vid>`).
    pub(crate) vlans: Vec<VlanHardif>,
    /// The MTUs `config device` sections set, by device name.
    pub(crate) mtus: BTreeMap<String, u32>,
}

impl Wiring {
    /// The wiring `network` describes; none if it doesn't parse.
    pub(crate) fn of(network: Option<&str>) -> Self {
        let Some(package) = network.and_then(|text| uniwrt_uci::parse("network", text).ok()) else {
            return Self::default();
        };
        let mut wiring = Self::default();
        for device in package.sections_of("device") {
            let Some(name) = device.option("name") else {
                continue;
            };
            if name == "br-lan" {
                let ports = device.get("ports").map(uniwrt_uci::Value::items);
                wiring.bridged = ports.unwrap_or_default().iter().cloned().collect();
            }
            if let Some(mtu) = device.option("mtu").and_then(|mtu| mtu.parse().ok()) {
                wiring.mtus.insert(name.to_owned(), mtu);
            }
        }
        for iface in package.sections_of("interface") {
            if iface.option("proto") != Some("batadv_hardif") {
                continue;
            }
            let vlan = iface
                .option("device")
                .and_then(|dev| dev.rsplit_once('.'))
                .and_then(|(port, vid)| Some((port, vid.parse().ok()?)));
            if let (Some(name), Some((port, vid))) = (iface.name.as_deref(), vlan) {
                wiring.vlans.push(VlanHardif {
                    name: name.to_owned(),
                    port: port.to_owned(),
                    vid,
                });
            }
        }
        wiring.vlans.sort();
        wiring
    }

    fn mtu_of(&self, dev: &str) -> u32 {
        self.mtus.get(dev).copied().unwrap_or(1500)
    }

    /// A device's MTU: a port's as set, a VLAN device's at most its port's; `None` for a VLAN
    /// device this node has no hard interface on.
    pub(crate) fn mtu(&self, dev: &str) -> Option<u32> {
        match self.vlans.iter().find(|vlan| vlan.dev() == dev) {
            Some(vlan) => Some(self.mtu_of(dev).min(self.mtu_of(&vlan.port))),
            None if dev.contains('.') => None,
            None => Some(self.mtu_of(dev)),
        }
    }
}

impl Node {
    /// The address of one of its ports: a DSA port and an access point's LAN port share the
    /// label MAC, an access point's WAN port has its own (a VLAN device takes its port's).
    pub(crate) fn port_mac(&self, port: &str) -> MacAddr {
        if !self.gateway && port == self.model.wan_port {
            self.mac_at(1)
        } else {
            self.mac
        }
    }
}

/// IPv6's link-local address of `mac` (modified EUI-64), as the kernel gives a device.
pub(crate) fn link_local(mac: MacAddr) -> Ipv6Addr {
    let o = mac.octets();
    Ipv6Addr::new(
        0xfe80,
        0,
        0,
        0,
        u16::from_be_bytes([o[0] ^ 0x02, o[1]]),
        u16::from_be_bytes([o[2], 0xff]),
        u16::from_be_bytes([0xfe, o[3]]),
        u16::from_be_bytes([o[4], o[5]]),
    )
}

/// A cable's batman-adv link seen from one end: the VLAN device, the far end and its address.
pub(crate) struct CableLink {
    pub(crate) dev: String,
    pub(crate) to: usize,
    pub(crate) neigh: MacAddr,
}

impl World {
    /// The far end of the cable on node `n`'s `port`.
    pub(crate) fn cable_at(&self, n: usize, port: &str) -> Option<(usize, &'static str)> {
        self.cables.iter().find_map(|cable| match cable.ends {
            [(a, pa), far] | [far, (a, pa)] if a == n && pa == port => Some(far),
            _ => None,
        })
    }

    /// The node at the far end of `port`, while both ends are up and bridge the cable into
    /// their LANs.
    pub(crate) fn bridged_peer(&self, n: usize, port: &str) -> Option<usize> {
        let (to, far) = self.cable_at(n, port)?;
        let both = self.nodes[n].up() && self.nodes[to].up();
        let bridged = |at: usize, port: &str| self.nodes[at].wiring.bridged.contains(port);
        (both && bridged(n, port) && bridged(to, far)).then_some(to)
    }

    /// Node `n`'s batman-adv links over cables: a hard interface on a VLAN of the port at each
    /// end, the same VLAN, both up.
    pub(crate) fn cable_links(&self, n: usize) -> Vec<CableLink> {
        let node = &self.nodes[n];
        if !node.up() {
            return Vec::new();
        }
        node.wiring
            .vlans
            .iter()
            .filter_map(|vlan| {
                let (to, far) = self.cable_at(n, &vlan.port)?;
                let peer = &self.nodes[to];
                let heard = peer.up()
                    && peer
                        .wiring
                        .vlans
                        .iter()
                        .any(|other| other.port == far && other.vid == vlan.vid);
                heard.then(|| CableLink {
                    dev: vlan.dev(),
                    to,
                    neigh: peer.port_mac(far),
                })
            })
            .collect()
    }

    /// The `mtu` op's probe from node `n`'s `dev` with packets of `size` bytes: the device's
    /// MTU and the link-local addresses that answer (none when the device takes less); `None`
    /// for a device the node doesn't have.
    pub(crate) fn probe(&self, n: usize, dev: &str, size: u16) -> Option<(u32, Vec<Ipv6Addr>)> {
        let node = &self.nodes[n];
        let mtu = node.wiring.mtu(dev)?;
        let size = u32::from(size);
        if mtu < size {
            return Some((mtu, Vec::new()));
        }
        let peers = dev
            .rsplit_once('.')
            .and_then(|(port, vid)| {
                let (to, far) = self.cable_at(n, port)?;
                let peer = &self.nodes[to];
                let far_dev = format!("{far}.{vid}");
                let takes = peer.wiring.mtu(&far_dev)? >= size;
                (node.up() && peer.up() && takes).then(|| link_local(peer.port_mac(far)))
            })
            .into_iter()
            .collect();
        Some((mtu, peers))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OVERLAY: &str = "
config device
	option name 'br-lan'
	option type 'bridge'
	list ports 'eth1'
	list ports 'bat0'

config device
	option name 'eth1'
	option mtu '1532'

config device
	option type '8021q'
	option ifname 'eth1'
	option vid '4094'
	option name 'eth1.4094'
	option mtu '1532'

config interface 'uw_bh_eth1'
	option proto 'batadv_hardif'
	option master 'bat0'
	option device 'eth1.4094'

config interface 'batmesh'
	option proto 'batadv_hardif'
	option master 'bat0'
	option device 'phy1-mesh0'
";

    #[test]
    fn the_wiring_follows_network() {
        let wiring = Wiring::of(Some(OVERLAY));
        assert_eq!(wiring.bridged, ["bat0", "eth1"].map(String::from).into());
        assert_eq!(
            wiring.vlans,
            [VlanHardif {
                name: "uw_bh_eth1".to_owned(),
                port: "eth1".to_owned(),
                vid: 4094,
            }]
        );
        assert_eq!(wiring.mtu("eth1.4094"), Some(1532));
        assert_eq!(wiring.mtu("eth1"), Some(1532));
        assert_eq!(wiring.mtu("eth0"), Some(1500));
        assert_eq!(wiring.mtu("eth0.4094"), None);
        // A VLAN device takes at most its port's MTU.
        let capped = Wiring::of(Some(&OVERLAY.replacen("'1532'", "'1500'", 1)));
        assert_eq!(capped.mtu("eth1.4094"), Some(1500));
        assert_eq!(Wiring::of(Some("config 'broken")), Wiring::default());
    }

    #[test]
    fn a_link_local_address_is_the_modified_eui_64() {
        let mac = MacAddr::new([0x94, 0x83, 0xc4, 0x00, 0x02, 0x01]);
        assert_eq!(
            link_local(mac),
            "fe80::9683:c4ff:fe00:201".parse::<Ipv6Addr>().unwrap()
        );
    }
}
