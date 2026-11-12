//! Which cables an apply brings over: from the topology's direct links between managed nodes,
//! what the nodes hold, and the cables that failed the probe.

use uniwrt_core::{JobId, NodeId, Timestamp};
use uniwrt_model::{
    apply::{BackhaulFailure, CableEnd},
    facts::{NodeFacts, Software},
    node::{Node, NodeRole},
    site::SiteConfig,
};
use uniwrt_render::{Backhaul, BackhaulStage};
use uniwrt_topo::{Medium, TopoEdge, Topology};
use uniwrt_uci::Package;

use super::{Cables, Candidate, find};
use crate::apply::plan::tests::{facts_of, node, site};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const C: NodeId = NodeId::new(3);

fn facts(role: NodeRole) -> NodeFacts {
    let mut facts = facts_of(role);
    facts.capabilities.software.insert(Software::BatmanAdv);
    facts.board.layout.lan = vec!["lan1".to_owned(), "lan2".to_owned()];
    facts
}

fn cable(a: (NodeId, &str), b: (NodeId, &str)) -> TopoEdge {
    TopoEdge {
        id: format!("node:{}~node:{}~ethernet", a.0, b.0),
        a: format!("node:{}", a.0),
        b: format!("node:{}", b.0),
        medium: Medium::Ethernet,
        a_port: Some(a.1.to_owned()),
        b_port: Some(b.1.to_owned()),
        up: true,
        band: None,
        ssid: None,
        signal_dbm: None,
        tq: None,
        capacity_mbps: Some(1000),
        quality: None,
        traffic: None,
    }
}

fn topology(edges: Vec<TopoEdge>) -> Topology {
    Topology {
        at: Timestamp::from_unix_ms(0),
        nodes: Vec::new(),
        edges,
    }
}

/// A `network` holding the backhaul on `port`, in the bridge (an overlay) or out of it.
fn holding(port: &str, bridged: bool) -> Package {
    let ports = if bridged {
        format!("\tlist ports '{port}'\n")
    } else {
        String::new()
    };
    let text = format!(
        "config device\n\toption name 'br-lan'\n\toption type 'bridge'\n\tlist ports 'lan9'\n{ports}\n\
         config interface 'uw_bh_{port}'\n\toption proto 'batadv_hardif'\n\toption master 'bat0'\n\
         \toption device '{port}.4094'\n"
    );
    uniwrt_uci::parse("network", &text).unwrap()
}

struct Fleet {
    nodes: Vec<(Node, NodeFacts, Option<Package>)>,
}

impl Fleet {
    fn new() -> Self {
        Self {
            nodes: vec![
                (
                    node(1, NodeRole::Gateway, "192.168.1.1"),
                    facts(NodeRole::Gateway),
                    None,
                ),
                (
                    node(2, NodeRole::Ap, "192.168.1.2"),
                    facts(NodeRole::Ap),
                    None,
                ),
                (
                    node(3, NodeRole::Ap, "192.168.1.3"),
                    facts(NodeRole::Ap),
                    None,
                ),
            ],
        }
    }

    fn holds(mut self, id: NodeId, network: Package) -> Self {
        self.nodes[usize::try_from(id.get() - 1).unwrap()].2 = Some(network);
        self
    }

    fn find(
        &self,
        site: &SiteConfig,
        edges: Vec<TopoEdge>,
        failures: &[BackhaulFailure],
    ) -> Cables {
        let candidates: Vec<Candidate<'_>> = self
            .nodes
            .iter()
            .map(|(node, facts, network)| Candidate {
                node,
                facts,
                network: network.as_ref(),
            })
            .collect();
        find(site, &topology(edges), &candidates, failures)
    }
}

fn end(node: NodeId, port: &str) -> CableEnd {
    CableEnd {
        node,
        port: port.to_owned(),
    }
}

