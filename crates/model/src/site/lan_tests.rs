//! Validating the LAN's parts that M4.2a added: port profiles, DHCP options, IPv6 clients and the
//! controller's record.

use std::{collections::BTreeSet, net::Ipv4Addr};

use serde_json::Value;
use uniwrt_core::NodeId;

use super::{
    SiteConfig,
    gateway::DnsRecord,
    migrate::upgrade,
    network::{Boot, Ipv6Clients},
    node::{PortConfig, TaggedNetworks},
    validate::{Severity, validate},
};

fn sample() -> SiteConfig {
    let value: Value = serde_json::from_str(include_str!("../../fixtures/site/v4.json")).unwrap();
    upgrade(value).unwrap()
}

/// The (path, severity) of the issues of the sample after `edit`.
fn issues_after(edit: impl FnOnce(&mut SiteConfig)) -> Vec<(String, Severity)> {
    let mut config = sample();
    edit(&mut config);
    validate(&config, &BTreeSet::from([NodeId::new(1), NodeId::new(2)]))
        .into_iter()
        .map(|issue| (issue.path, issue.severity))
        .collect()
}

fn profile(port: &str, native: Option<&str>, tagged: TaggedNetworks) -> PortConfig {
    PortConfig {
        port: port.to_owned(),
        native: native.map(str::to_owned),
        tagged,
    }
}

fn only(ids: &[&str]) -> TaggedNetworks {
    TaggedNetworks::Only {
        networks: ids.iter().map(|id| (*id).to_owned()).collect(),
    }
}

type Edit = Box<dyn Fn(&mut SiteConfig)>;

#[test]
fn the_fixture_is_clean() {
    assert_eq!(issues_after(|_| {}), vec![]);
}

#[test]
fn port_profiles_name_ports_and_networks_that_exist() {
    let ports = |profiles: Vec<PortConfig>| -> Edit {
        Box::new(move |c: &mut SiteConfig| c.nodes[0].ports = profiles.clone())
    };
    let cases: Vec<(Edit, &str)> = vec![
        (
            ports(vec![profile("lan 1", None, TaggedNetworks::All)]),
            "nodes/1/ports/lan 1",
        ),
        (
            ports(vec![
                profile("lan1", None, TaggedNetworks::All),
                profile("lan1", None, TaggedNetworks::None),
            ]),
            "nodes/1/ports/lan1",
        ),
        (
            ports(vec![profile("lan1", Some("lab"), TaggedNetworks::None)]),
            "nodes/1/ports/lan1/native",
        ),
        (
            ports(vec![profile("lan1", None, only(&["lab"]))]),
            "nodes/1/ports/lan1/tagged",
        ),
        // The untagged network is carried untagged only.
        (
            ports(vec![profile("lan1", None, only(&["lan"]))]),
            "nodes/1/ports/lan1/tagged",
        ),
        (
            ports(vec![profile("lan1", Some("iot"), only(&["iot"]))]),
            "nodes/1/ports/lan1/tagged",
        ),
        (
            ports(vec![profile("lan1", None, only(&["iot", "iot"]))]),
            "nodes/1/ports/lan1/tagged",
        ),
    ];
    for (edit, path) in cases {
        let issues = issues_after(edit);
        assert!(
            issues.contains(&(path.to_owned(), Severity::Error)),
            "{path}: {issues:?}"
        );
    }
    let fine = issues_after(|c| {
        c.nodes[0].ports = vec![
            profile("switch0:3", Some("iot"), only(&["guest"])),
            profile("eth0", None, TaggedNetworks::None),
        ];
    });
    assert_eq!(fine, vec![]);
}

#[test]
fn dhcp_options_stay_on_their_line() {
    let options = |edit: fn(&mut super::network::DhcpOptions)| -> Edit {
        Box::new(move |c: &mut SiteConfig| {
            let dhcp = c.networks[0].ipv4.as_mut().unwrap().dhcp.as_mut().unwrap();
            edit(&mut dhcp.options);
        })
    };
    let at = "networks/lan/ipv4/dhcp/options";
    let cases: Vec<(Edit, String)> = vec![
        (
            options(|o| o.dns = vec![Ipv4Addr::BROADCAST]),
            format!("{at}/dns"),
        ),
        (
            options(|o| o.ntp = vec![Ipv4Addr::LOCALHOST; 5]),
            format!("{at}/ntp"),
        ),
        (
            options(|o| o.domain = Some("home arpa".to_owned())),
            format!("{at}/domain"),
        ),
        (
            options(|o| {
                o.boot = Some(Boot {
                    filename: "pxe linux.0".to_owned(),
                    server_name: None,
                    server: None,
                });
            }),
            format!("{at}/boot/filename"),
        ),
        (
            options(|o| {
                o.boot = Some(Boot {
                    filename: "pxelinux.0".to_owned(),
                    server_name: None,
                    server: Some(Ipv4Addr::new(10, 0, 0, 20)),
                });
            }),
            format!("{at}/boot/server_name"),
        ),
        (
            options(|o| o.custom = vec!["26 1400".to_owned()]),
            format!("{at}/custom/0"),
        ),
        (
            options(|o| o.custom = vec!["26,1400\nx".to_owned()]),
            format!("{at}/custom/0"),
        ),
        (
            options(|o| o.custom = vec!["tag:x,3,10.0.0.1".to_owned()]),
            format!("{at}/custom/0"),
        ),
    ];
    for (edit, path) in cases {
        let issues = issues_after(edit);
        assert!(
            issues.contains(&(path.clone(), Severity::Error)),
            "{path}: {issues:?}"
        );
    }
    let issues = issues_after(options(|o| o.custom = vec!["6,10.0.0.9".to_owned()]));
    assert_eq!(issues, vec![(format!("{at}/custom/0"), Severity::Warning)]);
}

#[test]
fn stateful_only_ipv6_warns_about_android() {
    let issues = issues_after(|c| c.networks[0].ipv6_clients = Ipv6Clients::Stateful);
    assert_eq!(
        issues,
        vec![("networks/lan/ipv6_clients".to_owned(), Severity::Warning)]
    );
}

#[test]
fn the_controllers_name_is_kept() {
    for name in ["uniwrt", "UniWRT.lan", "uniwrt.lan."] {
        let issues = issues_after(|c| {
            c.services.dns_records = vec![DnsRecord::Host {
                name: name.to_owned(),
                ip: "10.0.0.9".parse().unwrap(),
            }];
        });
        assert_eq!(
            issues,
            vec![("services/dns_records/0/name".to_owned(), Severity::Error)],
            "{name}"
        );
    }
    let issues = issues_after(|c| {
        c.services.dns_records = vec![DnsRecord::Host {
            name: "uniwrt.example.com".to_owned(),
            ip: "10.0.0.9".parse().unwrap(),
        }];
    });
    assert_eq!(issues, vec![]);
}
