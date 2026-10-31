//! Wi-Fi clients at their access point, and a bridge at the access point it is a station of.

use uniwrt_model::{facts::WifiIfaceMode, node::NodeRole, wifi::Band};

use super::scenario::{Node, client_mac, edges, node_mac, topology};

#[test]
fn a_roaming_client_is_at_the_access_point_it_used_last_named_by_its_lease() {
    let ap = |id| Node::ap(id).radio(Band::G5, "phy1-ap0", WifiIfaceMode::Ap, 0x11);
    let nodes = [
        Node::gateway(1).lease(client_mac(5), "192.168.1.50", "phone"),
        ap(2).station("phy1-ap0", client_mac(5), -80, 5_000),
        ap(3).station("phy1-ap0", client_mac(5), -55, 100),
    ];
    let topology = topology(&nodes);
    assert_eq!(
        edges(&topology),
        ["node:3 > client:0a:00:00:00:00:05 (Wifi)"]
    );
    let client = topology
        .nodes
        .iter()
        .find(|node| node.id.starts_with("client:"))
        .unwrap();
    assert_eq!(client.name.as_deref(), Some("phone"));
    assert_eq!(
        client.ips,
        ["192.168.1.50".parse::<std::net::IpAddr>().unwrap()]
    );
    let edge = &topology.edges[0];
    assert_eq!(edge.id, "client:0a:00:00:00:00:05~node:3~wifi~5g");
    assert_eq!(
        (edge.ssid.as_deref(), edge.band),
        (Some("home"), Some(Band::G5))
    );
    assert_eq!(
        (edge.signal_dbm, edge.quality, edge.capacity_mbps),
        (Some(-55), Some(87), Some(300))
    );
    let traffic = edge.traffic.unwrap();
    assert_eq!(
        (traffic.down_bytes, traffic.up_bytes),
        (2_000, 1_000),
        "sent to the client is down"
    );
}

#[test]
fn a_bridge_is_linked_to_its_access_point_not_a_client() {
    let nodes = [
        Node::ap(2)
            .radio(Band::G5, "phy1-ap0", WifiIfaceMode::Ap, 0x11)
            .station("phy1-ap0", node_mac(3, 0x10), -60, 0),
        Node::new(3, NodeRole::Bridge)
            .radio(Band::G5, "phy1-sta0", WifiIfaceMode::Sta, 0x10)
            .station("phy1-sta0", node_mac(2, 0x11), -61, 0)
            .radio(Band::G2, "phy0-sta0", WifiIfaceMode::Sta, 0x12)
            .station("phy0-sta0", node_mac(9, 0x11), -70, 0),
    ];
    let topology = topology(&nodes);
    assert_eq!(edges(&topology), ["node:2 > node:3 (Wifi)"]);
    assert_eq!(topology.edges[0].signal_dbm, Some(-61));
}
