//! The demo's plain cable through the node crate's parsers and the topology: bridged at both
//! ends it is a wired link; the hard interfaces an apply adds on its VLAN carry batman-adv once
//! both ends have them, and the `mtu` probe passes where both ends take 1532 bytes.

use std::{collections::BTreeSet, fmt::Write as _};

use uniwrt_core::{NodeId, Timestamp};
use uniwrt_model::{facts::NodeFacts, node::NodeRole, status::NodeState};
use uniwrt_node::{
    facts,
    proto::ops::{Collected, Set},
};
use uniwrt_topo::{Medium, Member};

use super::collect;
use crate::{
    SimConfig,
    world::{
        World,
        cables::{Wiring, link_local},
        catalog::AP_CABLE,
    },
};

const START: Timestamp = Timestamp::from_unix_ms(1_793_245_539_000);
const VID: u16 = 4094;

fn facts_of(world: &World, node: usize) -> NodeFacts {
    let files = super::config::files(world, node);
    let raw = collect(
        world,
        node,
        &Set::ALL,
        &files,
        &BTreeSet::new(),
        super::wpad::MESH,
    );
    let collected: Collected = serde_json::from_value(raw).expect("a collect answer");
    let mut facts = NodeFacts::default();
    let errors = facts::apply(&collected, &mut facts);
    assert_eq!(errors, Vec::new());
    facts
}

/// The world with each node wired as its hand config says, as the fleet does.
fn world() -> World {
    let mut world = World::new(&SimConfig::default(), START);
    for n in 0..world.nodes.len() {
        let network = super::config::files(&world, n).remove("network");
        world.nodes[n].wiring = Wiring::of(network.as_deref());
    }
    world
}

/// Node `n`'s hand config with the backhaul on `port`: as an overlay (still bridged) or alone,
/// at `mtu` bytes.
fn backhaul(world: &mut World, n: usize, port: &str, alone: bool, mtu: u32) {
    let mut network = super::config::files(world, n)
        .remove("network")
        .unwrap_or_default();
    if alone {
        network = network.replacen(&format!("\tlist ports '{port}'\n"), "", 1);
    }
    let _ = write!(
        network,
        "\nconfig device\n\toption name '{port}'\n\toption mtu '{mtu}'\n\
         \nconfig device\n\toption type '8021q'\n\toption ifname '{port}'\n\
         \toption vid '{VID}'\n\toption name '{port}.{VID}'\n\toption mtu '{mtu}'\n\
         \nconfig interface 'uw_bh_{port}'\n\toption proto 'batadv_hardif'\n\
         \toption master 'bat0'\n\toption device '{port}.{VID}'\n"
    );
    world.nodes[n].wiring = Wiring::of(Some(&network));
}

/// The topology's Ethernet edges between nodes, as (node, port) pairs.
fn cables(world: &World) -> Vec<[(usize, String); 2]> {
    let facts: Vec<_> = (0..world.nodes.len()).map(|n| facts_of(world, n)).collect();
    let members: Vec<_> = world
        .nodes
        .iter()
        .zip(&facts)
        .enumerate()
        .map(|(n, (node, facts))| Member {
            id: NodeId::new(n as i64 + 1),
            role: if node.gateway {
                NodeRole::Gateway
            } else {
                NodeRole::Ap
            },
            mac: node.mac,
            state: NodeState::Online,
            facts: Some(facts),
        })
        .collect();
    let at = |vertex: &str| -> Option<usize> {
        let id: usize = vertex.strip_prefix("node:")?.parse().ok()?;
        Some(id - 1)
    };
    uniwrt_topo::build(&members, None)
        .edges
        .into_iter()
        .filter(|edge| edge.medium == Medium::Ethernet)
        .filter_map(|edge| {
            let mut ends = [(at(&edge.a)?, edge.a_port?), (at(&edge.b)?, edge.b_port?)];
            ends.sort();
            Some(ends)
        })
        .collect()
}

fn neighbors_on(world: &World, n: usize, dev: &str) -> usize {
    let facts = facts_of(world, n);
    let mesh = facts.batman.iter().flat_map(|mesh| &mesh.neighbors);
    mesh.filter(|neighbor| neighbor.hard_if == dev).count()
}

const A: (usize, &str) = AP_CABLE[0];
const B: (usize, &str) = AP_CABLE[1];

fn plain() -> [(usize, String); 2] {
    [(A.0, A.1.to_owned()), (B.0, B.1.to_owned())]
}

#[test]
fn the_plain_cable_is_a_wired_link_between_two_access_points() {
    let world = world();
    let a = facts_of(&world, A.0);
    let b = facts_of(&world, B.0);
    let learned = |facts: &NodeFacts, port: &str, n: usize| {
        facts
            .fdb
            .iter()
            .any(|e| e.port == port && e.mac == world.nodes[n].mac)
    };
    assert!(learned(&a, A.1, B.0) && learned(&b, B.1, A.0));
    assert!(cables(&world).contains(&plain()), "{:?}", cables(&world));
    assert_eq!(neighbors_on(&world, A.0, &format!("{}.{VID}", A.1)), 0);
}

#[test]
fn the_cable_follows_each_end_s_network() {
    let mut world = world();
    let (a_dev, b_dev) = (format!("{}.{VID}", A.1), format!("{}.{VID}", B.1));

    // One end over: no neighbor, and nothing answers the probe.
    backhaul(&mut world, A.0, A.1, false, 1532);
    assert_eq!(neighbors_on(&world, A.0, &a_dev), 0);
    assert_eq!(world.probe(A.0, &a_dev, 1532), Some((1532, Vec::new())));
    assert_eq!(world.probe(B.0, &b_dev, 1532), None, "no such device");

    // Both as overlays: batman-adv hears the far end, the probe passes, the LAN still flows.
    backhaul(&mut world, B.0, B.1, false, 1532);
    assert_eq!(neighbors_on(&world, A.0, &a_dev), 1);
    assert_eq!(neighbors_on(&world, B.0, &b_dev), 1);
    let b_ll = link_local(world.nodes[B.0].port_mac(B.1));
    assert_eq!(world.probe(A.0, &a_dev, 1532), Some((1532, vec![b_ll])));
    let a_ll = link_local(world.nodes[A.0].port_mac(A.1));
    assert_eq!(world.probe(B.0, &b_dev, 1532), Some((1532, vec![a_ll])));
    // One edge: batman-adv's view names the VLAN devices (the apply counts them as the ports).
    let over_vlan = [(A.0, a_dev.clone()), (B.0, b_dev.clone())];
    let found = cables(&world);
    assert!(
        found.contains(&over_vlan) && !found.contains(&plain()),
        "{found:?}"
    );

    // Alone at both ends: no longer bridged, batman-adv's link is the cable.
    backhaul(&mut world, A.0, A.1, true, 1532);
    backhaul(&mut world, B.0, B.1, true, 1532);
    assert!(cables(&world).contains(&over_vlan));
    assert!(!facts_of(&world, A.0).fdb.iter().any(|e| e.port == A.1));

    // A far end that takes 1500 bytes doesn't answer 1532.
    backhaul(&mut world, B.0, B.1, false, 1500);
    assert_eq!(world.probe(A.0, &a_dev, 1532), Some((1532, Vec::new())));
    assert_eq!(world.probe(B.0, &b_dev, 1532), Some((1500, Vec::new())));
}
