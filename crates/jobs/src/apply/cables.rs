//! Which cables an apply brings over to the wired backhaul (features/mesh.md, Wired backhaul;
//! D-094, D-097), pure: the direct Ethernet links between two managed nodes that the topology
//! sees, on ports render accepts at both ends, and the backhauls the nodes hold. A cable a node
//! holds alone (exclusive) stays so; any other comes over as an overlay first, and the apply's
//! backhaul round probes it. A cable that failed the probe stays a trunk while the same two
//! ports face each other.

use std::collections::{BTreeMap, BTreeSet};

use uniwrt_core::NodeId;
use uniwrt_model::{
    apply::{BackhaulFailure, CableEnd},
    facts::NodeFacts,
    node::Node,
    site::SiteConfig,
};
use uniwrt_render::{
    Backhaul, BackhaulStage, Target, backhaul_ports, held_backhauls, meshes_over_radio,
};
use uniwrt_topo::{Medium, Topology};
use uniwrt_uci::Package;

/// A managed node in the apply, as the survey read it.
pub(crate) struct Candidate<'a> {
    pub(crate) node: &'a Node,
    pub(crate) facts: &'a NodeFacts,
    /// Its `network` as it holds it, when it parses.
    pub(crate) network: Option<&'a Package>,
}

/// What the apply does with the cables.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Cables {
    /// The backhauls each node's first transaction renders.
    pub(crate) asks: BTreeMap<NodeId, Vec<Backhaul>>,
    /// The cables the backhaul round probes: both ends come over as overlays (or hold it).
    pub(crate) probes: Vec<[CableEnd; 2]>,
    /// What the review says about them, by node.
    pub(crate) notes: BTreeMap<NodeId, Vec<String>>,
}

/// A topology port as the node's Ethernet port: batman-adv's hard interface on a VLAN,
/// `<port>.<vid>`, is the port's.
fn physical(port: &str) -> &str {
    port.rsplit_once('.')
        .filter(|(_, vid)| vid.parse::<u16>().is_ok())
        .map_or(port, |(port, _)| port)
}

fn node_id(vertex: &str) -> Option<NodeId> {
    vertex.strip_prefix("node:")?.parse().ok().map(NodeId::new)
}

