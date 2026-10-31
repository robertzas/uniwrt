//! The graph while it is built: vertices by id, and links by key, each merged from every end
//! that reports it.

use std::collections::{BTreeMap, BTreeSet};

use uniwrt_core::{MacAddr, Timestamp};
use uniwrt_model::{facts::Counters, wifi::Band};

use crate::{
    Medium, TopoKind, TopoNode,
    clients::Directory,
    fleet::{client_vertex, peer_vertex},
};

/// One end's report of a link: `ends[0]` reported it, unless both ends are only inferred.
#[derive(Debug, Clone)]
pub(crate) struct Link {
    pub(crate) ends: [String; 2],
    pub(crate) ports: [Option<String>; 2],
    pub(crate) medium: Medium,
    /// What tells parallel links between the same ends apart: the band, or the WAN device.
    pub(crate) tag: Option<String>,
    pub(crate) up: bool,
    pub(crate) band: Option<Band>,
    pub(crate) ssid: Option<String>,
    pub(crate) signal_dbm: Option<i16>,
    pub(crate) tq: Option<u8>,
    pub(crate) capacity_mbps: Option<u32>,
    pub(crate) quality: Option<u8>,
    pub(crate) sample: Option<Sample>,
}

/// Bytes `out` from `ends[0]` to `ends[1]` and `in` back, as `ends[0]`'s counters say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sample {
    pub(crate) at: Timestamp,
    pub(crate) out_bytes: u64,
    pub(crate) in_bytes: u64,
}

impl Sample {
    /// A port's counters: what it sent went out.
    pub(crate) const fn port(at: Timestamp, counters: &Counters) -> Self {
        Self {
            at,
            out_bytes: counters.tx_bytes,
            in_bytes: counters.rx_bytes,
        }
    }

    /// The same reading, as the other end would count it.
    pub(crate) const fn reversed(self) -> Self {
        Self {
            out_bytes: self.in_bytes,
            in_bytes: self.out_bytes,
            ..self
        }
    }
}

impl Link {
    pub(crate) fn new(from: String, to: String, medium: Medium) -> Self {
        Self {
            ends: [from, to],
            ports: [None, None],
            medium,
            tag: None,
            up: true,
            band: None,
            ssid: None,
            signal_dbm: None,
            tq: None,
            capacity_mbps: None,
            quality: None,
            sample: None,
        }
    }

    pub(crate) fn key(&self) -> String {
        key(
            &self.ends[0],
            &self.ends[1],
            self.medium,
            self.tag.as_deref(),
        )
    }

    /// The same link seen from its other end.
    pub(crate) fn reversed(mut self) -> Self {
        self.ends.swap(0, 1);
        self.ports.swap(0, 1);
        self.sample = self.sample.map(Sample::reversed);
        self
    }

    /// Folds in another report of the same link: what this one lacks, the weaker of what both
    /// measure, and up if either end says so. The first report's traffic stays, so a link's
    /// rate comes from the same counters build after build.
    fn merge(&mut self, other: Self) {
        let other = if other.ends == self.ends {
            other
        } else {
            other.reversed()
        };
        let [near, far] = other.ports;
        self.ports[0] = self.ports[0].take().or(near);
        self.ports[1] = self.ports[1].take().or(far);
        self.up |= other.up;
        self.band = self.band.or(other.band);
        self.ssid = self.ssid.take().or(other.ssid);
        self.signal_dbm = weaker(self.signal_dbm, other.signal_dbm);
        self.tq = weaker(self.tq, other.tq);
        self.capacity_mbps = weaker(self.capacity_mbps, other.capacity_mbps);
        self.quality = weaker(self.quality, other.quality);
        self.sample = self.sample.or(other.sample);
    }
}

/// An edge's id: both ends in byte order, the medium and the tag.
pub(crate) fn key(a: &str, b: &str, medium: Medium, tag: Option<&str>) -> String {
    let (low, high) = if a <= b { (a, b) } else { (b, a) };
    let medium = match medium {
        Medium::Wan => "wan",
        Medium::Ethernet => "ethernet",
        Medium::Mesh => "mesh",
        Medium::Wifi => "wifi",
    };
    match tag {
        Some(tag) => format!("{low}~{high}~{medium}~{tag}"),
        None => format!("{low}~{high}~{medium}"),
    }
}

pub(crate) fn weaker<T: Ord>(a: Option<T>, b: Option<T>) -> Option<T> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

#[derive(Debug, Default)]
pub(crate) struct Graph {
    pub(crate) nodes: BTreeMap<String, TopoNode>,
    pub(crate) links: BTreeMap<String, Link>,
    /// Links on batman-adv's best paths toward a gateway.
    pub(crate) preferred: BTreeSet<String>,
    /// Addresses that have a vertex of their own (clients and peers).
    placed: BTreeSet<MacAddr>,
}

impl Graph {
    /// Adds a vertex, unless one with its id exists.
    pub(crate) fn vertex(&mut self, node: TopoNode) {
        self.nodes.entry(node.id.clone()).or_insert(node);
    }

    pub(crate) fn link(&mut self, link: Link) {
        match self.links.entry(link.key()) {
            std::collections::btree_map::Entry::Occupied(mut entry) => entry.get_mut().merge(link),
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(link);
            }
        }
    }

    pub(crate) fn prefer(&mut self, key: String) {
        self.preferred.insert(key);
    }

    pub(crate) fn is_placed(&self, mac: MacAddr) -> bool {
        self.placed.contains(&mac)
    }

    /// The client's vertex, added if it is new.
    pub(crate) fn client(&mut self, mac: MacAddr, directory: &Directory) -> String {
        self.other(TopoKind::Client, client_vertex(mac), mac, directory)
    }

    /// The vertex of a mesh peer that is not adopted, added if it is new.
    pub(crate) fn peer(&mut self, mac: MacAddr, directory: &Directory) -> String {
        self.other(TopoKind::Peer, peer_vertex(mac), mac, directory)
    }

    fn other(&mut self, kind: TopoKind, id: String, mac: MacAddr, directory: &Directory) -> String {
        self.placed.insert(mac);
        let host = directory.get(mac);
        self.vertex(TopoNode {
            id: id.clone(),
            kind,
            node: None,
            mac: Some(mac),
            name: host.and_then(|host| host.name.clone()),
            ips: host
                .map(|host| host.ips.iter().copied().collect())
                .unwrap_or_default(),
            state: None,
            cpu_percent: None,
            memory_percent: None,
            parent: None,
            uplink: None,
            hops: None,
        });
        id
    }
}
