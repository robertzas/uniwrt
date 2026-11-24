use std::{collections::BTreeMap, net::Ipv4Addr};

use uniwrt_core::{MacAddr, NodeId};

use super::*;
use crate::{
    site::{
        network::Reservation,
        node::{NodeConfig, PortConfig, RadioOverride, RawSection, TaggedNetworks},
        wireless::{PlannedRadio, TxPower},
    },
    wifi::ChannelWidth,
};

const OLD: MacAddr = MacAddr::new([0x94, 0x83, 0xc4, 0, 0, 3]);
const NEW: MacAddr = MacAddr::new([0x60, 0x38, 0xe0, 0, 0, 9]);
const NODE: NodeId = NodeId::new(3);

fn device(mac: MacAddr, radios: &[(&str, Band)], ports: &[&str]) -> Device {
    Device {
        mac,
        radios: radios.iter().map(|(r, b)| ((*r).to_owned(), *b)).collect(),
        ports: ports.iter().map(|p| (*p).to_owned()).collect(),
    }
}

fn radio(name: &str, channel: Option<u16>) -> RadioOverride {
    RadioOverride {
        radio: name.to_owned(),
        disabled: false,
        channel: channel.map(crate::site::wireless::Channel::Fixed),
        width: None,
        tx_power: None,
    }
}

fn planned(node: NodeId, radio: &str, band: Band, channel: u16) -> PlannedRadio {
    PlannedRadio {
        node,
        radio: radio.to_owned(),
        band,
        channel,
        width: ChannelWidth::Mhz20,
        tx_power: TxPower::Auto,
    }
}

fn port(name: &str) -> PortConfig {
    PortConfig {
        port: name.to_owned(),
        native: Some("lan".to_owned()),
        tagged: TaggedNetworks::None,
        backhaul: false,
    }
}

fn site() -> SiteConfig {
    let mut config = SiteConfig::defaults(None);
    let mut node = NodeConfig::new(NODE);
    node.hostname = Some("hall".to_owned());
    node.radios = vec![
        radio("radio0", Some(6)),
        radio("radio1", Some(36)),
        radio("radio2", Some(149)),
    ];
    node.ports = vec![port("lan1"), port("lan4")];
    node.raw = vec![RawSection {
        package: "network".to_owned(),
        kind: "device".to_owned(),
        name: Some("br_old".to_owned()),
        options: BTreeMap::new(),
        merge: false,
        unset: std::collections::BTreeSet::new(),
        remove: false,
        matching: std::collections::BTreeMap::new(),
    }];
    config.nodes = vec![NodeConfig::new(NodeId::new(2)), node];
    config.radio.plan.radios = vec![
        planned(NodeId::new(2), "radio1", Band::G5, 44),
        planned(NODE, "radio0", Band::G2, 6),
        planned(NODE, "radio1", Band::G5, 36),
        planned(NODE, "radio2", Band::G5, 149),
    ];
    let dhcp = config.networks[0]
        .ipv4
        .as_mut()
        .and_then(|v4| v4.dhcp.as_mut())
        .unwrap();
    dhcp.reservations.push(Reservation {
        mac: OLD,
        ip: Ipv4Addr::new(10, 0, 0, 3),
        name: None,
    });
    config
}

#[test]
fn radios_follow_their_band_ports_their_name_and_the_reservation_the_mac() {
    let mut config = site();
    // Tri-band, swconfig-less: the old one had 2.4 + two 5 GHz radios.
    let old = device(
        OLD,
        &[
            ("radio0", Band::G2),
            ("radio1", Band::G5),
            ("radio2", Band::G5),
        ],
        &["lan1", "lan2", "lan3", "lan4"],
    );
    // The new one lists 5 GHz first and has a single 5 GHz radio and two ports.
    let new = device(
        NEW,
        &[("radio0", Band::G5), ("radio1", Band::G2)],
        &["lan1", "lan2"],
    );
    let replaced = replace_node(&mut config, NODE, &old, &new);

    assert!(replaced.changed);
    let node = config.nodes.iter().find(|n| n.node == NODE).unwrap();
    let radios: Vec<_> = node
        .radios
        .iter()
        .map(|r| (r.radio.as_str(), r.channel))
        .collect();
    assert_eq!(
        radios,
        [
            ("radio1", Some(crate::site::wireless::Channel::Fixed(6))),
            ("radio0", Some(crate::site::wireless::Channel::Fixed(36))),
        ]
    );
    let ports: Vec<_> = node.ports.iter().map(|p| p.port.as_str()).collect();
    assert_eq!(ports, ["lan1"]);
    assert_eq!(node.raw, []);
    assert_eq!(node.hostname.as_deref(), Some("hall"));

    let plan: Vec<_> = config
        .radio
        .plan
        .radios
        .iter()
        .map(|p| (p.node.get(), p.radio.as_str(), p.channel))
        .collect();
    assert_eq!(
        plan,
        [(2, "radio1", 44), (3, "radio1", 6), (3, "radio0", 36)]
    );

    let dhcp = config.networks[0]
        .ipv4
        .as_ref()
        .unwrap()
        .dhcp
        .as_ref()
        .unwrap();
    assert!(dhcp.reservations.iter().any(|r| r.mac == NEW));
    assert!(!dhcp.reservations.iter().any(|r| r.mac == OLD));

    assert_eq!(
        replaced.dropped,
        Dropped {
            radios: vec!["radio2 (5 GHz)".to_owned()],
            ports: vec!["lan4".to_owned()],
            raw: 1,
        }
    );
    assert_eq!(replaced.dropped.lines().len(), 3);
}

#[test]
fn the_same_device_back_changes_nothing() {
    let mut config = site();
    for entry in &mut config.nodes {
        entry.raw.clear();
    }
    let before = config.clone();
    let same = device(
        OLD,
        &[
            ("radio0", Band::G2),
            ("radio1", Band::G5),
            ("radio2", Band::G5),
        ],
        &["lan1", "lan4"],
    );
    let replaced = replace_node(&mut config, NODE, &same, &same);
    assert!(!replaced.changed, "{replaced:?}");
    assert!(replaced.dropped.is_empty());
    assert_eq!(config, before);
}

#[test]
fn radios_the_old_facts_never_listed_stay_only_by_name_and_band() {
    let mut config = site();
    let old = device(OLD, &[], &[]);
    let new = device(NEW, &[("radio0", Band::G2), ("radio1", Band::G6)], &[]);
    let replaced = replace_node(&mut config, NODE, &old, &new);
    let node = config.nodes.iter().find(|n| n.node == NODE).unwrap();
    let radios: Vec<_> = node.radios.iter().map(|r| r.radio.as_str()).collect();
    assert_eq!(radios, ["radio0", "radio1"]);
    // The plan knows radio1 was 5 GHz: a 6 GHz radio of that name doesn't take it.
    let plan: Vec<_> = config
        .radio
        .plan
        .radios
        .iter()
        .filter(|p| p.node == NODE)
        .map(|p| p.radio.as_str())
        .collect();
    assert_eq!(plan, ["radio0"]);
    assert_eq!(replaced.dropped.radios, ["radio2", "radio1"]);
}
