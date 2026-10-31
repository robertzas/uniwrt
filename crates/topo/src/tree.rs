//! Finishing a build: the tree view (the cheapest path from the roots to every vertex, where
//! batman-adv's best paths, wires and a WAN are cheap and links that are down dear), every edge
//! oriented downstream, and traffic rates against the previous build.

use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, BinaryHeap},
};

use uniwrt_core::Timestamp;

use crate::{
    Medium, TopoEdge, TopoKind, TopoNode, Topology,
    links::{Graph, Link},
    quality::traffic,
};

/// How far one vertex is from the roots, and over which link it is reached.
#[derive(Debug, Clone, Copy)]
struct Reached {
    cost: u32,
    hops: u16,
    via: Option<usize>,
}

/// What crossing a link costs: batman-adv's choice first (it measures), then wires, then
/// Wi-Fi; a link that is down is crossed only where nothing else leads (a gateway whose WAN is
/// down still hangs from the internet).
fn cost(link: &Link, preferred: bool) -> u32 {
    let medium = match link.medium {
        Medium::Wan => 1,
        _ if preferred => 1,
        Medium::Ethernet => 2,
        Medium::Wifi => 3,
        Medium::Mesh => 6,
    };
    if link.up { medium } else { medium + 1_000 }
}

/// A gateway that does not reach the internet is a root of its own, after everything that does.
const GATEWAY_ROOT: u32 = 1_000_000;

impl Graph {
    pub(crate) fn finish(self, at: Timestamp, previous: Option<&Topology>) -> Topology {
        let Self {
            mut nodes,
            links,
            preferred,
            ..
        } = self;
        let links: Vec<(String, Link)> = links.into_iter().collect();
        let reached = reach(&nodes, &links, &preferred);
        let previous: BTreeMap<&str, &TopoEdge> = previous
            .map(|topology| {
                topology
                    .edges
                    .iter()
                    .map(|edge| (edge.id.as_str(), edge))
                    .collect()
            })
            .unwrap_or_default();
        let mut edges = Vec::with_capacity(links.len());
        for (index, (key, link)) in links.iter().enumerate() {
            let reversed = downstream_first(link, index, &reached, &nodes);
            let [a, b] = if reversed { [1, 0] } else { [0, 1] };
            let before = previous
                .get(key.as_str())
                .map(|edge| (*edge, edge.a != link.ends[a]));
            edges.push(TopoEdge {
                id: key.clone(),
                a: link.ends[a].clone(),
                b: link.ends[b].clone(),
                medium: link.medium,
                a_port: link.ports[a].clone(),
                b_port: link.ports[b].clone(),
                up: link.up,
                band: link.band,
                ssid: link.ssid.clone(),
                signal_dbm: link.signal_dbm,
                tq: link.tq,
                capacity_mbps: link.capacity_mbps,
                quality: link.quality,
                traffic: link.sample.map(|sample| {
                    let before =
                        before.and_then(|(edge, flipped)| Some((edge.traffic.as_ref()?, flipped)));
                    traffic(sample, reversed, before)
                }),
            });
        }
        for (id, node) in &mut nodes {
            let Some(reach) = reached.get(id) else {
                continue;
            };
            node.hops = Some(reach.hops);
            if let Some(index) = reach.via {
                let (key, link) = &links[index];
                let parent = if link.ends[0] == *id {
                    &link.ends[1]
                } else {
                    &link.ends[0]
                };
                node.parent = Some(parent.clone());
                node.uplink = Some(key.clone());
            }
        }
        Topology {
            at,
            nodes: nodes.into_values().collect(),
            edges,
        }
    }
}

/// The cheapest way from the roots to every vertex they reach.
fn reach(
    nodes: &BTreeMap<String, TopoNode>,
    links: &[(String, Link)],
    preferred: &BTreeSet<String>,
) -> BTreeMap<String, Reached> {
    let mut adjacent = BTreeMap::<&str, Vec<(usize, &str)>>::new();
    for (index, (_, link)) in links.iter().enumerate() {
        let [a, b] = &link.ends;
        adjacent.entry(a).or_default().push((index, b));
        adjacent.entry(b).or_default().push((index, a));
    }
    let roots = nodes.values().filter_map(|node| match node.kind {
        TopoKind::Internet => Some((0, node.id.clone())),
        TopoKind::Gateway => Some((GATEWAY_ROOT, node.id.clone())),
        _ => None,
    });
    let mut reached = BTreeMap::<String, Reached>::new();
    let mut queue = BinaryHeap::new();
    for (cost, root) in roots {
        let via = None;
        reached.insert(root.clone(), Reached { cost, hops: 0, via });
        queue.push(Reverse((cost, root)));
    }
    while let Some(Reverse((cost_so_far, vertex))) = queue.pop() {
        let here = reached[&vertex];
        // Stale entries, and clients, which relay nothing.
        if here.cost < cost_so_far
            || nodes
                .get(&vertex)
                .is_some_and(|node| node.kind == TopoKind::Client)
        {
            continue;
        }
        for &(index, other) in adjacent.get(vertex.as_str()).into_iter().flatten() {
            let (key, link) = &links[index];
            let next = here.cost + cost(link, preferred.contains(key));
            if reached.get(other).is_none_or(|known| next < known.cost) {
                let hops = here.hops.saturating_add(1);
                let via = Some(index);
                reached.insert(
                    other.to_owned(),
                    Reached {
                        cost: next,
                        hops,
                        via,
                    },
                );
                queue.push(Reverse((next, other.to_owned())));
            }
        }
    }
    reached
}

/// Whether the link's second end is the upstream one: the one the tree reaches the other
/// through, else the one with fewer hops, else not a client, else the first in byte order.
fn downstream_first(
    link: &Link,
    index: usize,
    reached: &BTreeMap<String, Reached>,
    nodes: &BTreeMap<String, TopoNode>,
) -> bool {
    let [a, b] = &link.ends;
    let reach = |end: &String| reached.get(end).copied();
    if reach(b).is_some_and(|reach| reach.via == Some(index)) {
        return false;
    }
    if reach(a).is_some_and(|reach| reach.via == Some(index)) {
        return true;
    }
    let rank = |end: &String| {
        let hops = reach(end).map_or(u16::MAX, |reach| reach.hops);
        let client = nodes
            .get(end)
            .is_some_and(|node| node.kind == TopoKind::Client);
        (hops, client)
    };
    (rank(b), b) < (rank(a), a)
}
