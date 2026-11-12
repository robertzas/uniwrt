//! Health on topologies built by hand: what a build's tree and links look like once finished.

use uniwrt_core::Timestamp;
use uniwrt_model::status::NodeState;

use super::{score, throughput_score};
use crate::{
    DROP_WINDOW_MS, LinkFactor, LinkWatch, Medium, TopoEdge, TopoKind, TopoNode, Topology,
};

fn vertex(id: &str, kind: TopoKind, parent: Option<(&str, Medium)>) -> TopoNode {
    TopoNode {
        id: id.to_owned(),
        kind,
        node: None,
        mac: None,
        name: None,
        ips: Vec::new(),
        state: matches!(kind, TopoKind::Gateway | TopoKind::Ap).then_some(NodeState::Online),
        cpu_percent: None,
        memory_percent: None,
        parent: parent.map(|(parent, _)| parent.to_owned()),
        uplink: parent.map(|(parent, medium)| link_id(parent, id, medium)),
        hops: None,
        gateway_hops: None,
        wireless_hops: None,
        depends_on: None,
    }
}

fn link_id(a: &str, b: &str, medium: Medium) -> String {
    crate::links::key(a, b, medium, None)
}

fn edge(a: &str, b: &str, medium: Medium) -> TopoEdge {
    TopoEdge {
        id: link_id(a, b, medium),
        a: a.to_owned(),
        b: b.to_owned(),
        medium,
        a_port: None,
        b_port: None,
        up: true,
        band: None,
        ssid: None,
        signal_dbm: None,
        tq: None,
        capacity_mbps: None,
        quality: None,
        traffic: None,
        score: None,
        weakest: None,
        drops: Vec::new(),
        critical: false,
    }
}

/// internet ─ gw ═ switch ═ ap1 ~ ap2, ap1 ~ ap3 ~ ap2, a client on ap2, and a mesh link from
/// the gateway to ap2 that is down.
fn site() -> Topology {
    use Medium::{Ethernet, Mesh, Wan, Wifi};
    let mut nodes = vec![
        vertex("internet", TopoKind::Internet, None),
        vertex("gw", TopoKind::Gateway, Some(("internet", Wan))),
        vertex("sw", TopoKind::Switch, Some(("gw", Ethernet))),
        vertex("ap1", TopoKind::Ap, Some(("sw", Ethernet))),
        vertex("ap2", TopoKind::Ap, Some(("ap1", Mesh))),
        vertex("ap3", TopoKind::Ap, Some(("ap1", Mesh))),
        vertex("client", TopoKind::Client, Some(("ap2", Wifi))),
    ];
    nodes.sort_by(|a, b| a.id.cmp(&b.id));
    let mut down = edge("gw", "ap2", Mesh);
    down.up = false;
    let mut edges = vec![
        edge("internet", "gw", Wan),
        edge("gw", "sw", Ethernet),
        edge("sw", "ap1", Ethernet),
        edge("ap1", "ap2", Mesh),
        edge("ap1", "ap3", Mesh),
        edge("ap3", "ap2", Mesh),
        edge("ap2", "client", Wifi),
        down,
    ];
    edges.sort_by(|a, b| a.id.cmp(&b.id));
    let mut topology = Topology {
        at: Timestamp::from_unix_ms(0),
        nodes,
        edges,
    };
    topology.assess();
    topology
}

fn node<'a>(topology: &'a Topology, id: &str) -> &'a TopoNode {
    topology.nodes.iter().find(|node| node.id == id).unwrap()
}

fn link<'a>(topology: &'a mut Topology, a: &str, b: &str, medium: Medium) -> &'a mut TopoEdge {
    let id = link_id(a, b, medium);
    topology
        .edges
        .iter_mut()
        .find(|edge| edge.id == id)
        .unwrap()
}

#[test]
fn the_weakest_factor_sets_the_score() {
    let mut mesh = edge("a", "b", Medium::Mesh);
    (mesh.signal_dbm, mesh.tq, mesh.capacity_mbps) = (Some(-70), Some(200), Some(100));
    assert_eq!(score(&mesh), (50, Some(LinkFactor::Signal)));
    mesh.signal_dbm = Some(-55);
    assert_eq!(score(&mesh), (68, Some(LinkFactor::Throughput)));
    mesh.drops = vec![Timestamp::from_unix_ms(0); 3];
    assert_eq!(score(&mesh), (40, Some(LinkFactor::Flapping)));
    mesh.up = false;
    assert_eq!(score(&mesh), (0, Some(LinkFactor::Down)));
    // A cable: its speed counts only below 100 Mbit/s, and a signal never.
    let mut cable = edge("a", "b", Medium::Ethernet);
    (cable.capacity_mbps, cable.signal_dbm) = (Some(1000), Some(-90));
    assert_eq!(score(&cable), (100, None));
    cable.capacity_mbps = Some(10);
    assert_eq!(score(&cable), (0, Some(LinkFactor::Throughput)));
    let scale: Vec<_> = [5, 10, 50, 100, 300, 1000]
        .into_iter()
        .map(throughput_score)
        .collect();
    assert_eq!(scale, [0, 0, 47, 68, 100, 100]);
}

#[test]
fn backhaul_links_are_scored_and_the_wan_and_clients_not() {
    let topology = site();
    let scored: Vec<_> = topology
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge.score, edge.weakest))
        .collect();
    assert_eq!(
        scored,
        [
            ("ap1~ap2~mesh", Some(100), None),
            ("ap1~ap3~mesh", Some(100), None),
            ("ap1~sw~ethernet", Some(100), None),
            ("ap2~ap3~mesh", Some(100), None),
            ("ap2~client~wifi", None, None),
            ("ap2~gw~mesh", Some(0), Some(LinkFactor::Down)),
            ("gw~internet~wan", None, None),
            ("gw~sw~ethernet", Some(100), None),
        ]
    );
}