fn asks(found: &Cables, node: NodeId) -> Vec<(String, BackhaulStage)> {
    found
        .asks
        .get(&node)
        .into_iter()
        .flatten()
        .map(|Backhaul { port, stage }| (port.clone(), *stage))
        .collect()
}

/// The test site with the mesh on, and its wired links (the default).
fn meshed() -> SiteConfig {
    let mut site = site(&[A, B, C]);
    site.mesh.enabled = true;
    site
}

#[test]
fn a_new_cable_comes_over_as_an_overlay_and_is_probed() {
    let site = meshed();
    // batman-adv's link on the VLAN is the same cable as the bridges' one.
    let found = Fleet::new().find(
        &site,
        vec![
            cable((A, "lan1"), (B, "lan2")),
            cable((B, "lan2.4094"), (A, "lan1.4094")),
        ],
        &[],
    );
    assert_eq!(
        asks(&found, A),
        [("lan1".to_owned(), BackhaulStage::Overlay)]
    );
    assert_eq!(
        asks(&found, B),
        [("lan2".to_owned(), BackhaulStage::Overlay)]
    );
    assert_eq!(found.probes, [[end(A, "lan1"), end(B, "lan2")]]);
    assert!(found.notes[&A][0].contains("node-2"), "{:?}", found.notes);
    assert!(!found.asks.contains_key(&C));
}

#[test]
fn a_cable_held_alone_at_both_ends_stays_so_unprobed() {
    let site = meshed();
    let fleet = Fleet::new()
        .holds(A, holding("lan1", false))
        .holds(B, holding("lan2", false));
    let found = fleet.find(&site, vec![cable((A, "lan1.4094"), (B, "lan2.4094"))], &[]);
    assert_eq!(
        asks(&found, A),
        [("lan1".to_owned(), BackhaulStage::Exclusive)]
    );
    assert_eq!(
        asks(&found, B),
        [("lan2".to_owned(), BackhaulStage::Exclusive)]
    );
    assert!(
        found.probes.is_empty() && found.notes.is_empty(),
        "{found:?}"
    );
    // Its far end away (no cable seen): the end that holds it alone keeps it.
    let alone = fleet.find(&site, Vec::new(), &[]);
    assert_eq!(
        asks(&alone, A),
        [("lan1".to_owned(), BackhaulStage::Exclusive)]
    );
}

#[test]
fn a_cable_that_failed_the_probe_stays_a_trunk() {
    let site = meshed();
    let failed = BackhaulFailure {
        ends: [end(B, "lan2"), end(A, "lan1")],
        job: JobId::new(4),
        at: Timestamp::from_unix_ms(0),
        why: "node-2's lan2 takes 1500 bytes".to_owned(),
    };
    let found = Fleet::new().find(
        &site,
        vec![cable((A, "lan1"), (B, "lan2"))],
        std::slice::from_ref(&failed),
    );
    assert_eq!(found, Cables::default());
    // Plugged into another port, it is another cable.
    let moved = Fleet::new().find(&site, vec![cable((A, "lan2"), (B, "lan2"))], &[failed]);
    assert_eq!(moved.probes, [[end(A, "lan2"), end(B, "lan2")]]);
}

#[test]
fn only_direct_cables_between_managed_nodes_on_ports_render_accepts() {
    let mut site = meshed();
    let fleet = Fleet::new();
    let edges = vec![
        // The gateway's WAN port.
        cable((A, "eth1"), (C, "lan1")),
        // A port on two links leads to more than one node.
        cable((B, "lan1"), (C, "lan2")),
        cable((B, "lan1"), (A, "lan2")),
    ];
    assert_eq!(fleet.find(&site, edges, &[]), Cables::default());
    // A node outside the apply (observe mode, unreachable) is no end.
    let unmanaged = cable((B, "lan2"), (NodeId::new(9), "lan1"));
    assert_eq!(fleet.find(&site, vec![unmanaged], &[]), Cables::default());
    site.mesh.wired = false;
    assert_eq!(
        fleet.find(&site, vec![cable((A, "lan1"), (B, "lan2"))], &[]),
        Cables::default()
    );
}

