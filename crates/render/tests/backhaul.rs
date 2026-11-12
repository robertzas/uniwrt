//! The wired backhaul (features/mesh.md, Wired backhaul; D-094): batman-adv over a cable on the
//! backhaul VLAN, beside the networks (overlay) or alone (exclusive), and the cables a node
//! can't give it.

// Tests and their fixtures may panic on what they set up.
#![allow(clippy::unwrap_used)]

mod support;

use std::fmt::Write as _;

use insta::assert_snapshot;
use uniwrt_model::{
    facts::{NodeFacts, Software, WifiFeature},
    node::NodeRole,
    site::{
        SiteConfig,
        node::{ManagedPackage, PortConfig, TaggedNetworks},
    },
};
use uniwrt_render::{
    Backhaul, BackhaulStage, Issue, Rendered, Target, backhaul_ports, held_backhauls, render,
};
use uniwrt_uci::Package;

use support::*;

fn cable(port: &str, stage: BackhaulStage) -> Backhaul {
    Backhaul {
        port: port.to_owned(),
        stage,
    }
}

fn with_backhaul(
    site: &SiteConfig,
    node: (uniwrt_core::NodeId, NodeRole),
    facts: &NodeFacts,
    backhaul: &[Backhaul],
) -> Result<Rendered, Vec<Issue>> {
    render(
        site,
        &secrets(),
        Target {
            node: node.0,
            role: node.1,
            facts,
            current: &stock(),
            controller: None,
            backhaul,
        },
    )
}

fn network(rendered: &Result<Rendered, Vec<Issue>>) -> &Package {
    let rendered = rendered
        .as_ref()
        .unwrap_or_else(|issues| panic!("{issues:?}"));
    &rendered.packages[&ManagedPackage::Network]
}

/// The bridge's ports, as the `network` package lists them.
fn bridge_ports(network: &Package) -> Vec<String> {
    network
        .sections_of("device")
        .find(|d| d.option("name") == Some("br-lan"))
        .and_then(|d| d.get("ports"))
        .map(|ports| ports.items().to_vec())
        .unwrap_or_default()
}

#[test]
fn a_cable_comes_over_beside_the_networks_then_alone() {
    let site = home_site();
    let facts = gateway_facts();
    let rendered = with_backhaul(
        &site,
        (AP, NodeRole::Ap),
        &facts,
        &[
            cable("lan1", BackhaulStage::Overlay),
            cable("eth1", BackhaulStage::Exclusive),
        ],
    );
    let network = network(&rendered);
    let ports = bridge_ports(network);
    assert!(ports.contains(&"lan1".to_owned()), "{ports:?}");
    assert!(!ports.contains(&"eth1".to_owned()), "{ports:?}");
    assert!(ports.contains(&"bat0".to_owned()), "{ports:?}");
    assert_snapshot!("backhaul_overlay_and_exclusive", network.to_file());
}

#[test]
fn a_node_without_a_mesh_radio_meshes_over_its_cable() {
    let site = home_site();
    let mut facts = gateway_facts();
    facts.radios.clear();
    let wired = with_backhaul(
        &site,
        (AP, NodeRole::Ap),
        &facts,
        &[cable("lan1", BackhaulStage::Exclusive)],
    );
    let network = network(&wired);
    assert!(network.section("bat0").is_some());
    assert!(network.section("uw_bh_lan1").is_some());
    assert!(network.section("uw_mesh").is_none());
    assert!(bridge_ports(network).contains(&"bat0".to_owned()));
    // Without the cable, the node has no part in the mesh.
    let alone = with_backhaul(&site, (AP, NodeRole::Ap), &facts, &[]);
    assert!(self::network(&alone).section("bat0").is_none());
}

#[test]
fn cables_the_node_cant_give_the_backhaul_are_refused() {
    let facts = gateway_facts();
    let mut site = home_site();
    let profile = |port: &str, native: Option<&str>, backhaul: bool| PortConfig {
        port: port.to_owned(),
        native: native.map(str::to_owned),
        tagged: TaggedNetworks::All,
        backhaul,
    };
    for config in &mut site.nodes {
        config.ports = vec![
            profile("lan2", Some("lan"), false),
            profile("lan3", Some("iot"), true),
        ];
    }
    let asked = [
        cable("eth1", BackhaulStage::Overlay),
        cable("lan2", BackhaulStage::Overlay),
        cable("lan3", BackhaulStage::Overlay),
        cable("lan9", BackhaulStage::Overlay),
    ];
    let mut all = String::new();
    let _ = writeln!(
        all,
        "## gateway\n{}",
        show(&with_backhaul(
            &site,
            (GATEWAY, NodeRole::Gateway),
            &facts,
            &asked
        ))
    );
    let swconfig = home_site();
    let mut meshing = swconfig_facts();
    meshing.capabilities.software.insert(Software::BatmanAdv);
    meshing.capabilities.wifi.insert(WifiFeature::Mesh);
    let _ = writeln!(
        all,
        "## swconfig\n{}",
        show(&with_backhaul(
            &swconfig,
            (AP, NodeRole::Ap),
            &meshing,
            &[cable("switch0:1", BackhaulStage::Overlay)],
        ))
    );
    let mut off = home_site();
    off.mesh.wired = false;
    let _ = writeln!(
        all,
        "## wired off\n{}",
        show(&with_backhaul(
            &off,
            (AP, NodeRole::Ap),
            &facts,
            &[cable("lan1", BackhaulStage::Overlay)],
        ))
    );
    assert_snapshot!("backhaul_refused", all);
}

