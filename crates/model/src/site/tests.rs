//! The site document: the fixture of every schema, defaults, secrets, validation and the diff.

use std::{collections::BTreeSet, net::Ipv4Addr};

use serde_json::{Value, json};
use uniwrt_core::{Ipv4Net, Ipv6Net, NodeId};

use super::{
    SCHEMA_VERSION, SecretRef, SiteConfig,
    diff::{ChangeOp, diff},
    gateway::{DhcpClient, WanIpv6, WanProto},
    migrate::{SchemaError, upgrade},
    network::Purpose,
    validate::{Severity, has_errors, validate},
    wireless::{Channel, WlanSecurity},
};
use crate::wifi::{Band, ChannelWidth};

/// The fixture of every schema, `FIXTURES[n - 1]` for version n.
const FIXTURES: [&str; SCHEMA_VERSION as usize] = [
    include_str!("../../fixtures/site/v1.json"),
    include_str!("../../fixtures/site/v2.json"),
    include_str!("../../fixtures/site/v3.json"),
];

fn fixture(version: u32) -> Value {
    serde_json::from_str(FIXTURES[version as usize - 1]).unwrap()
}

fn sample() -> SiteConfig {
    upgrade(fixture(1)).unwrap()
}

fn adopted() -> BTreeSet<NodeId> {
    [NodeId::new(1), NodeId::new(2)].into()
}

fn ula() -> Ipv6Net {
    "fd12:3456:789a::/48".parse().unwrap()
}

/// The issues of `sample()` after `edit`, as (path, severity).
fn issues_after(edit: impl FnOnce(&mut SiteConfig)) -> Vec<(String, Severity)> {
    let mut config = sample();
    edit(&mut config);
    validate(&config, &adopted())
        .into_iter()
        .map(|issue| (issue.path, issue.severity))
        .collect()
}

fn has(issues: &[(String, Severity)], path: &str, severity: Severity) -> bool {
    issues.iter().any(|(p, s)| p == path && *s == severity)
}

#[test]
fn every_schemas_fixture_upgrades_and_validates() {
    for version in 1..=SCHEMA_VERSION {
        let config = upgrade(fixture(version)).unwrap();
        assert_eq!(config.schema, SCHEMA_VERSION);
        assert_eq!(validate(&config, &adopted()), vec![], "v{version}");
    }
}

#[test]
fn the_current_fixture_round_trips_without_losing_a_field() {
    let value = fixture(SCHEMA_VERSION);
    assert_eq!(
        serde_json::to_value(upgrade(value.clone()).unwrap()).unwrap(),
        value
    );
}

#[test]
fn documents_without_a_known_schema_are_refused() {
    let mut value = fixture(1);
    value["schema"] = json!(SCHEMA_VERSION + 1);
    assert!(matches!(
        upgrade(value.clone()),
        Err(SchemaError::TooNew { .. })
    ));
    value["schema"] = json!(0);
    assert!(matches!(upgrade(value.clone()), Err(SchemaError::Missing)));
    value.as_object_mut().unwrap().remove("schema");
    assert!(matches!(upgrade(value), Err(SchemaError::Missing)));
    let mut value = fixture(1);
    value["networks"][0]["purpose"] = json!("lab");
    assert!(matches!(upgrade(value), Err(SchemaError::Invalid(_))));
}

#[test]
fn v1_wans_upgrade_to_an_ipv6_mode() {
    let config = upgrade(fixture(1)).unwrap();
    assert_eq!(config.wan.ipv6, WanIpv6::Dhcpv6 { reqprefix: None });
    assert_eq!(
        (config.wan.pcp, config.wan.mtu, config.wan.preset),
        (None, None, None)
    );
    assert!(matches!(
        config.wan.proto,
        WanProto::Pppoe { service: None, .. }
    ));
    let mut value = fixture(1);
    value["wan"]["ipv6"] = json!(false);
    value["wan"]["proto"] = json!({ "kind": "dhcp" });
    let config = upgrade(value).unwrap();
    assert_eq!(config.wan.ipv6, WanIpv6::Off);
    assert_eq!(config.wan.proto, WanProto::Dhcp(DhcpClient::default()));
}