#[test]
fn held_backhauls_without_a_cable_or_once_wired_links_are_off_go_back_to_trunks() {
    let mut site = meshed();
    let fleet = Fleet::new()
        .holds(A, holding("lan1", false))
        .holds(B, holding("lan2", true));
    // B's overlay faces no managed node any more.
    let found = fleet.find(&site, Vec::new(), &[]);
    assert_eq!(asks(&found, B), []);
    assert!(
        found.notes[&B][0].starts_with("lan2 goes back to a plain trunk"),
        "{found:?}"
    );
    site.mesh.wired = false;
    let off = fleet.find(&site, Vec::new(), &[]);
    assert!(off.asks.is_empty(), "{off:?}");
    assert!(off.notes[&A][0].contains("can't carry"), "{off:?}");
}

#[test]
fn a_cable_with_one_end_held_alone_is_probed_again() {
    let site = meshed();
    // A was reset to a trunk (or replaced) while B kept the cable alone.
    let fleet = Fleet::new().holds(B, holding("lan2", false));
    let found = fleet.find(&site, vec![cable((A, "lan1"), (B, "lan2"))], &[]);
    assert_eq!(
        asks(&found, A),
        [("lan1".to_owned(), BackhaulStage::Overlay)]
    );
    assert_eq!(
        asks(&found, B),
        [("lan2".to_owned(), BackhaulStage::Exclusive)]
    );
    assert_eq!(found.probes.len(), 1);
}

/// The sim's wired links run batman-adv on the bare port, as hand config does; the import keeps
/// them the owner's (raw interfaces that claim the port), so an apply leaves them as they are.
#[tokio::test]
async fn brownfield_wired_batman_links_stay_as_they_are() {
    let sim =
        uniwrt_sim::SimFleet::spawn(&uniwrt_sim::SimConfig::default(), &uniwrt_core::SystemClock);
    let found = sim.brownfield();
    let ids: Vec<NodeId> = (1..=found.len())
        .map(|n| NodeId::new(i64::try_from(n).unwrap()))
        .collect();
    let packages: Vec<std::collections::BTreeMap<String, Package>> = found
        .iter()
        .map(|node| {
            node.files
                .iter()
                .map(|(name, text)| (name.clone(), uniwrt_uci::parse(name, text).unwrap()))
                .collect()
        })
        .collect();
    let sources: Vec<uniwrt_import::Source<'_>> = found
        .iter()
        .zip(&ids)
        .zip(&packages)
        .map(|((node, &id), packages)| uniwrt_import::Source {
            node: id,
            role: node.role,
            facts: &node.facts,
            packages,
        })
        .collect();
    let mut n = 0_u128;
    let mut mint = || {
        n += 1;
        uniwrt_model::site::SecretRef::numbered(n)
    };
    let site = uniwrt_import::import(&SiteConfig::defaults(None), &sources, &mut mint).site;
    assert!(site.mesh.enabled && site.mesh.wired);
    let current = std::collections::BTreeMap::new();
    let mut wired = 0;
    for ((node, id), packages) in found.iter().zip(&ids).zip(&packages) {
        let ports: Vec<&str> = packages["network"]
            .sections_of("interface")
            .filter(|s| s.option("proto") == Some("batadv_hardif"))
            .filter_map(|s| s.option("device"))
            .filter(|device| !device.contains("mesh"))
            .collect();
        wired += ports.len();
        let target = uniwrt_render::Target {
            node: *id,
            role: node.role,
            facts: &node.facts,
            current: &current,
            controller: None,
            backhaul: &[],
        };
        let eligible = uniwrt_render::backhaul_ports(&site, target, ports.iter().copied());
        assert!(eligible.is_empty(), "{}: {eligible:?}", node.name);
    }
    assert!(wired >= 2, "the sim has a wired link");
}
