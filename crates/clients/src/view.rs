//! What a client looks like now ([`Client`]), as the API and the WS topics `clients` and
//! `client:<mac>` carry it, its signal history's points ([`ClientSample`]), and what one
//! topology build says about each client ([`observe`]).

use std::{collections::BTreeMap, net::IpAddr};

use serde::{Deserialize, Serialize};
use uniwrt_core::{MacAddr, NodeId, Timestamp};
use uniwrt_model::{
    client::{ClientMedium, ClientWifi},
    facts::{RadioFacts, Rate, StationCapability, StationFacts},
    status::NodeState,
    wifi::{Band, WifiStandard},
};
use uniwrt_topo::{Medium, Member, TopoEdge, TopoKind, TopoNode, Topology, Traffic};

use crate::{ClientKind, Experience};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Client {
    pub mac: MacAddr,
    /// The name a user gave it; for a randomized address without one, the name given to
    /// another address of its group.
    pub alias: Option<String>,
    /// The name it asked the DHCP server for.
    pub hostname: Option<String>,
    pub vendor: Option<String>,
    pub kind: ClientKind,
    /// A locally administered address: randomized ("private") by the device.
    pub randomized: bool,
    /// What ties a randomized address to the device's other ones: `name:<hostname>` or
    /// `id:<DHCP client ID>`.
    pub group: Option<String>,
    pub ips: Vec<IpAddr>,
    pub online: bool,
    /// Where it is connected; while offline, where it was last.
    pub connection: Option<Connection>,
    pub first_seen: Timestamp,
    pub last_seen: Timestamp,
    /// When a user blocked it (features/wifi.md, Client actions).
    pub blocked: Option<Timestamp>,
    /// While it is online over Wi-Fi.
    pub experience: Option<Experience>,
    /// What it showed the access points over its associations, kept while it is away.
    pub wifi: ClientWifi,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Connection {
    /// `None` behind a mesh peer that is not adopted.
    pub node: Option<NodeId>,
    /// The topology's vertex it hangs from: `node:<id>`, `switch:<node>:<port>` or
    /// `peer:<mac>`.
    pub upstream: String,
    /// That vertex's name: a mesh peer's DHCP hostname, an adopted node's hostname.
    pub upstream_name: Option<String>,
    /// The access point's interface or the node's port.
    pub port: Option<String>,
    pub medium: ClientMedium,
    /// When it connected there.
    pub since: Timestamp,
    pub band: Option<Band>,
    pub ssid: Option<String>,
    pub channel: Option<u16>,
    pub signal_dbm: Option<i16>,
    /// The radio's noise floor.
    pub noise_dbm: Option<i16>,
    /// The rates of the last frames from and to it.
    pub rx_rate: Option<Rate>,
    pub tx_rate: Option<Rate>,
    /// The newest Wi-Fi standard it supports.
    pub standard: Option<WifiStandard>,
    pub capabilities: Vec<StationCapability>,
    pub capacity_mbps: Option<u32>,
    /// Its link's counters and rates; `down` is toward the client.
    pub traffic: Option<Traffic>,
}

/// One point of a Wi-Fi client's recent history (the Signal tab).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ClientSample {
    pub at: Timestamp,
    pub signal_dbm: Option<i16>,
    pub snr_db: Option<i16>,
    pub tx_kbps: Option<u32>,
    pub rx_kbps: Option<u32>,
    pub down_bps: Option<u64>,
    pub up_bps: Option<u64>,
    pub score: Option<u8>,
}

/// One client in one build.
pub(crate) struct Seen<'a> {
    pub(crate) vertex: &'a TopoNode,
    pub(crate) edge: &'a TopoEdge,
    pub(crate) upstream: &'a str,
    pub(crate) upstream_name: Option<&'a str>,
    pub(crate) node: Option<NodeId>,
    pub(crate) port: Option<String>,
    pub(crate) medium: ClientMedium,
    /// Its link is up and the node reporting it is not offline.
    pub(crate) online: bool,
    /// When the facts that place it were collected.
    pub(crate) at: Timestamp,
    pub(crate) station: Option<&'a StationFacts>,
    pub(crate) radio: Option<&'a RadioFacts>,
    pub(crate) client_id: Option<&'a str>,
}

