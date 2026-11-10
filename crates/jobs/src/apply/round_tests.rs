//! The gateway's rounds, planned on the facts of OpenWrt 24.10 in a container: each round's
//! transaction is confirmed onto what the node holds, and the next is planned from that.

use std::collections::{BTreeMap, BTreeSet};

use uniwrt_model::{
    apply::{PlanAction, Renumbering},
    facts::NodeFacts,
    node::{Node, NodeRole},
    site::{SecretRef, SiteConfig},
};
use uniwrt_node::txn::current::ServiceState;

use super::Round;
use crate::{
    ApplyTiming,
    apply::plan::{
        Part, Report, plan,
        tests::{facts_of, node, site, stock_services},
    },
};

/// Where the controller logs in from before anything moves.
const CONTROLLER: &str = "192.168.1.50";

/// What a gateway holds and reports.
struct Held {
    node: Node,
    facts: NodeFacts,
    files: BTreeMap<String, Option<String>>,
    services: BTreeMap<String, ServiceState>,
}

impl Held {
    fn stock() -> Self {
        Self {
            node: node(1, NodeRole::Gateway, "192.168.1.1"),
            facts: facts_of(NodeRole::Gateway),
            files: BTreeMap::new(),
            services: stock_services(),
        }
    }

    fn plan(&self, site: &SiteConfig, controller: &str) -> Part {
        let secrets: BTreeMap<SecretRef, String> = BTreeMap::new();
        let part = plan(
            site,
            &secrets,
            &Report {
                node: &self.node,
                facts: &self.facts,
                files: &self.files,
                services: &self.services,
                installed: Ok(&BTreeSet::new()),
                controller: controller.parse().ok(),
            },
            &ApplyTiming::default(),
        );
        assert_ne!(
            part.plan.action,
            PlanAction::Refused,
            "{:?}",
            part.plan.issues
        );
        part
    }

    /// Confirms `part`'s transaction.
    fn confirm(&mut self, part: &Part) {
        for (name, text) in &part.files {
            self.files.insert(name.clone(), Some(text.clone()));
        }
        for (name, on) in &part.services {
            let state = if *on {
                ServiceState::On
            } else {
                ServiceState::Off
            };
            self.services.insert(name.clone(), state);
        }
    }

    /// Applies `site` round by round until nothing is left: what each transaction left for later.
    fn apply(&mut self, site: &SiteConfig, controller: &str) -> Vec<Vec<Round>> {
        let mut rounds = Vec::new();
        loop {
            let part = self.plan(site, controller);
            if part.plan.action == PlanAction::Unchanged {
                return rounds;
            }
            self.confirm(&part);
            rounds.push(part.next.clone());
            assert!(rounds.len() < 4, "the rounds never end: {rounds:?}");
        }
    }

    /// The gateway holds both addresses of its LAN, and the controller reaches it at the new one.
    fn bridged(&mut self) {
        self.node.host = "10.0.0.1".to_owned();
        let lan = self
            .facts
            .interfaces
            .iter_mut()
            .find(|iface| iface.name == "lan");
        lan.unwrap().ipv4 = vec![
            "10.0.0.1/24".parse().unwrap(),
            "192.168.1.1/24".parse().unwrap(),
        ];
    }
}

/// `site` with its LAN on 10.0.0.0/24.
fn moved(site: &SiteConfig) -> SiteConfig {
    let mut site = site.clone();
    let lan = site.networks[0].ipv4.as_mut().unwrap();
    lan.address = "10.0.0.1/24".parse().unwrap();
    let dhcp = lan.dhcp.as_mut().unwrap();
    dhcp.start = "10.0.0.100".parse().unwrap();
    dhcp.end = "10.0.0.249".parse().unwrap();
    site
}

fn names(part: &Part) -> Vec<&str> {
    part.files.iter().map(|(name, _)| name.as_str()).collect()
}

fn file<'a>(part: &'a Part, name: &str) -> &'a str {
    let file = part.files.iter().find(|(package, _)| package == name);
    &file
        .unwrap_or_else(|| panic!("no {name} in {:?}", names(part)))
        .1
}