type Edit = Box<dyn Fn(&mut SiteConfig)>;

#[test]
fn wan_rules() {
    let sixrd = WanIpv6::SixRd {
        prefix: "2602::/24".parse().unwrap(),
        border_relay: Ipv4Addr::new(205, 171, 2, 64),
        ipv4_mask_len: 0,
    };
    let cases: Vec<(Edit, &str)> = vec![
        (Box::new(|c| c.wan.pcp = Some(8)), "wan/pcp"),
        (
            Box::new(|c| {
                c.wan.vlan = None;
                c.wan.pcp = Some(3);
            }),
            "wan/pcp",
        ),
        (Box::new(|c| c.wan.mtu = Some(1200)), "wan/mtu"),
        (
            Box::new(|c| {
                c.wan.proto = WanProto::Dhcp(DhcpClient {
                    client_id: Some("abc".to_owned()),
                    ..DhcpClient::default()
                });
            }),
            "wan/proto/client_id",
        ),
        (
            Box::new(|c| {
                c.wan.proto = WanProto::Dhcp(DhcpClient {
                    send_options: vec!["0:x".to_owned()],
                    ..DhcpClient::default()
                });
            }),
            "wan/proto/send_options/0",
        ),
        (
            Box::new(|c| {
                c.wan.proto = WanProto::DsLite { aftr: None };
                c.wan.ipv6 = WanIpv6::Off;
            }),
            "wan/ipv6",
        ),
        (
            Box::new(|c| {
                c.wan.proto = WanProto::None;
                c.wan.ipv6 = WanIpv6::Off;
            }),
            "wan/ipv6",
        ),
        (
            Box::new(|c| {
                c.wan.ipv6 = WanIpv6::Dhcpv6 {
                    reqprefix: Some(40),
                }
            }),
            "wan/ipv6/reqprefix",
        ),
        (
            Box::new(|c| {
                c.wan.ipv6 = WanIpv6::SixRd {
                    prefix: "2602::/48".parse().unwrap(),
                    border_relay: Ipv4Addr::new(205, 171, 2, 64),
                    ipv4_mask_len: 0,
                };
            }),
            "wan/ipv6/prefix",
        ),
        (
            Box::new(|c| {
                c.wan.ipv6 = WanIpv6::SixRd {
                    prefix: "2602::/24".parse().unwrap(),
                    border_relay: Ipv4Addr::BROADCAST,
                    ipv4_mask_len: 0,
                };
            }),
            "wan/ipv6/border_relay",
        ),
    ];
    for (edit, path) in cases {
        let issues = issues_after(edit);
        assert!(has(&issues, path, Severity::Error), "{path}: {issues:?}");
    }
    let issues = issues_after(|c| {
        c.wan.proto = WanProto::DsLite {
            aftr: Some("aftr.example.net".to_owned()),
        };
        c.wan.ipv6 = WanIpv6::Dhcpv6 {
            reqprefix: Some(56),
        };
    });
    assert!(
        !issues.iter().any(|(p, _)| p.starts_with("wan")),
        "{issues:?}"
    );
    let issues = issues_after(move |c| {
        c.wan.proto = WanProto::None;
        c.wan.ipv6 = sixrd.clone();
    });
    assert!(has(&issues, "wan/ipv6", Severity::Error));
}

#[test]
fn defaults_are_valid_with_or_without_a_ula() {
    for ula in [Some(ula()), None] {
        let config = SiteConfig::defaults(ula);
        assert_eq!(validate(&config, &BTreeSet::new()), vec![]);
        assert_eq!(config.networks[0].purpose, Purpose::Default);
        assert_eq!(config.addressing.ula, ula);
    }
}

#[test]
fn secret_refs_name_configuration_secrets_only() {
    assert!(SecretRef::new("config/0123456789abcdef0123456789abcdef").is_some());
    assert!(SecretRef::new("config/0123456789ABCDEF0123456789abcdef").is_none());
    assert!(SecretRef::new("config/0123").is_none());
    assert!(SecretRef::new("controller/ssh_key").is_none());
}

