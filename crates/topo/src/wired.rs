//! Wired links from the bridges' forwarding databases. Each wired port of a node sees the
//! nodes and clients beyond it; those that another node there has on a port other than the one
//! toward this node are behind that node, the rest share the port's segment. Segments that
//! name each other (two nodes, each on its port toward the other) are one; a segment of two is a
//! cable, one of more an unmanaged switch.

use std::collections::{BTreeMap, BTreeSet};

use uniwrt_core::MacAddr;
use uniwrt_model::facts::{DeviceKind, NodeFacts};

use crate::{
    Medium, TopoKind, TopoNode,
    clients::Directory,
    fleet::{Fleet, device, unicast},
    links::{Graph, Link, Sample, weaker},
};

/// What a bridge port leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Seen {
    Node(usize),
    Client(MacAddr),
}

/// A node's port.
type Port = (usize, String);

pub(crate) fn link(graph: &mut Graph, fleet: &Fleet<'_>, directory: &Directory) {
    let ports = ports(graph, fleet);
    // Where each node sees each thing it sees.
    let mut port_of = BTreeMap::<(usize, Seen), &str>::new();
    for ((member, port), seen) in &ports {
        for &item in seen {
            port_of.entry((*member, item)).or_insert(port);
        }
    }
    let behind = |via: usize, from: usize, item: Seen| {
        let toward = port_of.get(&(via, Seen::Node(from)));
        let at = port_of.get(&(via, item));
        matches!((toward, at), (Some(toward), Some(at)) if toward != at)
    };
    let mut segments = BTreeMap::<Port, BTreeSet<Seen>>::new();
    for ((member, port), seen) in &ports {
        let direct = seen
            .iter()
            .copied()
            .filter(|&item| {
                !seen.iter().any(|&via| match via {
                    Seen::Node(via) => Seen::Node(via) != item && behind(via, *member, item),
                    Seen::Client(_) => false,
                })
            })
            .collect();
        segments.insert((*member, port.clone()), direct);
    }
    // Segments that name each other are one.
    let keys: Vec<Port> = segments.keys().cloned().collect();
    let mut sets = UnionFind::new(keys.len());
    for (index, (member, _)) in keys.iter().enumerate() {
        for item in &segments[&keys[index]] {
            let Seen::Node(other) = *item else { continue };
            let Some(port) = port_of.get(&(other, Seen::Node(*member))) else {
                continue;
            };
            if let Ok(found) = keys.binary_search(&(other, (*port).to_owned())) {
                sets.union(index, found);
            }
        }
    }
    let mut groups = BTreeMap::<usize, Vec<usize>>::new();
    for index in 0..keys.len() {
        groups.entry(sets.find(index)).or_default().push(index);
    }
    for members in groups.values() {
        segment(
            graph,
            fleet,
            directory,
            members
                .iter()
                .map(|&index| (&keys[index], &segments[&keys[index]])),
        );
    }
}

/// Every wired port of every node with what it sees: the other nodes, and the clients that no
/// Wi-Fi or mesh source placed.
fn ports(graph: &Graph, fleet: &Fleet<'_>) -> BTreeMap<Port, BTreeSet<Seen>> {
    let mut ports = BTreeMap::<Port, BTreeSet<Seen>>::new();
    for (member, _, facts) in fleet.with_facts() {
        for entry in &facts.fdb {
            if !unicast(entry.mac) || !wired(facts, &entry.port) {
                continue;
            }
            let seen = match fleet.owner(entry.mac) {
                Some(owner) if owner == member => continue,
                Some(owner) => Seen::Node(owner),
                None if graph.is_placed(entry.mac) => continue,
                None => Seen::Client(entry.mac),
            };
            ports
                .entry((member, entry.port.clone()))
                .or_default()
                .insert(seen);
        }
    }
    ports
}

/// A port that is neither Wi-Fi nor a mesh (whose clients other sources place) nor a tunnel.
fn wired(facts: &NodeFacts, port: &str) -> bool {
    device(facts, port).is_none_or(|device| {
        !matches!(
            device.kind,
            DeviceKind::Wireless | DeviceKind::Batman | DeviceKind::Tunnel
        )
    })
}

