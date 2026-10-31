//! A whole build: the WAN, the fallback to neighbor caches, traffic rates between builds,
//! deltas, and determinism.

use uniwrt_model::{facts::WifiIfaceMode, status::NodeState, wifi::Band};

use super::scenario::{Node, client_mac, edges, node_mac, parents, topology, topology_after};
use crate::{TopoKind, Topology};

fn gateway() -> Node {
    Node::gateway(1)
        .port("eth1", 9)
        .interface("wan", "eth1", true, true)
        .interface("wan6", "eth1", true, false)
        .interface("lan", "br-lan", true, false)
}

#[test]
fn the_gateway_hangs_from_the_internet_by_one_link_per_wan_device() {
    let topology = topology(&[gateway().counters("eth1", 100, 900)]);
    assert_eq!(edges(&topology), ["internet > node:1 (Wan)"]);
    let wan = &topology.edges[0];
    assert_eq!(wan.id, "internet~node:1~wan~eth1");
    assert_eq!(
        (wan.a_port.as_deref(), wan.b_port.as_deref()),
        (None, Some("eth1"))
    );
    let traffic = wan.traffic.unwrap();
    assert_eq!(
        (traffic.down_bytes, traffic.up_bytes),
        (900, 100),
        "received is down"
    );
    assert_eq!(parents(&topology), ["internet < -", "node:1 < internet"]);
    let node = &topology.nodes[1];
    assert_eq!(
        (node.kind, node.name.as_deref(), node.state),
        (TopoKind::Gateway, Some("node1"), Some(NodeState::Online))
    );
}

#[test]
fn a_wan_that_is_down_still_holds_the_gateway() {
    let mut node = gateway();
    node.facts.interfaces[0].up = false;
    let topology = topology(&[node]);
    assert_eq!(edges(&topology), ["internet > node:1 (Wan, down)"]);
    assert_eq!(topology.nodes[1].hops, Some(1));
}

#[test]
fn without_a_gateway_there_is_no_internet_and_no_root() {
    let topology = topology(&[Node::ap(2)]);
    assert_eq!(parents(&topology), ["node:2 < -"]);
    assert_eq!(topology.nodes[0].hops, None);
}

#[test]
fn clients_only_a_neighbor_cache_knows_hang_off_the_device_but_not_upstream() {
    let nodes = [gateway()
        .arp(client_mac(6), "192.168.1.60", "br-lan")
        .arp(node_mac(7, 0), "192.0.2.1", "eth1")
        .lease(client_mac(6), "192.168.1.60", "*")];
    let topology = topology(&nodes);
    assert_eq!(
        edges(&topology),
        [
            "internet > node:1 (Wan)",
            "node:1 > client:0a:00:00:00:00:06 (Ethernet)"
        ]
    );
    let client = topology
        .nodes
        .iter()
        .find(|node| node.kind == TopoKind::Client)
        .unwrap();
    assert_eq!(client.name, None, "`*` is dnsmasq's no name");
}

fn fleet(at: i64, clients: &[u8], bytes: u64) -> [Node; 2] {
    let mut ap = Node::ap(2)
        .at(at)
        .radio(Band::G2, "phy0-ap0", WifiIfaceMode::Ap, 0x10)
        .port("lan", 1)
        .fdb("lan", &[node_mac(1, 0)]);
    for &client in clients {
        ap = ap.station("phy0-ap0", client_mac(client), -60, 0);
    }
    let gateway = gateway()
        .at(at)
        .port("lan2", 2)
        .fdb("lan2", &[node_mac(2, 0)])
        .counters("lan2", bytes, bytes / 2);
    [gateway, ap]
}

#[test]
fn rates_follow_the_counters_from_one_build_to_the_next() {
    let first = topology(&fleet(1_000, &[4], 1_000));
    let second = topology_after(&fleet(3_000, &[4], 501_000), Some(&first));
    assert_eq!(second.at.as_unix_ms(), 3_000);
    let cable = second
        .edges
        .iter()
        .find(|edge| edge.id == "node:1~node:2~ethernet")
        .unwrap();
    let traffic = cable.traffic.unwrap();
    assert_eq!(
        (traffic.down_bps, traffic.up_bps),
        (Some(2_000_000), Some(1_000_000))
    );
}

#[test]
fn a_delta_turns_one_build_into_the_next() {
    let first = topology(&fleet(1_000, &[4, 5], 1_000));
    let second = topology_after(&fleet(3_000, &[5, 6], 1_000), Some(&first));
    let delta = first.diff(&second);
    assert_eq!(delta.removed_nodes, ["client:0a:00:00:00:00:04"]);
    assert_eq!(
        delta.removed_edges,
        ["client:0a:00:00:00:00:04~node:2~wifi~2g"]
    );
    let added: Vec<_> = delta.nodes.iter().map(|node| node.id.as_str()).collect();
    assert_eq!(added, ["client:0a:00:00:00:00:06"]);
    let mut applied = first.clone();
    applied.apply(&delta);
    assert_eq!(applied, second);
    let unchanged = second.diff(&topology_after(
        &fleet(3_000, &[5, 6], 1_000),
        Some(&second),
    ));
    assert!(unchanged.is_empty(), "{unchanged:?}");
}

#[test]
fn the_same_facts_give_the_same_graph_in_any_order() {
    let forward = topology(&fleet(1_000, &[4, 5], 1_000));
    let [gateway, ap] = fleet(1_000, &[5, 4], 1_000);
    let backward: Topology = topology(&[ap, gateway]);
    assert_eq!(forward, backward);
    let json = serde_json::to_string(&forward).unwrap();
    assert_eq!(serde_json::from_str::<Topology>(&json).unwrap(), forward);
}