#[test]
fn secrets_are_listed_and_set_by_path() {
    let mut config = sample();
    let paths: Vec<_> = config.secrets().into_iter().map(|slot| slot.path).collect();
    assert_eq!(
        paths,
        [
            "wlans/home/passphrase",
            "wlans/things/passphrase",
            "wlans/visitors/passphrase",
            "mesh/key",
            "wan/password"
        ]
    );
    assert!(config.secret("wlans/visitors/passphrase").is_none());
    assert_eq!(
        config.secret("mesh/key").map(SecretRef::name),
        Some("config/00112233445566778899aabbccddeeff")
    );
    let new = SecretRef::new(format!("config/{}", "a".repeat(32))).unwrap();
    *config.secret_mut("wan/password").unwrap() = Some(new.clone());
    assert_eq!(config.secret("wan/password"), Some(&new));
    assert!(config.secret_mut("wlans/nope/passphrase").is_none());
    assert!(config.secret_mut("site/name").is_none());
    config.wan.proto = WanProto::Dhcp(DhcpClient::default());
    assert!(config.secret_mut("wan/password").is_none());
    assert_eq!(config.secrets().len(), 4);
}

#[test]
fn raw_secrets_are_slots_that_replace_secret_renames() {
    let mut config = sample();
    let old = SecretRef::new(format!("config/{}", "b".repeat(32))).unwrap();
    let options = &mut config.nodes[1].raw[0].options;
    options.insert(
        "private_key".to_owned(),
        super::node::RawValue::Secret {
            secret: old.clone(),
        },
    );
    let json = serde_json::to_value(&config.nodes[1].raw[0]).unwrap();
    assert_eq!(
        json["options"]["private_key"],
        json!({"secret": old.name()})
    );
    let back: super::node::RawSection = serde_json::from_value(json).unwrap();
    assert_eq!(back, config.nodes[1].raw[0]);
    let path = format!("nodes/{}/raw/0/private_key", config.nodes[1].node);
    assert_eq!(config.secret(&path), Some(&old));
    assert!(config.secret_mut(&path).is_none());
    let new = SecretRef::new(format!("config/{}", "c".repeat(32))).unwrap();
    let mesh = config.mesh.key.clone().unwrap();
    config.replace_secret(&old, &new);
    config.replace_secret(&mesh, &new);
    assert_eq!(config.secret(&path), Some(&new));
    assert_eq!(config.mesh.key.as_ref(), Some(&new));
    assert!(super::check_secret(&path, "aGVsbG8=").is_ok());
    assert!(super::check_secret(&path, "a\nb").is_err());
    assert!(super::node::is_secret_option("preshared_key"));
    assert!(!super::node::is_secret_option("ssid"));
}

#[test]
fn secret_values() {
    use super::check_secret;
    assert!(check_secret("wlans/home/passphrase", "correct horse").is_ok());
    assert!(check_secret("wlans/home/passphrase", &"a".repeat(64)).is_ok());
    assert!(check_secret("mesh/key", &"a".repeat(64)).is_err());
    assert!(check_secret("wlans/home/passphrase", "short").is_err());
    assert!(check_secret("wlans/home/passphrase", "naïve passphrase").is_err());
    assert!(check_secret("wan/password", "p").is_ok());
    assert!(check_secret("wan/password", "").is_err());
    assert!(check_secret("wan/password", "a\nb").is_err());
    assert!(check_secret("wan/password", "a\u{7}b").is_err());
}