#[test]
fn the_wan_changes_last_when_more_than_it_changes() {
    let mut gw = Held::stock();
    let old = site(&[gw.node.id]);
    // Managed for the first time: everything but the WAN, then the WAN.
    assert_eq!(gw.apply(&old, CONTROLLER), [vec![Round::Wan], vec![]]);

    let mut new = old.clone();
    new.wan.mtu = Some(1492);
    let dhcp = new.networks[0]
        .ipv4
        .as_mut()
        .unwrap()
        .dhcp
        .as_mut()
        .unwrap();
    dhcp.end = "192.168.1.199".parse().unwrap();
    let first = gw.plan(&new, CONTROLLER);
    assert!(first.plan.wan_last && first.plan.renumbering.is_none());
    assert_eq!(first.next, [Round::Wan]);
    assert_eq!(names(&first), ["dhcp"]);
    assert!(!first.uplink);
    // The review shows the whole change.
    let reviewed: Vec<&str> = first
        .plan
        .packages
        .iter()
        .map(|diff| diff.package.as_str())
        .collect();
    assert_eq!(reviewed, ["network", "dhcp"]);

    gw.confirm(&first);
    let wan = gw.plan(&new, CONTROLLER);
    assert!(!wan.plan.wan_last && wan.next.is_empty() && wan.uplink);
    assert_eq!(names(&wan), ["network"]);
    assert!(file(&wan, "network").contains("1492"));
    gw.confirm(&wan);
    assert_eq!(gw.plan(&new, CONTROLLER).plan.action, PlanAction::Unchanged);

    // The WAN alone changes in one transaction.
    new.wan.mtu = Some(1480);
    let alone = gw.plan(&new, CONTROLLER);
    assert!(!alone.plan.wan_last && alone.next.is_empty());
    assert_eq!(names(&alone), ["network"]);
}

#[test]
fn the_controllers_network_keeps_its_old_address_until_the_controller_moved() {
    let mut gw = Held::stock();
    let old = site(&[gw.node.id]);
    gw.apply(&old, CONTROLLER);
    let new = moved(&old);
    let bridge = gw.plan(&new, CONTROLLER);
    assert_eq!(
        bridge.plan.renumbering,
        Some(Renumbering {
            network: "lan".to_owned(),
            from: "192.168.1.0/24".to_owned(),
            to: "10.0.0.0/24".to_owned(),
        })
    );
    assert_eq!(bridge.next, [Round::Settle]);
    assert!(!bridge.plan.wan_last);
    // The review warns about the devices the bounce doesn't reach.
    let warned = bridge.plan.warnings.iter().any(|warning| {
        warning.path == "networks/lan" && warning.message.contains("on Wi-Fi or behind a switch")
    });
    assert!(warned, "{:?}", bridge.plan.warnings);
    assert_eq!(bridge.address, "10.0.0.1".parse().ok());
    assert!(bridge.uplink);
    assert_eq!(bridge.timeout, ApplyTiming::default().timer_uplink);
    // The LAN's ports bounce, not the WAN's.
    assert!(
        !bridge.bounce.is_empty() && !bridge.bounce.iter().any(|port| port == "eth1"),
        "{:?}",
        bridge.bounce
    );
    let network = file(&bridge, "network");
    assert!(
        network.contains("\tlist ipaddr '10.0.0.1/24'\n\tlist ipaddr '192.168.1.1/24'\n"),
        "{network}"
    );
    assert!(file(&bridge, "dhcp").contains("option leasetime '2m'"));

    // A controller on no network of the gateway's (routed) has nothing moving under it.
    let routed = gw.plan(&new, "172.16.0.9");
    assert!(routed.bridge.is_none() && routed.bounce.is_empty() && routed.next.is_empty());
    assert!(file(&routed, "network").contains("\tlist ipaddr '10.0.0.1/24'\n\toption"));

    // Bridged: the controller comes from the new subnet, and the old address goes.
    gw.confirm(&bridge);
    gw.bridged();
    let settle = gw.plan(&new, "10.0.0.50");
    assert!(settle.bridge.is_none() && settle.bounce.is_empty() && settle.next.is_empty());
    assert_eq!(settle.address, None);
    let network = file(&settle, "network");
    assert!(
        network.contains("list ipaddr '10.0.0.1/24'") && !network.contains("192.168.1.1"),
        "{network}"
    );
    let dhcp = file(&settle, "dhcp");
    assert!(
        !dhcp.contains("'2m'") && dhcp.contains("option ip '10.0.0.50'"),
        "{dhcp}"
    );
    gw.confirm(&settle);
    assert_eq!(
        gw.plan(&new, "10.0.0.50").plan.action,
        PlanAction::Unchanged
    );
}

#[test]
fn a_renumbering_with_a_wan_change_bridges_settles_then_changes_the_wan() {
    let mut gw = Held::stock();
    let old = site(&[gw.node.id]);
    gw.apply(&old, CONTROLLER);
    let mut new = moved(&old);
    new.wan.mtu = Some(1492);
    let bridge = gw.plan(&new, CONTROLLER);
    assert_eq!(bridge.next, [Round::Settle, Round::Wan]);
    assert!(bridge.plan.wan_last && bridge.plan.renumbering.is_some());
    assert!(!file(&bridge, "network").contains("1492"));
    gw.confirm(&bridge);
    gw.bridged();
    // The settle leaves the WAN for later; the WAN's round leaves nothing.
    assert_eq!(gw.apply(&new, "10.0.0.50"), [vec![Round::Wan], vec![]]);
    let network = gw.files["network"].as_deref().unwrap();
    assert!(
        network.contains("1492") && !network.contains("192.168.1.1"),
        "{network}"
    );
}
