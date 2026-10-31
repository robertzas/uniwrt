//! Facts built in code, as small as each case needs. Node `n` has the MAC `02:00:00:00:0n:00`
//! and its ports `02:00:00:00:0n:pp`; clients are `0a:00:00:00:00:cc`.

use std::collections::BTreeSet;

use uniwrt_core::{MacAddr, NodeId, Timestamp};
use uniwrt_model::{
    facts::{
        BatmanFacts, BatmanNeighbor, Counters, DeviceFacts, DeviceKind, FdbEntry, InterfaceFacts,
        LeaseFacts, LocalClient, NeighborFacts, NodeFacts, Originator, PeerLinkState, RadioFacts,
        RemoteClient, StationFacts, WifiIfaceFacts, WifiIfaceMode,
    },
    node::NodeRole,
    status::NodeState,
    wifi::Band,
};

use crate::{Member, Topology, build};

pub(super) fn node_mac(node: u8, port: u8) -> MacAddr {
    MacAddr::new([2, 0, 0, 0, node, port])
}

pub(super) fn client_mac(client: u8) -> MacAddr {
    MacAddr::new([0x0a, 0, 0, 0, 0, client])
}

pub(super) struct Node {
    pub(super) id: u8,
    pub(super) role: NodeRole,
    pub(super) state: NodeState,
    pub(super) facts: NodeFacts,
}

impl Node {
    pub(super) fn new(id: u8, role: NodeRole) -> Self {
        let mut facts = NodeFacts {
            collected_at: Timestamp::from_unix_ms(1_000),
            ..NodeFacts::default()
        };
        facts.board.hostname = format!("node{id}");
        Self {
            id,
            role,
            state: NodeState::Online,
            facts,
        }
        .device("br-lan", DeviceKind::Bridge, 0)
    }

    pub(super) fn gateway(id: u8) -> Self {
        Self::new(id, NodeRole::Gateway)
    }

    pub(super) fn ap(id: u8) -> Self {
        Self::new(id, NodeRole::Ap)
    }

    pub(super) fn at(mut self, ms: i64) -> Self {
        self.facts.collected_at = Timestamp::from_unix_ms(ms);
        self
    }

    /// A device whose MAC is the node's port `port`, up, with carrier, at 1 Gbit/s.
    pub(super) fn device(mut self, name: &str, kind: DeviceKind, port: u8) -> Self {
        self.facts.devices.push(DeviceFacts {
            name: name.to_owned(),
            kind,
            mac: Some(node_mac(self.id, port)),
            mtu: 1500,
            up: true,
            carrier: true,
            speed_mbps: (kind == DeviceKind::Ethernet).then_some(1000),
            full_duplex: None,
            master: None,
            counters: Counters::default(),
        });
        self
    }

    pub(super) fn counters(mut self, name: &str, tx_bytes: u64, rx_bytes: u64) -> Self {
        let device = self
            .facts
            .devices
            .iter_mut()
            .find(|device| device.name == name)
            .unwrap();
        device.counters.tx_bytes = tx_bytes;
        device.counters.rx_bytes = rx_bytes;
        self
    }

    pub(super) fn port(self, name: &str, port: u8) -> Self {
        self.device(name, DeviceKind::Ethernet, port)
    }

    pub(super) fn fdb(mut self, port: &str, macs: &[MacAddr]) -> Self {
        self.facts.fdb.extend(macs.iter().map(|&mac| FdbEntry {
            mac,
            bridge: "br-lan".to_owned(),
            port: port.to_owned(),
            vlan: None,
        }));
        self
    }

    /// An interface on `device`, with a default route if `gateway`.
    pub(super) fn interface(mut self, name: &str, device: &str, up: bool, gateway: bool) -> Self {
        self.facts.interfaces.push(InterfaceFacts {
            name: name.to_owned(),
            proto: "dhcp".to_owned(),
            up,
            device: Some(device.to_owned()),
            uptime_s: None,
            ipv4: Vec::new(),
            ipv6: Vec::new(),
            delegated_prefixes: Vec::new(),
            ipv4_gateway: gateway.then(|| "192.0.2.1".parse().unwrap()),
            ipv6_gateway: None,
            dns_servers: Vec::new(),
            errors: Vec::new(),
        });
        self
    }

    /// A radio on `band` with one interface `iface` (MAC/BSSID: the node's port `port`).
    pub(super) fn radio(mut self, band: Band, iface: &str, mode: WifiIfaceMode, port: u8) -> Self {
        let name = format!("radio{}", self.facts.radios.len());
        self.facts.radios.push(RadioFacts {
            name,
            phy: None,
            band,
            up: true,
            disabled: false,
            channel: None,
            frequency_mhz: None,
            center_frequency_mhz: None,
            width: None,
            standard: None,
            tx_power_dbm: None,
            noise_dbm: None,
            channel_busy_percent: None,
            country: None,
            ifaces: vec![WifiIfaceFacts {
                name: Some(iface.to_owned()),
                section: None,
                mode,
                ssid: (mode == WifiIfaceMode::Ap).then(|| "home".to_owned()),
                mesh_id: None,
                bssid: Some(node_mac(self.id, port)),
                encryption: None,
                networks: Vec::new(),
                up: true,
            }],
        });
        self.device(iface, DeviceKind::Wireless, port)
    }