#[test]
fn addressing_guarantees() {
    let issues = issues_after(|c| {
        let iot = c.networks[1].ipv4.as_mut().unwrap();
        iot.address = Ipv4Net::new(Ipv4Addr::new(10, 0, 0, 2), 23).unwrap();
    });
    assert!(
        has(&issues, "networks/iot/ipv4/address", Severity::Error),
        "{issues:?}"
    );
    let issues = issues_after(|c| {
        c.networks[1].ipv4.as_mut().unwrap().address = "100.64.0.1/24".parse().unwrap();
    });
    assert!(has(&issues, "networks/iot/ipv4/address", Severity::Error));
    let issues = issues_after(|c| {
        c.networks[1].ipv4.as_mut().unwrap().address = "192.168.1.1/24".parse().unwrap();
    });
    assert!(has(&issues, "networks/iot/ipv4/address", Severity::Warning));
    assert!(!has_errors(&validate(
        &{
            let mut c = sample();
            c.networks[1].ipv4.as_mut().unwrap().address = "192.168.1.1/24".parse().unwrap();
            c
        },
        &adopted()
    )));
    let issues = issues_after(|c| {
        c.networks[0].ipv4.as_mut().unwrap().address = "10.0.0.0/24".parse().unwrap();
    });
    assert!(has(&issues, "networks/lan/ipv4/address", Severity::Error));
    let issues = issues_after(|c| {
        let dhcp = c.networks[0].ipv4.as_mut().unwrap().dhcp.as_mut().unwrap();
        dhcp.start = Ipv4Addr::new(10, 0, 0, 1);
        dhcp.end = Ipv4Addr::new(10, 0, 1, 9);
        dhcp.reservations.push(dhcp.reservations[0].clone());
        dhcp.lease_time_s = 60;
    });
    for path in [
        "start",
        "end",
        "lease_time_s",
        "reservations/aa:bb:cc:00:00:01",
    ] {
        let path = format!("networks/lan/ipv4/dhcp/{path}");
        assert!(has(&issues, &path, Severity::Error), "{path}: {issues:?}");
    }
    let issues = issues_after(|c| c.addressing.ipv4 = "8.8.0.0/16".parse().unwrap());
    assert!(has(&issues, "addressing/ipv4", Severity::Error));
    assert!(has(&issues, "networks/lan/ipv4/address", Severity::Warning));
}

#[test]
fn network_identity() {
    let issues = issues_after(|c| {
        c.networks[1].id = "lan".to_owned();
        c.networks[2].vlan = Some(30);
        c.networks[2].purpose = Purpose::Default;
        c.networks[0].name = "Guest".to_owned();
    });
    assert!(has(&issues, "networks/lan", Severity::Error));
    assert!(has(&issues, "networks/guest/vlan", Severity::Error));
    assert!(has(&issues, "networks", Severity::Error));
    assert!(has(&issues, "networks/guest/name", Severity::Error));
    let issues = issues_after(|c| c.networks[1].id = "Iot-Things".to_owned());
    assert!(has(&issues, "networks/Iot-Things", Severity::Error));
    // The WLAN on it now names a network that does not exist.
    assert!(has(&issues, "wlans/things/network", Severity::Error));
    let issues = issues_after(|c| c.addressing.ula = None);
    assert!(has(&issues, "networks/lan/ipv6", Severity::Error));
    for reserved in ["wan", "bat0", "uw_mesh"] {
        let issues = issues_after(|c| c.networks[1].id = reserved.to_owned());
        assert!(
            has(&issues, &format!("networks/{reserved}"), Severity::Error),
            "{reserved}"
        );
    }
    let issues = issues_after(|c| c.site.timezone = "Mars/Olympus".to_owned());
    assert!(has(&issues, "site/timezone", Severity::Error));
}

#[test]
fn wireless_rules() {
    let issues = issues_after(|c| {
        c.wlans[0].security = WlanSecurity::Wpa2;
        c.wlans[0].bands.push(Band::G6);
        c.wlans[1].passphrase = None;
        c.wlans[2].ssid = "x".repeat(33);
        c.wlans[2].bands = vec![];
        c.mesh.key = None;
        c.site.country = None;
        c.radio.bands[0].channel = Channel::Fixed(36);
        c.radio.bands[0].width = ChannelWidth::Mhz80;
        c.radio.bands.push(c.radio.bands[1].clone());
    });
    for (path, severity) in [
        ("wlans/home/security", Severity::Error),
        ("wlans/things/passphrase", Severity::Error),
        ("wlans/visitors/ssid", Severity::Error),
        ("wlans/visitors/bands", Severity::Error),
        ("mesh/key", Severity::Error),
        ("site/country", Severity::Warning),
        ("radio/bands/2.4 GHz/channel", Severity::Error),
        ("radio/bands/2.4 GHz/width", Severity::Error),
        ("radio/bands/5 GHz", Severity::Error),
    ] {
        assert!(has(&issues, path, severity), "{path}: {issues:?}");
    }
    let issues =
        issues_after(|c| c.wlans[1].nodes = super::wireless::NodeScope::Only(vec![NodeId::new(9)]));
    assert!(has(&issues, "wlans/things/nodes", Severity::Warning));
}