#[test]
fn the_gateways_firewall_lets_only_the_probe_in_over_a_cable() {
    let site = home_site();
    let facts = gateway_facts();
    let firewall = |node: (uniwrt_core::NodeId, NodeRole), backhaul: &[Backhaul]| {
        let rendered = with_backhaul(&site, node, &facts, backhaul).unwrap();
        rendered.packages[&ManagedPackage::Firewall].clone()
    };
    let cables = [
        cable("lan1", BackhaulStage::Overlay),
        cable("lan2", BackhaulStage::Exclusive),
    ];
    let gateway = firewall((GATEWAY, NodeRole::Gateway), &cables);
    let zone = gateway
        .sections_of("zone")
        .find(|z| z.option("name") == Some("uw_bh"))
        .unwrap();
    assert_eq!(
        zone.get("network").unwrap().items(),
        ["uw_bh_lan1", "uw_bh_lan2"]
    );
    assert_eq!(zone.option("input"), Some("REJECT"));
    assert_eq!(zone.option("forward"), Some("REJECT"));
    let rule = gateway
        .sections_of("rule")
        .find(|r| r.option("src") == Some("uw_bh"))
        .unwrap();
    assert_eq!(rule.option("family"), Some("ipv6"));
    assert_eq!(
        rule.get("icmp_type").unwrap().items(),
        [
            "echo-request",
            "echo-reply",
            "neighbour-solicitation",
            "neighbour-advertisement"
        ]
    );
    // No forwarding leaves or enters the zone, and without cables there's no zone.
    assert!(
        gateway
            .sections_of("forwarding")
            .all(|f| f.option("src") != Some("uw_bh") && f.option("dest") != Some("uw_bh"))
    );
    let plain = firewall((GATEWAY, NodeRole::Gateway), &[]);
    assert!(
        plain
            .sections_of("zone")
            .all(|z| z.option("name") != Some("uw_bh"))
    );
    // An AP runs no firewall.
    let ap = firewall((AP, NodeRole::Ap), &cables);
    assert_eq!(ap.sections_of("zone").count(), 0);
}

#[test]
fn the_apply_asks_only_for_the_ports_render_accepts() {
    let facts = gateway_facts();
    let mut site = home_site();
    for config in &mut site.nodes {
        config.ports = vec![PortConfig {
            port: "lan2".to_owned(),
            native: Some("lan".to_owned()),
            tagged: TaggedNetworks::All,
            backhaul: false,
        }];
    }
    let current = stock();
    let target = |node, role| Target {
        node,
        role,
        facts: &facts,
        current: &current,
        controller: None,
        backhaul: &[],
    };
    let ports = ["eth1", "lan1", "lan2", "lan9"];
    let eligible = |site: &SiteConfig, node, role| {
        backhaul_ports(site, target(node, role), ports)
            .into_iter()
            .collect::<Vec<_>>()
    };
    assert_eq!(eligible(&site, GATEWAY, NodeRole::Gateway), ["lan1"]);
    assert_eq!(eligible(&site, AP, NodeRole::Ap), ["eth1", "lan1"]);
    // Every port it accepts renders.
    let asked: Vec<Backhaul> = eligible(&site, AP, NodeRole::Ap)
        .into_iter()
        .map(|port| cable(port, BackhaulStage::Overlay))
        .collect();
    network(&with_backhaul(&site, (AP, NodeRole::Ap), &facts, &asked));
    let mut off = site.clone();
    off.mesh.wired = false;
    assert_eq!(eligible(&off, AP, NodeRole::Ap), Vec::<&str>::new());
    let mut bare = facts.clone();
    bare.capabilities.software.remove(&Software::BatmanAdv);
    assert!(
        backhaul_ports(
            &site,
            Target {
                facts: &bare,
                ..target(AP, NodeRole::Ap)
            },
            ports
        )
        .is_empty()
    );
}

#[test]
fn a_nodes_network_tells_how_far_each_cable_came() {
    let current = stock();
    let site = home_site();
    let facts = gateway_facts();
    let rendered = with_backhaul(
        &site,
        (AP, NodeRole::Ap),
        &facts,
        &[
            cable("lan1", BackhaulStage::Overlay),
            cable("eth1", BackhaulStage::Exclusive),
        ],
    );
    assert_eq!(
        held_backhauls(network(&rendered)),
        [
            cable("lan1", BackhaulStage::Overlay),
            cable("eth1", BackhaulStage::Exclusive),
        ]
    );
    assert_eq!(held_backhauls(&current["network"]), []);
}
