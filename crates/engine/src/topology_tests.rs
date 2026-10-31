//! The topology of the hwsim lab's captures (TESTING.md, Wi-Fi lab): node a (the gateway, 25.12)
//! and node b (an access point, 24.10) meshed over 802.11s and a wired backhaul (`bh0`) with
//! batman-adv over both; client c on a's 2g AP (`…:04:00`) and b's 5g AP (`…:05:00`).

use uniwrt_core::NodeId;
use uniwrt_model::{facts::NodeFacts, node::NodeRole, status::NodeState};
use uniwrt_node::{facts::apply, proto::ops::Collected};
use uniwrt_topo::{Member, build};

const GATEWAY: &str = include_str!("../../node/fixtures/collect/hwsim-a-25.12.6.json");
const AP: &str = include_str!("../../node/fixtures/collect/hwsim-b-24.10.8.json");

fn facts(json: &str) -> NodeFacts {
    let collected: Collected = serde_json::from_str(json).unwrap();
    let mut facts = NodeFacts::default();
    assert_eq!(apply(&collected, &mut facts), []);
    facts
}

fn member(id: i64, role: NodeRole, facts: &NodeFacts) -> Member<'_> {
    let bridge = facts
        .devices
        .iter()
        .find(|device| device.name == "br-lan")
        .unwrap();
    Member {
        id: NodeId::new(id),
        role,
        mac: bridge.mac.unwrap(),
        state: NodeState::Online,
        facts: Some(facts),
    }
}

#[test]
fn the_lab_is_two_nodes_meshed_and_wired_with_the_client_at_both() {
    let (gateway, ap) = (facts(GATEWAY), facts(AP));
    let members = [
        member(1, NodeRole::Gateway, &gateway),
        member(2, NodeRole::Ap, &ap),
    ];
    let topology = build(&members, None);
    let edge = |id: &str| {
        topology
            .edges
            .iter()
            .find(|edge| edge.id == id)
            .unwrap_or_else(|| panic!("no {id}"))
    };
    // The backhaul and the mesh, each one edge from both ends' reports.
    let wire = edge("node:1~node:2~ethernet");
    let ports = (wire.a_port.as_deref(), wire.b_port.as_deref());
    assert_eq!(
        (wire.a.as_str(), ports),
        ("node:1", (Some("bh0"), Some("bh0")))
    );
    assert!(wire.up && wire.tq.is_some());
    let mesh = edge("node:1~node:2~mesh~5g");
    let ports = (mesh.a_port.as_deref(), mesh.b_port.as_deref());
    assert_eq!(ports, (Some("phy2-mesh0"), Some("phy3-mesh0")));
    assert!(mesh.up && mesh.tq.is_some() && mesh.signal_dbm.is_some());
    let node = |id: &str| topology.nodes.iter().find(|node| node.id == id).unwrap();
    assert_eq!(node("node:2").parent.as_deref(), Some("node:1"));
    // The client's two radios, each at its access point; the lease names one of them.
    let client_a = edge("client:02:00:00:00:04:00~node:1~wifi~2g");
    assert_eq!(
        (client_a.a.as_str(), client_a.a_port.as_deref()),
        ("node:1", Some("phy1-ap0"))
    );
    let client_b = edge("client:02:00:00:00:05:00~node:2~wifi~5g");
    assert_eq!(
        (client_b.a.as_str(), client_b.a_port.as_deref()),
        ("node:2", Some("phy3-ap0"))
    );
    assert_eq!(
        node("client:02:00:00:00:05:00").name.as_deref(),
        Some("lab-client")
    );
    // Both containers' eth0 are ports of Docker's bridge, with the host: a switch.
    let switch = "switch:1:eth0";
    for end in ["node:1", "node:2"] {
        let id = format!("{end}~{switch}~ethernet");
        assert_eq!(edge(&id).b, switch, "{end}");
    }
    let host = topology
        .nodes
        .iter()
        .find(|node| node.ips == ["172.17.0.1".parse::<std::net::IpAddr>().unwrap()]);
    assert_eq!(host.and_then(|host| host.parent.as_deref()), Some(switch));
}