#[test]
fn gateway_and_node_rules() {
    let issues = issues_after(|c| {
        c.wan.proto = WanProto::Pppoe {
            username: String::new(),
            password: None,
            service: Some(String::new()),
        };
        c.firewall.policies.push(c.firewall.policies[0].clone());
        c.firewall.policies.push(super::gateway::ZonePolicy {
            from: "lab".to_owned(),
            to: "lab".to_owned(),
            action: super::gateway::ZoneAction::Block,
        });
        c.nodes[0].hostname = Some("bad name".to_owned());
        c.nodes[0].raw[0].package = "wireless".to_owned();
        c.nodes[1].raw[0].options.insert(
            "bad-option".to_owned(),
            super::node::RawValue::Option("a\nb".to_owned()),
        );
        c.nodes.push(super::node::NodeConfig::new(NodeId::new(7)));
    });
    for (path, severity) in [
        ("wan/proto/username", Severity::Error),
        ("wan/password", Severity::Error),
        ("wan/proto/service", Severity::Error),
        ("firewall/policies/1", Severity::Error),
        ("firewall/policies/2/from", Severity::Error),
        ("firewall/policies/2", Severity::Error),
        ("nodes/1/hostname", Severity::Error),
        ("nodes/1/raw/0", Severity::Warning),
        ("nodes/2/raw/0/options/bad-option", Severity::Error),
        ("nodes/7", Severity::Warning),
    ] {
        assert!(has(&issues, path, severity), "{path}: {issues:?}");
    }
}

#[test]
fn the_diff_matches_list_items_by_key() {
    let before = sample();
    let mut after = before.clone();
    after.networks[1].vlan = Some(31);
    after.networks.swap(0, 2);
    after.wlans.remove(2);
    after.site.ntp_servers.push("pool.ntp.org".to_owned());
    after.nodes[1].radios[0].disabled = true;
    after
        .nodes
        .push(super::node::NodeConfig::new(NodeId::new(3)));
    let changes: Vec<_> = diff(&before, &after)
        .into_iter()
        .map(|change| (change.path.join("/"), change.op))
        .collect();
    assert_eq!(
        changes,
        [
            ("networks/iot/vlan".to_owned(), ChangeOp::Changed),
            (
                "nodes/2/radios/radio1/disabled".to_owned(),
                ChangeOp::Changed
            ),
            ("nodes/3".to_owned(), ChangeOp::Added),
            ("site/ntp_servers".to_owned(), ChangeOp::Changed),
            ("wlans/visitors".to_owned(), ChangeOp::Removed),
        ]
    );
    assert_eq!(diff(&before, &before), vec![]);
    let change = &diff(&before, &after)[0];
    assert_eq!(
        (change.before.clone(), change.after.clone()),
        (Some(json!(30)), Some(json!(31)))
    );
}

#[test]
fn serde_names() {
    assert_eq!(
        serde_json::to_value(Channel::Fixed(36)).unwrap(),
        json!({ "fixed": 36 })
    );
    assert_eq!(serde_json::to_value(Channel::Auto).unwrap(), json!("auto"));
    assert_eq!(
        serde_json::to_value(WlanSecurity::Wpa2Wpa3).unwrap(),
        json!("wpa2_wpa3")
    );
    assert_eq!(
        serde_json::to_value(super::revision::RevisionKind::Restore).unwrap(),
        json!("restore")
    );
}