#[test]
fn hops_count_a_cable_through_a_switch_once_and_the_air_apart() {
    let topology = site();
    let hops = |id| {
        let node = node(&topology, id);
        (node.gateway_hops, node.wireless_hops)
    };
    assert_eq!(hops("gw"), (Some(0), Some(0)));
    assert_eq!(hops("sw"), (Some(1), Some(0)));
    assert_eq!(hops("ap1"), (Some(1), Some(0)));
    assert_eq!(hops("ap2"), (Some(2), Some(1)));
    assert_eq!(hops("internet"), (None, None));
    assert_eq!(hops("client"), (None, None));
}

#[test]
fn what_each_vertex_cant_lose() {
    let mut topology = site();
    let depends = |topology: &Topology, id| node(topology, id).depends_on.clone();
    assert_eq!(depends(&topology, "sw"), None, "the gateway itself");
    assert_eq!(depends(&topology, "ap1").as_deref(), Some("sw"));
    assert_eq!(depends(&topology, "ap2").as_deref(), Some("ap1"));
    assert_eq!(depends(&topology, "ap3").as_deref(), Some("ap1"));
    assert_eq!(depends(&topology, "client"), None, "clients aren't asked");
    let critical: Vec<_> = topology
        .edges
        .iter()
        .filter(|edge| edge.critical)
        .map(|edge| edge.id.as_str())
        .collect();
    assert_eq!(critical, ["ap1~sw~ethernet", "gw~sw~ethernet"]);
    // The gateway's mesh link to ap2 comes up: a ring, so nothing hangs on one vertex.
    link(&mut topology, "gw", "ap2", Medium::Mesh).up = true;
    for edge in &mut topology.edges {
        edge.critical = false;
    }
    topology.assess();
    assert_eq!(depends(&topology, "ap2"), None);
    assert_eq!(depends(&topology, "ap3"), None);
    assert_eq!(depends(&topology, "ap1"), None, "through ap2 too now");
    assert!(topology.edges.iter().all(|edge| !edge.critical));
}

#[test]
fn parallel_links_back_each_other_up() {
    let mut topology = Topology {
        at: Timestamp::from_unix_ms(0),
        nodes: vec![
            vertex("ap", TopoKind::Ap, Some(("gw", Medium::Ethernet))),
            vertex("gw", TopoKind::Gateway, None),
        ],
        edges: vec![
            edge("ap", "gw", Medium::Ethernet),
            edge("ap", "gw", Medium::Mesh),
        ],
    };
    topology.assess();
    assert!(topology.edges.iter().all(|edge| !edge.critical));
    topology.edges[1].up = false;
    topology.assess();
    assert!(topology.edges[0].critical);
    assert_eq!(topology.nodes[0].depends_on, None);
}

/// The site at `minute`, with ap1 ~ ap2 up or not, or gone.
fn at(minute: i64, ap1_ap2: Option<bool>, ap1_state: NodeState) -> Topology {
    let mut topology = site();
    topology.at = Timestamp::from_unix_ms(minute * 60_000);
    let id = link_id("ap1", "ap2", Medium::Mesh);
    match ap1_ap2 {
        Some(up) => link(&mut topology, "ap1", "ap2", Medium::Mesh).up = up,
        None => topology.edges.retain(|edge| edge.id != id),
    }
    for node in &mut topology.nodes {
        if node.id == "ap1" {
            node.state = Some(ap1_state);
        }
    }
    topology.assess();
    topology
}

fn watched(
    watch: &mut LinkWatch,
    mut topology: Topology,
) -> Option<(usize, u8, Option<LinkFactor>)> {
    watch.observe(&mut topology);
    let edge = topology
        .edges
        .iter()
        .find(|edge| edge.id == link_id("ap1", "ap2", Medium::Mesh))?;
    Some((edge.drops.len(), edge.score?, edge.weakest))
}

#[test]
fn drops_while_both_ends_are_online_make_a_link_flap() {
    use NodeState::{Offline, Online};
    let mut watch = LinkWatch::default();
    assert_eq!(
        watched(&mut watch, at(0, Some(true), Online)),
        Some((0, 100, None))
    );
    let down = Some((1, 0, Some(LinkFactor::Down)));
    assert_eq!(watched(&mut watch, at(1, Some(false), Online)), down);
    assert_eq!(
        watched(&mut watch, at(2, Some(true), Online)),
        Some((1, 100, None))
    );
    // Vanished with both ends there: a drop too.
    assert_eq!(watched(&mut watch, at(3, None, Online)), None);
    let flapping = Some((2, 70, Some(LinkFactor::Flapping)));
    assert_eq!(watched(&mut watch, at(4, Some(true), Online)), flapping);
    // Gone with ap1 offline: not the link's fault.
    assert_eq!(watched(&mut watch, at(5, None, Offline)), None);
    assert_eq!(watched(&mut watch, at(6, Some(true), Online)), flapping);
    // The first drop ages out after the window, then the second.
    let window = DROP_WINDOW_MS / 60_000;
    let one = Some((1, 100, None));
    assert_eq!(watched(&mut watch, at(1 + window, Some(true), Online)), one);
    assert_eq!(watched(&mut watch, at(3 + window, None, Offline)), None);
    let id = link_id("ap1", "ap2", Medium::Mesh);
    assert!(!watch.links.contains_key(&id), "nothing left to remember");
}