pub(crate) fn find(
    site: &SiteConfig,
    topology: &Topology,
    candidates: &[Candidate<'_>],
    failures: &[BackhaulFailure],
) -> Cables {
    let by_id: BTreeMap<NodeId, &Candidate<'_>> =
        candidates.iter().map(|c| (c.node.id, c)).collect();
    let held: BTreeMap<NodeId, Vec<Backhaul>> = candidates
        .iter()
        .map(|c| (c.node.id, c.network.map(held_backhauls).unwrap_or_default()))
        .collect();
    let radio = meshing_over_radio(site, candidates);
    let cables = direct(topology, &by_id);
    let eligible = eligible(site, &by_id, &cables, &held);
    let accepts = |end: &CableEnd| {
        eligible
            .get(&end.node)
            .is_some_and(|p| p.contains(end.port.as_str()))
    };
    let holds = |end: &CableEnd, stage: BackhaulStage| {
        held[&end.node]
            .iter()
            .any(|b| b.port == end.port && b.stage == stage)
    };

    let mut found = Cables::default();
    let ask = |found: &mut Cables, end: &CableEnd, stage| {
        found.asks.entry(end.node).or_default().push(Backhaul {
            port: end.port.clone(),
            stage,
        });
    };
    let mut asked = BTreeSet::new();
    for ends in &cables {
        if !ends.iter().all(accepts) || failures.iter().any(|f| f.is(&ends[0], &ends[1])) {
            continue;
        }
        let exclusive = ends.iter().all(|end| holds(end, BackhaulStage::Exclusive));
        for (end, other) in [(&ends[0], &ends[1]), (&ends[1], &ends[0])] {
            asked.insert(end.clone());
            if holds(end, BackhaulStage::Exclusive) {
                ask(&mut found, end, BackhaulStage::Exclusive);
                continue;
            }
            ask(&mut found, end, BackhaulStage::Overlay);
            let peer = &by_id[&other.node].node.name;
            let mut note = format!(
                "{}'s cable to {peer} ({}) carries the networks and the mesh's VLAN for now; it \
                 carries the mesh alone once both ends pass the MTU probe, or goes back to a \
                 plain trunk",
                end.port, other.port
            );
            if radio.contains(&end.node) && radio.contains(&other.node) {
                note = format!(
                    "{note}. Both ends mesh over Wi-Fi too: when the cable carries the mesh \
                     alone, traffic between this node's network and {peer}'s pauses for about \
                     a minute, until batman-adv lets go of the cable's old path"
                );
            }
            found.notes.entry(end.node).or_default().push(note);
        }
        if !exclusive {
            found.probes.push(ends.clone());
        }
    }
    // The backhauls held on no cable found: one held alone stays while render accepts it (its
    // far end may be away; without the mesh on it, the port would cut that end off).
    for (node, backhauls) in &held {
        for backhaul in backhauls {
            let end = CableEnd {
                node: *node,
                port: backhaul.port.clone(),
            };
            if asked.contains(&end) {
                continue;
            }
            if backhaul.stage == BackhaulStage::Exclusive && accepts(&end) {
                ask(&mut found, &end, BackhaulStage::Exclusive);
                continue;
            }
            let why = if backhaul.stage == BackhaulStage::Overlay {
                "no cable to another managed node is seen on it"
            } else {
                "it can't carry the wired backhaul now"
            };
            found.notes.entry(*node).or_default().push(format!(
                "{} goes back to a plain trunk: {why}",
                backhaul.port
            ));
        }
    }
    for asks in found.asks.values_mut() {
        asks.sort_by(|a, b| a.port.cmp(&b.port));
    }
    found
}

/// The candidates whose `bat0` stays bridged for a mesh radio: a cable between two of them
/// pauses the LAN between them for about a minute when it goes exclusive (D-098).
fn meshing_over_radio(site: &SiteConfig, candidates: &[Candidate<'_>]) -> BTreeSet<NodeId> {
    let empty = BTreeMap::new();
    candidates
        .iter()
        .filter(|c| {
            let target = Target {
                node: c.node.id,
                role: c.node.role,
                facts: c.facts,
                current: &empty,
                controller: None,
                backhaul: &[],
            };
            meshes_over_radio(site, target)
        })
        .map(|c| c.node.id)
        .collect()
}

/// The direct cables between two candidates, each once, its ends in order: a port on two links
/// leads to more than one node.
fn direct(topology: &Topology, by_id: &BTreeMap<NodeId, &Candidate<'_>>) -> Vec<[CableEnd; 2]> {
    let mut seen = BTreeSet::new();
    for edge in topology
        .edges
        .iter()
        .filter(|e| e.medium == Medium::Ethernet)
    {
        let (Some(a), Some(b)) = (node_id(&edge.a), node_id(&edge.b)) else {
            continue;
        };
        let (Some(a_port), Some(b_port)) = (&edge.a_port, &edge.b_port) else {
            continue;
        };
        if a == b || !by_id.contains_key(&a) || !by_id.contains_key(&b) {
            continue;
        }
        let mut ends = [
            CableEnd {
                node: a,
                port: physical(a_port).to_owned(),
            },
            CableEnd {
                node: b,
                port: physical(b_port).to_owned(),
            },
        ];
        ends.sort();
        seen.insert(ends);
    }
    let mut uses = BTreeMap::<&CableEnd, usize>::new();
    for end in seen.iter().flatten() {
        *uses.entry(end).or_default() += 1;
    }
    seen.iter()
        .filter(|ends| ends.iter().all(|end| uses[end] == 1))
        .cloned()
        .collect()
}

/// The ports render accepts on each node, of those its cables and backhauls name.
fn eligible<'p>(
    site: &SiteConfig,
    by_id: &BTreeMap<NodeId, &Candidate<'_>>,
    cables: &'p [[CableEnd; 2]],
    held: &'p BTreeMap<NodeId, Vec<Backhaul>>,
) -> BTreeMap<NodeId, BTreeSet<&'p str>> {
    let mut ports = BTreeMap::<NodeId, BTreeSet<&str>>::new();
    for end in cables.iter().flatten() {
        ports.entry(end.node).or_default().insert(&end.port);
    }
    for (node, backhauls) in held {
        let asked = ports.entry(*node).or_default();
        asked.extend(backhauls.iter().map(|b| b.port.as_str()));
    }
    let empty = BTreeMap::new();
    ports
        .iter()
        .map(|(id, asked)| {
            let c = by_id[id];
            let target = Target {
                node: *id,
                role: c.node.role,
                facts: c.facts,
                current: &empty,
                controller: None,
                backhaul: &[],
            };
            (*id, backhaul_ports(site, target, asked.iter().copied()))
        })
        .collect()
}

#[cfg(test)]
#[path = "cables_tests.rs"]
mod tests;
