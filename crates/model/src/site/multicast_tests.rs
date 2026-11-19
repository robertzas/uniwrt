//! Multicast (schema v15, D-136): migration and the mDNS reflection's limit.

use std::collections::BTreeSet;

use serde_json::Value;
use uniwrt_core::NodeId;

use super::{
    ipam,
    migrate::upgrade,
    network::{MDNS_NETWORKS, Purpose},
    node::ManagedPackage,
    validate::{Severity, validate},
};

fn fixture(version: u32) -> Value {
    let text = match version {
        14 => include_str!("../../fixtures/site/v14.json"),
        _ => include_str!("../../fixtures/site/v15.json"),
    };
    serde_json::from_str(text).unwrap()
}

#[test]
fn v15_turns_multicast_to_unicast_on_and_manages_the_repeater_where_dhcp_is() {
    let config = upgrade(fixture(14)).unwrap();
    assert_eq!(config.wlans.len(), 3);
    assert!(config.wlans.iter().all(|w| w.multicast_to_unicast));
    for node in &config.nodes {
        let dhcp = node.managed.contains(&ManagedPackage::Dhcp);
        assert_eq!(node.managed.contains(&ManagedPackage::MdnsRepeater), dhcp);
    }
    assert_eq!(config, upgrade(fixture(15)).unwrap());
}

#[test]
fn mdns_reflection_serves_at_most_five_networks() {
    let mut config = upgrade(fixture(15)).unwrap();
    let mdns_errors = |config: &super::SiteConfig| {
        validate(config, &BTreeSet::from([NodeId::new(1), NodeId::new(2)]))
            .into_iter()
            .filter(|i| i.path == "networks" && i.severity == Severity::Error)
            .filter(|i| i.message.contains("mDNS"))
            .count()
    };
    assert_eq!(mdns_errors(&config), 0);
    for _ in 0..MDNS_NETWORKS {
        let mut network = ipam::network(&config, Purpose::Iot, None).unwrap();
        network.mdns = true;
        config.networks.push(network);
    }
    assert!(config.networks.iter().filter(|n| n.mdns).count() > MDNS_NETWORKS);
    assert_eq!(mdns_errors(&config), 1);
}
