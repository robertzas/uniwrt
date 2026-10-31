//! Wired links from the bridges' forwarding databases: cables, daisy chains, unmanaged switches.

use uniwrt_model::{facts::WifiIfaceMode, wifi::Band};

use super::scenario::{Node, client_mac, edges, node_mac, parents, topology};

#[test]
fn a_cable_joins_two_nodes_and_clients_hang_from_the_nearest() {
    let nodes = [
        Node::gateway(1)
            .port("lan1", 1)
            .port("lan2", 2)
            .fdb("lan1", &[client_mac(4)])
            .fdb("lan2", &[node_mac(2, 0), client_mac(5)]),
        Node::ap(2)
            .port("lan", 1)
            .port("lan2", 2)
            .fdb("lan", &[node_mac(1, 0)])
            .fdb("lan2", &[client_mac(5)]),
    ];
    let topology = topology(&nodes);
    assert_eq!(
        edges(&topology),
        [
            "node:1 > client:0a:00:00:00:00:04 (Ethernet)",
            "node:1 > node:2 (Ethernet)",
            "node:2 > client:0a:00:00:00:00:05 (Ethernet)",
        ]
    );
    let cable = topology
        .edges
        .iter()
        .find(|edge| edge.b == "node:2")
        .unwrap();
    assert_eq!(cable.id, "node:1~node:2~ethernet");
    assert_eq!(
        (cable.a_port.as_deref(), cable.b_port.as_deref()),
        (Some("lan2"), Some("lan"))
    );
    assert_eq!(cable.capacity_mbps, Some(1000));
    assert_eq!(
        parents(&topology),
        [
            "client:0a:00:00:00:00:04 < node:1",
            "client:0a:00:00:00:00:05 < node:2",
            "internet < -",
            "node:1 < -",
            "node:2 < node:1",
        ]
    );
}

#[test]
fn nodes_and_clients_sharing_a_segment_hang_from_an_unmanaged_switch() {
    let nodes = [
        Node::gateway(1)
            .port("lan1", 1)
            .fdb("lan1", &[node_mac(2, 0), node_mac(3, 0), client_mac(4)]),
        Node::ap(2)
            .port("lan", 1)
            .fdb("lan", &[node_mac(1, 0), node_mac(3, 0), client_mac(4)]),
        Node::ap(3)
            .port("lan", 1)
            .fdb("lan", &[node_mac(1, 0), node_mac(2, 0), client_mac(4)]),
    ];
    let topology = topology(&nodes);
    assert_eq!(
        edges(&topology),
        [
            "node:1 > switch:1:lan1 (Ethernet)",
            "switch:1:lan1 > client:0a:00:00:00:00:04 (Ethernet)",
            "switch:1:lan1 > node:2 (Ethernet)",
            "switch:1:lan1 > node:3 (Ethernet)",
        ]
    );
    let to_ap = topology
        .edges
        .iter()
        .find(|edge| edge.b == "node:2")
        .unwrap();
    assert_eq!(
        (to_ap.a_port.as_deref(), to_ap.b_port.as_deref()),
        (None, Some("lan"))
    );
    let far_ap = topology
        .nodes
        .iter()
        .find(|node| node.id == "node:3")
        .unwrap();
    assert_eq!(
        (far_ap.parent.as_deref(), far_ap.hops),
        (Some("switch:1:lan1"), Some(2))
    );
}

#[test]
fn a_daisy_chain_is_two_cables_not_a_switch() {
    let nodes = [
        Node::gateway(1)
            .port("lan1", 1)
            .fdb("lan1", &[node_mac(2, 0), node_mac(3, 0)]),
        Node::ap(2)
            .port("lan", 1)
            .port("lan2", 2)
            .fdb("lan", &[node_mac(1, 0)])
            .fdb("lan2", &[node_mac(3, 0)]),
        Node::ap(3)
            .port("lan", 1)
            .fdb("lan", &[node_mac(1, 0), node_mac(2, 0)]),
    ];
    assert_eq!(
        edges(&topology(&nodes)),
        ["node:1 > node:2 (Ethernet)", "node:2 > node:3 (Ethernet)",]
    );
}

#[test]
fn wifi_clients_behind_an_access_point_make_no_switch() {
    let nodes = [
        Node::gateway(1)
            .port("lan2", 2)
            .fdb("lan2", &[node_mac(2, 0), client_mac(5)]),
        Node::ap(2)
            .port("lan", 1)
            .radio(Band::G5, "phy1-ap0", WifiIfaceMode::Ap, 0x11)
            .station("phy1-ap0", client_mac(5), -60, 0)
            .fdb("lan", &[node_mac(1, 0)])
            .fdb("phy1-ap0", &[client_mac(5)]),
    ];
    assert_eq!(
        edges(&topology(&nodes)),
        [
            "node:1 > node:2 (Ethernet)",
            "node:2 > client:0a:00:00:00:00:05 (Wifi)",
        ]
    );
}

#[test]
fn a_port_without_carrier_is_a_link_that_is_down() {
    let mut gateway = Node::gateway(1)
        .port("lan2", 2)
        .fdb("lan2", &[node_mac(2, 0)]);
    gateway.facts.devices[1].carrier = false;
    let nodes = [
        gateway,
        Node::ap(2).port("lan", 1).fdb("lan", &[node_mac(1, 0)]),
    ];
    assert_eq!(
        edges(&topology(&nodes)),
        ["node:1 > node:2 (Ethernet, down)"]
    );
}
