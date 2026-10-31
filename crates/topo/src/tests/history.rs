//! What the history keeps of a build: its shape, without traffic and load.

use uniwrt_model::{facts::WifiIfaceMode, wifi::Band};

use super::scenario::{Node, client_mac, topology};

#[test]
fn the_history_view_drops_traffic_and_load_and_rounds_signals() {
    let nodes = [
        Node::gateway(1),
        Node::ap(2)
            .radio(Band::G5, "phy1-ap0", WifiIfaceMode::Ap, 0x11)
            .station("phy1-ap0", client_mac(5), -61, 100),
    ];
    let topology = topology(&nodes);
    let edge = &topology.edges[0];
    assert!(edge.traffic.is_some());
    let kept = topology.for_history();
    assert_eq!(
        (kept.at, kept.nodes.len(), kept.edges.len()),
        (topology.at, topology.nodes.len(), topology.edges.len())
    );
    assert!(kept.nodes.iter().all(|node| node.cpu_percent.is_none()));
    let kept_edge = &kept.edges[0];
    assert_eq!((&kept_edge.id, kept_edge.up), (&edge.id, edge.up));
    assert_eq!(kept_edge.traffic, None);
    assert_eq!(kept_edge.signal_dbm, Some(-62));
    assert_eq!(kept_edge.quality, edge.quality.map(|q| (q + 2) / 5 * 5));
    assert_eq!(kept.for_history(), kept, "stable");
}