/// One segment, seen from the ports that lead into it.
fn segment<'p>(
    graph: &mut Graph,
    fleet: &Fleet<'_>,
    directory: &Directory,
    views: impl Iterator<Item = (&'p Port, &'p BTreeSet<Seen>)>,
) {
    let mut node_ports = BTreeMap::<usize, &str>::new();
    let mut seen = BTreeSet::<Seen>::new();
    for ((member, port), items) in views {
        node_ports.entry(*member).or_insert(port.as_str());
        seen.insert(Seen::Node(*member));
        seen.extend(items.iter().copied().filter(|item| match item {
            Seen::Client(mac) => !graph.is_placed(*mac),
            Seen::Node(_) => true,
        }));
    }
    let Some((&owner, &owner_port)) = node_ports.first_key_value() else {
        return;
    };
    let ends: Vec<(String, Option<Side>)> = seen
        .iter()
        .map(|item| match *item {
            Seen::Node(member) => {
                let side = node_ports
                    .get(&member)
                    .and_then(|port| Side::of(fleet, member, port));
                (fleet.vertex(member), side)
            }
            Seen::Client(mac) => (graph.client(mac, directory), None),
        })
        .collect();
    if let Ok(pair) = <[_; 2]>::try_from(ends.clone()) {
        graph.link(cable(pair));
        return;
    }
    let switch = format!("switch:{}:{owner_port}", fleet.members[owner].id);
    graph.vertex(TopoNode {
        id: switch.clone(),
        kind: TopoKind::Switch,
        node: None,
        mac: None,
        name: None,
        ips: Vec::new(),
        state: None,
        cpu_percent: None,
        memory_percent: None,
        parent: None,
        uplink: None,
        hops: None,
    });
    for end in ends {
        graph.link(cable([end, (switch.clone(), None)]));
    }
}

/// A node's side of a wired link: its port, with the port's carrier, speed and counters.
#[derive(Debug, Clone)]
struct Side {
    port: String,
    carrier: bool,
    speed_mbps: Option<u32>,
    sample: Option<Sample>,
}

impl Side {
    fn of(fleet: &Fleet<'_>, member: usize, port: &str) -> Option<Self> {
        let facts = fleet.members[member].facts?;
        let dev = device(facts, port);
        Some(Self {
            port: port.to_owned(),
            carrier: dev.is_none_or(|dev| dev.carrier),
            speed_mbps: dev.and_then(|dev| dev.speed_mbps),
            sample: dev.map(|dev| Sample::port(facts.collected_at, &dev.counters)),
        })
    }
}

/// An Ethernet link between two ends, from the sides known: down if either port has no carrier.
fn cable(ends: [(String, Option<Side>); 2]) -> Link {
    let [(a, a_side), (b, b_side)] = ends;
    let mut link = Link::new(a, b, Medium::Ethernet);
    for (index, side) in [a_side, b_side].into_iter().enumerate() {
        let Some(side) = side else { continue };
        link.ports[index] = Some(side.port);
        link.up &= side.carrier;
        link.capacity_mbps = weaker(link.capacity_mbps, side.speed_mbps);
        if link.sample.is_none() {
            link.sample = side.sample.map(|sample| {
                if index == 0 {
                    sample
                } else {
                    sample.reversed()
                }
            });
        }
    }
    link
}

/// Disjoint sets over indexes, for merging the segments.
struct UnionFind(Vec<usize>);

impl UnionFind {
    fn new(len: usize) -> Self {
        Self((0..len).collect())
    }

    fn find(&mut self, index: usize) -> usize {
        let parent = self.0[index];
        if parent == index {
            return index;
        }
        let root = self.find(parent);
        self.0[index] = root;
        root
    }

    /// Joins two sets under the lower root, so groups are keyed by their first segment.
    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        let (low, high) = (a.min(b), a.max(b));
        self.0[high] = low;
    }
}