    pub(super) fn station(
        mut self,
        iface: &str,
        mac: MacAddr,
        signal_dbm: i16,
        inactive_ms: u64,
    ) -> Self {
        self.facts.stations.push(StationFacts {
            mac,
            iface: iface.to_owned(),
            signal_dbm: Some(signal_dbm),
            connected_s: 60,
            inactive_ms,
            rx_rate: None,
            tx_rate: None,
            rx_bytes: 1_000,
            tx_bytes: 2_000,
            rx_packets: 0,
            tx_packets: 0,
            tx_retries: None,
            tx_failed: None,
            expected_throughput_kbps: Some(300_000),
            rx_airtime_us: None,
            tx_airtime_us: None,
            peer_link: None,
            authorized: true,
            standard: None,
            capabilities: BTreeSet::new(),
        });
        self
    }

    pub(super) fn peer_link(
        mut self,
        iface: &str,
        mac: MacAddr,
        signal_dbm: i16,
        state: PeerLinkState,
    ) -> Self {
        self = self.station(iface, mac, signal_dbm, 0);
        self.facts.stations.last_mut().unwrap().peer_link = Some(state);
        self
    }

    fn mesh(&mut self) -> &mut BatmanFacts {
        if self.facts.batman.is_empty() {
            self.facts.batman.push(BatmanFacts {
                mesh_if: "bat0".to_owned(),
                mac: Some(node_mac(self.id, 0xb0)),
                algorithm: None,
                gw_mode: None,
                hard_ifs: Vec::new(),
                originators: Vec::new(),
                neighbors: Vec::new(),
                gateways: Vec::new(),
                remote_clients: Vec::new(),
                local_clients: Vec::new(),
            });
        }
        &mut self.facts.batman[0]
    }

    /// A batman-adv neighbor heard on `hard_if`, with the route to it.
    pub(super) fn neighbor(mut self, hard_if: &str, mac: MacAddr, tq: u8) -> Self {
        let mesh = self.mesh();
        mesh.neighbors.push(BatmanNeighbor {
            mac,
            hard_if: hard_if.to_owned(),
            last_seen_ms: 100,
            throughput_kbps: None,
        });
        self.route(mac, mac, hard_if, tq, false)
    }

    /// A route to originator `to` through `next_hop`.
    pub(super) fn route(
        mut self,
        to: MacAddr,
        next_hop: MacAddr,
        hard_if: &str,
        tq: u8,
        best: bool,
    ) -> Self {
        self.mesh().originators.push(Originator {
            mac: to,
            next_hop,
            hard_if: hard_if.to_owned(),
            last_seen_ms: 100,
            tq: Some(tq),
            throughput_kbps: None,
            best,
        });
        self
    }

    pub(super) fn local_client(mut self, mac: MacAddr, wifi: bool) -> Self {
        self.mesh().local_clients.push(LocalClient {
            mac,
            vid: None,
            wifi,
            last_seen_ms: None,
        });
        self
    }

    pub(super) fn remote_client(mut self, mac: MacAddr, originator: MacAddr) -> Self {
        self.mesh().remote_clients.push(RemoteClient {
            mac,
            vid: None,
            originator,
            wifi: true,
            roaming: false,
        });
        self
    }

    pub(super) fn lease(mut self, mac: MacAddr, ip: &str, hostname: &str) -> Self {
        self.facts.leases.push(LeaseFacts {
            ip: ip.parse().unwrap(),
            mac: Some(mac),
            hostname: Some(hostname.to_owned()),
            client_id: None,
            device: None,
            expires_in_s: Some(3600),
        });
        self
    }

    pub(super) fn arp(mut self, mac: MacAddr, ip: &str, device: &str) -> Self {
        self.facts.neighbors.push(NeighborFacts {
            ip: ip.parse().unwrap(),
            mac,
            device: device.to_owned(),
            state: None,
            router: false,
        });
        self
    }
}

/// The topology of the nodes, built after `previous`.
pub(super) fn topology_after(nodes: &[Node], previous: Option<&Topology>) -> Topology {
    let members: Vec<Member<'_>> = nodes
        .iter()
        .map(|node| Member {
            id: NodeId::new(i64::from(node.id)),
            role: node.role,
            mac: node_mac(node.id, 0),
            state: node.state,
            facts: Some(&node.facts),
        })
        .collect();
    build(&members, previous)
}

pub(super) fn topology(nodes: &[Node]) -> Topology {
    topology_after(nodes, None)
}

/// The edges as `a > b (medium)`, sorted.
pub(super) fn edges(topology: &Topology) -> Vec<String> {
    let mut edges: Vec<String> = topology
        .edges
        .iter()
        .map(|edge| {
            format!(
                "{} > {} ({:?}{})",
                edge.a,
                edge.b,
                edge.medium,
                if edge.up { "" } else { ", down" }
            )
        })
        .collect();
    edges.sort();
    edges
}

/// Each vertex with its parent in the tree view, as `vertex < parent`.
pub(super) fn parents(topology: &Topology) -> Vec<String> {
    topology
        .nodes
        .iter()
        .map(|node| format!("{} < {}", node.id, node.parent.as_deref().unwrap_or("-")))
        .collect()
}
