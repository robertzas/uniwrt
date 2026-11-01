//! A book driven through topology builds of a small house made in code (`World`): a gateway
//! (node 1) and an access point (node 2), each with one 5 GHz AP interface, and clients
//! moving between them.

mod book;

use std::collections::{BTreeMap, BTreeSet};

use uniwrt_core::{MacAddr, NodeId, Timestamp};
use uniwrt_model::{
    facts::{
        LeaseFacts, NodeFacts, RadioFacts, Rate, StationCapability, StationFacts, WifiIfaceFacts,
        WifiIfaceMode,
    },
    node::NodeRole,
    status::NodeState,
    wifi::{Band, ChannelWidth, WifiStandard},
};
use uniwrt_topo::{Member, Topology};

use crate::{Book, BookConfig, Changes};

pub(super) const AP: &str = "phy1-ap0";

/// A phone with a real OUI (Apple's `f0:18:98`).
pub(super) fn phone() -> MacAddr {
    "f0:18:98:00:00:01".parse().unwrap()
}

/// A randomized address.
pub(super) fn private(last: u8) -> MacAddr {
    MacAddr::new([0x5a, 0, 0, 0, 0, last])
}

pub(super) fn secs(secs: i64) -> Timestamp {
    Timestamp::from_unix_secs(1_000_000 + secs)
}

pub(super) struct World {
    pub(super) book: Book,
    pub(super) nodes: [NodeFacts; 2],
    pub(super) states: [NodeState; 2],
    /// When each client associated where it is.
    since: BTreeMap<MacAddr, Timestamp>,
    previous: Option<Topology>,
}

impl World {
    pub(super) fn new() -> Self {
        Self {
            book: Book::new(BookConfig::default()),
            nodes: [node(1), node(2)],
            states: [NodeState::Online; 2],
            since: BTreeMap::new(),
            previous: None,
        }
    }

    /// The client associated with node `node` (1 or 2) at `since`.
    pub(super) fn associate(&mut self, node: usize, mac: MacAddr, since: Timestamp) {
        self.leave(mac);
        self.since.insert(mac, since);
        self.nodes[node - 1].stations.push(station(mac));
    }

    pub(super) fn leave(&mut self, mac: MacAddr) {
        for facts in &mut self.nodes {
            facts.stations.retain(|station| station.mac != mac);
        }
    }

    pub(super) fn retries(&mut self, mac: MacAddr, packets: u64, retries: u64) {
        for facts in &mut self.nodes {
            for station in &mut facts.stations {
                if station.mac == mac {
                    station.tx_packets = packets;
                    station.tx_retries = Some(retries);
                    station.tx_failed = Some(0);
                }
            }
        }
    }

    pub(super) fn lease(&mut self, mac: MacAddr, hostname: &str, client_id: Option<&str>) {
        self.nodes[0].leases.push(LeaseFacts {
            ip: format!("192.168.1.{}", mac.octets()[5]).parse().unwrap(),
            mac: Some(mac),
            hostname: Some(hostname.to_owned()),
            client_id: client_id.map(str::to_owned),
            device: Some("br-lan".to_owned()),
            expires_in_s: Some(3600),
        });
    }

    /// Collects at `at` on every node, builds, and updates the book.
    pub(super) fn build(&mut self, at: Timestamp) -> Changes {
        for facts in &mut self.nodes {
            facts.collected_at = at;
            for station in &mut facts.stations {
                station.connected_s = at
                    .saturating_duration_since(self.since[&station.mac])
                    .as_secs();
            }
        }
        let members: Vec<Member<'_>> = self
            .nodes
            .iter()
            .zip(self.states)
            .enumerate()
            .map(|(index, (facts, state))| Member {
                id: NodeId::new(i64::try_from(index).unwrap() + 1),
                role: if index == 0 {
                    NodeRole::Gateway
                } else {
                    NodeRole::Ap
                },
                mac: node_mac(u8::try_from(index).unwrap() + 1),
                state,
                facts: Some(facts),
            })
            .collect();
        let topology = uniwrt_topo::build(&members, self.previous.as_ref());
        let changes = self.book.update(&topology, &members);
        self.previous = Some(topology);
        changes
    }
}

fn node_mac(id: u8) -> MacAddr {
    MacAddr::new([2, 0, 0, 0, id, 0])
}

fn node(id: u8) -> NodeFacts {
    let mut facts = NodeFacts {
        collected_at: secs(0),
        ..NodeFacts::default()
    };
    facts.board.hostname = format!("node{id}");
    facts.radios.push(RadioFacts {
        name: "radio1".to_owned(),
        phy: Some("phy1".to_owned()),
        band: Band::G5,
        up: true,
        disabled: false,
        channel: Some(36),
        frequency_mhz: Some(5180),
        center_frequency_mhz: Some(5210),
        width: Some(ChannelWidth::Mhz80),
        standard: Some(WifiStandard::He),
        tx_power_dbm: Some(20),
        noise_dbm: Some(-95),
        channel_busy_percent: Some(20.0),
        country: None,
        ifaces: vec![WifiIfaceFacts {
            name: Some(AP.to_owned()),
            section: None,
            mode: WifiIfaceMode::Ap,
            ssid: Some("home".to_owned()),
            mesh_id: None,
            bssid: Some(MacAddr::new([2, 0, 0, 0, id, 1])),
            encryption: None,
            networks: vec!["lan".to_owned()],
            up: true,
        }],
    });
    facts
}

fn station(mac: MacAddr) -> StationFacts {
    let rate = Rate {
        kbps: 1_000_000,
        standard: WifiStandard::He,
        mcs: Some(10),
        nss: Some(2),
        width: Some(ChannelWidth::Mhz80),
        guard_interval_ns: Some(800),
    };
    StationFacts {
        mac,
        iface: AP.to_owned(),
        signal_dbm: Some(-55),
        connected_s: 0,
        inactive_ms: 10,
        rx_rate: Some(rate),
        tx_rate: Some(rate),
        rx_bytes: 0,
        tx_bytes: 0,
        rx_packets: 0,
        tx_packets: 0,
        tx_retries: Some(0),
        tx_failed: Some(0),
        expected_throughput_kbps: None,
        rx_airtime_us: None,
        tx_airtime_us: None,
        peer_link: None,
        authorized: true,
        standard: Some(WifiStandard::He),
        capabilities: BTreeSet::from([StationCapability::FastTransition]),
    }
}