/// Every client of the build, by MAC address, at the end of its edge.
pub(crate) fn observe<'a>(
    topology: &'a Topology,
    members: &[Member<'a>],
) -> BTreeMap<MacAddr, Seen<'a>> {
    let vertices: BTreeMap<&str, &TopoNode> = topology
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let clients: BTreeMap<&str, &TopoNode> = vertices
        .iter()
        .filter(|(_, node)| node.kind == TopoKind::Client && node.mac.is_some())
        .map(|(id, node)| (*id, *node))
        .collect();
    let mut client_ids = BTreeMap::new();
    for facts in members.iter().filter_map(|member| member.facts) {
        for lease in &facts.leases {
            if let (Some(mac), Some(id)) = (lease.mac, lease.client_id.as_deref()) {
                client_ids.entry(mac).or_insert(id);
            }
        }
    }
    let mut seen = BTreeMap::new();
    for edge in &topology.edges {
        let (vertex, upstream, port) = if let Some(vertex) = clients.get(edge.b.as_str()) {
            (*vertex, edge.a.as_str(), edge.a_port.as_deref())
        } else if let Some(vertex) = clients.get(edge.a.as_str()) {
            (*vertex, edge.b.as_str(), edge.b_port.as_deref())
        } else {
            continue;
        };
        let Some(mac) = vertex.mac else { continue };
        let (node, port) = place(upstream, port);
        let member = node.and_then(|node| members.iter().find(|member| member.id == node));
        let facts = member.and_then(|member| member.facts);
        let medium = if edge.medium == Medium::Wifi {
            ClientMedium::Wifi
        } else {
            ClientMedium::Wired
        };
        let wifi_port = port.as_deref().filter(|_| medium == ClientMedium::Wifi);
        let station = wifi_port.and_then(|port| {
            facts?
                .stations
                .iter()
                .find(|station| station.mac == mac && station.iface == port)
        });
        let radio = wifi_port.and_then(|port| {
            facts?.radios.iter().find(|radio| {
                radio
                    .ifaces
                    .iter()
                    .any(|iface| iface.name.as_deref() == Some(port))
            })
        });
        seen.insert(
            mac,
            Seen {
                vertex,
                edge,
                upstream,
                upstream_name: vertices
                    .get(upstream)
                    .and_then(|vertex| vertex.name.as_deref()),
                node,
                port,
                medium,
                online: edge.up && member.is_none_or(|member| member.state != NodeState::Offline),
                at: facts.map_or(topology.at, |facts| facts.collected_at),
                station,
                radio,
                client_id: client_ids.get(&mac).copied(),
            },
        );
    }
    seen
}

/// The node and port of an upstream vertex: a node's own port, or the port of the node that an
/// inferred switch hangs from (`switch:<node>:<port>`).
fn place(upstream: &str, port: Option<&str>) -> (Option<NodeId>, Option<String>) {
    let node = |id: &str| id.parse().ok().filter(|id| *id > 0).map(NodeId::new);
    if let Some(id) = upstream.strip_prefix("node:") {
        return (node(id), port.map(str::to_owned));
    }
    if let Some((id, port)) = upstream
        .strip_prefix("switch:")
        .and_then(|rest| rest.split_once(':'))
    {
        return (node(id), Some(port.to_owned()));
    }
    (None, port.map(str::to_owned))
}

#[cfg(test)]
mod tests {
    use uniwrt_core::NodeId;

    use super::place;

    #[test]
    fn upstreams_name_their_node_and_port() {
        assert_eq!(
            place("node:3", Some("phy1-ap0")),
            (Some(NodeId::new(3)), Some("phy1-ap0".to_owned()))
        );
        assert_eq!(
            place("switch:2:lan3", None),
            (Some(NodeId::new(2)), Some("lan3".to_owned()))
        );
        assert_eq!(place("peer:02:00:00:00:00:33", None), (None, None));
        assert_eq!(place("node:x", None), (None, None));
    }
}
