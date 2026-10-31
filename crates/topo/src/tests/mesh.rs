//! The mesh: 802.11s and batman-adv links merged, batman-adv's best paths, peers that are not
//! adopted, and the clients the translation tables place.

use uniwrt_model::{
    facts::{PeerLinkState, WifiIfaceMode},
    wifi::Band,
};

use super::scenario::{Node, client_mac, edges, node_mac, parents, topology};
use crate::Medium;

/// Like the hwsim lab: meshed over 5 GHz and a wired backhaul (`bh0`), batman-adv over both.
fn meshed(id: u8, other: u8, best: &str) -> Node {
    let (mesh, wire) = (node_mac(other, 0x20), node_mac(other, 0x30));
    let node = Node::new(
        id,
        if id == 1 {
            uniwrt_model::node::NodeRole::Gateway
        } else {
            uniwrt_model::node::NodeRole::Ap
        },
    )
    .radio(Band::G5, "phy2-mesh0", WifiIfaceMode::Mesh, 0x20)
    .port("bh0", 0x30)
    .peer_link(
        "phy2-mesh0",
        mesh,
        if id == 1 { -58 } else { -62 },
        PeerLinkState::Established,
    )
    .neighbor("phy2-mesh0", mesh, if id == 1 { 200 } else { 190 })
    .neighbor("bh0", wire, 250);
    match best {
        "mesh" => node.route(node_mac(other, 0xb0), mesh, "phy2-mesh0", 190, true),
        "wire" => node.route(node_mac(other, 0xb0), wire, "bh0", 250, true),
        _ => node,
    }
}

#[test]
fn both_ends_of_a_mesh_link_and_its_batman_neighbor_are_one_edge() {
    let topology = topology(&[meshed(1, 2, ""), meshed(2, 1, "wire")]);
    assert_eq!(
        edges(&topology),
        ["node:1 > node:2 (Ethernet)", "node:1 > node:2 (Mesh)"]
    );
    let mesh = topology
        .edges
        .iter()
        .find(|edge| edge.medium == Medium::Mesh)
        .unwrap();
    assert_eq!(mesh.id, "node:1~node:2~mesh~5g");
    assert_eq!(
        (mesh.a_port.as_deref(), mesh.b_port.as_deref()),
        (Some("phy2-mesh0"), Some("phy2-mesh0"))
    );
    assert_eq!(
        (mesh.band, mesh.signal_dbm, mesh.tq),
        (Some(Band::G5), Some(-62), Some(190))
    );
    assert_eq!(
        mesh.quality,
        Some(70),
        "the weaker of the signal's (70) and the TQ's (74)"
    );
    let ap = topology
        .nodes
        .iter()
        .find(|node| node.id == "node:2")
        .unwrap();
    assert_eq!(ap.uplink.as_deref(), Some("node:1~node:2~ethernet"));
}

#[test]
fn batman_choosing_the_mesh_over_a_wire_wins_the_tree() {
    let topology = topology(&[meshed(1, 2, ""), meshed(2, 1, "mesh")]);
    let ap = topology
        .nodes
        .iter()
        .find(|node| node.id == "node:2")
        .unwrap();
    assert_eq!(ap.uplink.as_deref(), Some("node:1~node:2~mesh~5g"));
}

#[test]
fn a_node_out_of_the_gateways_reach_hangs_from_the_neighbor_batman_routes_through() {
    let mac = |node| node_mac(node, 0x20);
    let mesh = |node: Node| node.radio(Band::G5, "phy2-mesh0", WifiIfaceMode::Mesh, 0x20);
    let nodes = [
        mesh(Node::gateway(1))
            .neighbor("phy2-mesh0", mac(2), 230)
            .neighbor("phy2-mesh0", mac(3), 60),
        mesh(Node::ap(2))
            .neighbor("phy2-mesh0", mac(1), 230)
            .neighbor("phy2-mesh0", mac(3), 220)
            .route(node_mac(1, 0xb0), mac(1), "phy2-mesh0", 230, true),
        mesh(Node::ap(3))
            .neighbor("phy2-mesh0", mac(2), 220)
            .neighbor("phy2-mesh0", mac(1), 60)
            .route(node_mac(1, 0xb0), mac(2), "phy2-mesh0", 190, true),
    ];
    let topology = topology(&nodes);
    assert_eq!(
        edges(&topology),
        [
            "node:1 > node:2 (Mesh)",
            "node:1 > node:3 (Mesh)",
            "node:2 > node:3 (Mesh)",
        ]
    );
    assert_eq!(
        parents(&topology),
        [
            "internet < -",
            "node:1 < -",
            "node:2 < node:1",
            "node:3 < node:2"
        ]
    );
    let weak = topology
        .edges
        .iter()
        .find(|edge| edge.id == "node:1~node:3~mesh~5g")
        .unwrap();
    assert_eq!((weak.tq, weak.quality), (Some(60), Some(23)));
}

#[test]
fn a_mesh_neighbor_that_is_not_adopted_is_a_peer_with_its_clients() {
    let stranger = node_mac(9, 0x20);
    let nodes = [Node::gateway(1)
        .radio(Band::G5, "phy2-mesh0", WifiIfaceMode::Mesh, 0x20)
        .neighbor("phy2-mesh0", stranger, 150)
        .remote_client(client_mac(7), stranger)
        .lease(stranger, "192.168.1.9", "OpenWrt")];
    let topology = topology(&nodes);
    assert_eq!(
        edges(&topology),
        [
            "node:1 > peer:02:00:00:00:09:20 (Mesh)",
            "peer:02:00:00:00:09:20 > client:0a:00:00:00:00:07 (Wifi)",
        ]
    );
    let peer = topology
        .nodes
        .iter()
        .find(|node| node.id.starts_with("peer:"))
        .unwrap();
    assert_eq!(peer.name.as_deref(), Some("OpenWrt"));
}

#[test]
fn a_peer_link_that_is_not_established_shows_only_to_adopted_nodes() {
    let mesh = |node: Node| node.radio(Band::G5, "phy2-mesh0", WifiIfaceMode::Mesh, 0x20);
    let nodes = [
        mesh(Node::gateway(1))
            .peer_link("phy2-mesh0", node_mac(2, 0x20), -85, PeerLinkState::Holding)
            .peer_link(
                "phy2-mesh0",
                node_mac(9, 0x20),
                -85,
                PeerLinkState::OpenSent,
            ),
        mesh(Node::ap(2)),
    ];
    assert_eq!(edges(&topology(&nodes)), ["node:1 > node:2 (Mesh, down)"]);
}

#[test]
fn translation_tables_place_clients_no_station_list_or_bridge_did() {
    let nodes = [
        Node::gateway(1)
            .radio(Band::G5, "phy2-mesh0", WifiIfaceMode::Mesh, 0x20)
            .neighbor("phy2-mesh0", node_mac(2, 0x20), 200),
        Node::ap(2)
            .radio(Band::G5, "phy2-mesh0", WifiIfaceMode::Mesh, 0x20)
            .neighbor("phy2-mesh0", node_mac(1, 0x20), 200)
            .local_client(client_mac(8), false)
            .local_client(client_mac(9), true)
            .local_client(node_mac(1, 0), false),
    ];
    assert_eq!(
        edges(&topology(&nodes)),
        [
            "node:1 > node:2 (Mesh)",
            "node:2 > client:0a:00:00:00:00:08 (Ethernet)",
            "node:2 > client:0a:00:00:00:00:09 (Wifi)",
        ]
    );
}
