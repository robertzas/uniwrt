//! Who is who: the adopted nodes ([`Member`]s) in build order, the MAC addresses each one owns,
//! the vertex ids, and lookups in a node's facts.

use std::collections::BTreeMap;

use uniwrt_core::{MacAddr, NodeId};
use uniwrt_model::{
    facts::{DeviceFacts, DeviceKind, NodeFacts, RadioFacts, WifiIfaceFacts},
    node::NodeRole,
    status::NodeState,
    wifi::Band,
};

/// An adopted node, as the topology needs it.
#[derive(Debug, Clone, Copy)]
pub struct Member<'a> {
    pub id: NodeId,
    pub role: NodeRole,
    /// The MAC address it was adopted by, which other nodes may see even while its own facts
    /// are missing.
    pub mac: MacAddr,
    pub state: NodeState,
    /// Its latest facts; `None` until any arrive.
    pub facts: Option<&'a NodeFacts>,
}

pub(crate) const INTERNET: &str = "internet";

/// The members, gateways first and then by id (so the gateway's view of a link wins), and the
/// owner of each MAC address they have.
pub(crate) struct Fleet<'a> {
    pub(crate) members: Vec<Member<'a>>,
    owners: BTreeMap<MacAddr, usize>,
}

impl<'a> Fleet<'a> {
    pub(crate) fn new(members: &[Member<'a>]) -> Self {
        let mut members = members.to_vec();
        members.sort_by_key(|member| (member.role != NodeRole::Gateway, member.id));
        let mut owners = BTreeMap::new();
        for (index, member) in members.iter().enumerate() {
            for mac in macs(member) {
                owners.entry(mac).or_insert(index);
            }
        }
        Self { members, owners }
    }

    /// The member that owns the address.
    pub(crate) fn owner(&self, mac: MacAddr) -> Option<usize> {
        self.owners.get(&mac).copied()
    }

    pub(crate) fn vertex(&self, member: usize) -> String {
        node_vertex(self.members[member].id)
    }

    /// The vertex the address belongs to: its member's, else a peer's of its own.
    pub(crate) fn vertex_of(&self, mac: MacAddr) -> String {
        self.owner(mac)
            .map_or_else(|| peer_vertex(mac), |member| self.vertex(member))
    }

    /// The members with facts, by index.
    pub(crate) fn with_facts(&self) -> impl Iterator<Item = (usize, &Member<'a>, &'a NodeFacts)> {
        self.members
            .iter()
            .enumerate()
            .filter_map(|(index, member)| Some((index, member, member.facts?)))
    }
}

pub(crate) fn node_vertex(id: NodeId) -> String {
    format!("node:{id}")
}

pub(crate) fn peer_vertex(mac: MacAddr) -> String {
    format!("peer:{mac}")
}

pub(crate) fn client_vertex(mac: MacAddr) -> String {
    format!("client:{mac}")
}

/// An address that names one device: not multicast, not all zeros.
pub(crate) fn unicast(mac: MacAddr) -> bool {
    !mac.is_multicast() && mac != MacAddr::default()
}

/// Every address the member answers to: its adoption MAC, its board's label MAC, its devices',
/// its BSSIDs, and its batman-adv meshes' and their hard interfaces'.
fn macs(member: &Member<'_>) -> Vec<MacAddr> {
    let mut macs = vec![member.mac];
    if let Some(facts) = member.facts {
        macs.extend(facts.board.label_mac);
        macs.extend(
            facts
                .devices
                .iter()
                .filter(|device| !matches!(device.kind, DeviceKind::Loopback | DeviceKind::Tunnel))
                .filter_map(|device| device.mac),
        );
        macs.extend(
            facts
                .radios
                .iter()
                .flat_map(|radio| &radio.ifaces)
                .filter_map(|iface| iface.bssid),
        );
        for mesh in &facts.batman {
            macs.extend(mesh.mac);
            macs.extend(mesh.hard_ifs.iter().filter_map(|hard_if| hard_if.mac));
        }
    }
    macs.retain(|mac| unicast(*mac));
    macs
}

pub(crate) fn device<'f>(facts: &'f NodeFacts, name: &str) -> Option<&'f DeviceFacts> {
    facts.devices.iter().find(|device| device.name == name)
}

/// The Wi-Fi interface with the kernel name, and its radio.
pub(crate) fn wifi_iface<'f>(
    facts: &'f NodeFacts,
    name: &str,
) -> Option<(&'f RadioFacts, &'f WifiIfaceFacts)> {
    facts.radios.iter().find_map(|radio| {
        radio
            .ifaces
            .iter()
            .find(|iface| iface.name.as_deref() == Some(name))
            .map(|iface| (radio, iface))
    })
}

/// The device is a Wi-Fi interface.
pub(crate) fn is_wireless(facts: &NodeFacts, name: &str) -> bool {
    device(facts, name).map_or_else(
        || wifi_iface(facts, name).is_some(),
        |device| device.kind == DeviceKind::Wireless,
    )
}

/// How parallel Wi-Fi and mesh links are told apart: by band, as UCI names it.
pub(crate) const fn band_tag(band: Band) -> &'static str {
    match band {
        Band::G2 => "2g",
        Band::G5 => "5g",
        Band::G6 => "6g",
        Band::G60 => "60g",
    }
}

/// The devices of the interfaces with a default route: where a node's upstream is.
pub(crate) fn uplink_devices(facts: &NodeFacts) -> Vec<&str> {
    facts
        .interfaces
        .iter()
        .filter(|iface| iface.ipv4_gateway.is_some() || iface.ipv6_gateway.is_some())
        .filter_map(|iface| iface.device.as_deref())
        .collect()
}
