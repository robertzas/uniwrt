//! Validating the firewall's parts that M4.2b added (port forwards, the DMZ, UPnP), and their
//! migration.

use std::{collections::BTreeSet, net::Ipv4Addr};

use serde_json::Value;
use uniwrt_core::NodeId;

use super::{
    SiteConfig,
    firewall::{ForwardProto, PortForward, PortRange},
    gateway::WanProto,
    migrate::upgrade,
    node::ManagedPackage,
    validate::{Severity, validate},
};

fn fixture(version: u32) -> Value {
    let text = match version {
        4 => include_str!("../../fixtures/site/v4.json"),
        _ => include_str!("../../fixtures/site/v5.json"),
    };
    serde_json::from_str(text).unwrap()
}

/// The (path, severity) of the issues of the v5 fixture after `edit`.
fn issues_after(edit: impl FnOnce(&mut SiteConfig)) -> Vec<(String, Severity)> {
    let mut config = upgrade(fixture(5)).unwrap();
    edit(&mut config);
    validate(&config, &BTreeSet::from([NodeId::new(1), NodeId::new(2)]))
        .into_iter()
        .map(|issue| (issue.path, issue.severity))
        .collect()
}

fn forward(name: &str, proto: ForwardProto, ports: (u16, u16), to: [u8; 4]) -> PortForward {
    PortForward {
        name: name.to_owned(),
        enabled: true,
        proto,
        ports: PortRange {
            first: ports.0,
            last: ports.1,
        },
        to: Ipv4Addr::from(to),
        to_port: None,
        source: None,
        reflection: true,
    }
}

#[test]
fn v4_documents_gain_forwards_and_upnp_off_for_the_default_network() {
    let config = upgrade(fixture(4)).unwrap();
    let firewall = &config.firewall;
    assert_eq!(firewall.forwards, []);
    assert_eq!(firewall.dmz, None);
    assert!(!firewall.upnp.enabled);
    assert_eq!(firewall.upnp.networks, ["lan"]);
    // The gateway manages the firewall, the AP doesn't.
    assert!(config.nodes[0].managed.contains(&ManagedPackage::Upnpd));
    assert!(!config.nodes[1].managed.contains(&ManagedPackage::Upnpd));
}

#[test]
fn new_sites_have_upnp_on_for_the_main_lan() {
    let config = SiteConfig::defaults(None);
    assert!(config.firewall.upnp.enabled);
    assert_eq!(
        config.firewall.upnp.networks,
        [config.networks[0].id.clone()]
    );
}

#[test]
fn the_fixtures_forwards_validate() {
    assert_eq!(issues_after(|_| {}), []);
}

#[test]
fn forwards_are_checked() {
    let at = "firewall/forwards/2";
    let cases: Vec<(PortForward, &str)> = vec![
        (
            forward("", ForwardProto::Tcp, (80, 80), [10, 0, 0, 5]),
            "name",
        ),
        (
            forward(" web", ForwardProto::Tcp, (80, 80), [10, 0, 0, 5]),
            "name",
        ),
        (
            forward("game server", ForwardProto::Tcp, (80, 80), [10, 0, 0, 5]),
            "name",
        ),
        (
            forward("web", ForwardProto::Tcp, (0, 80), [10, 0, 0, 5]),
            "ports",
        ),
        (
            forward("web", ForwardProto::Tcp, (90, 80), [10, 0, 0, 5]),
            "ports",
        ),
        (
            forward("web", ForwardProto::Tcp, (80, 80), [10, 9, 0, 5]),
            "to",
        ),
        (
            forward("web", ForwardProto::Tcp, (80, 80), [10, 0, 0, 1]),
            "to",
        ),
        (
            forward("web", ForwardProto::Tcp, (80, 80), [10, 0, 0, 255]),
            "to",
        ),
        (
            forward("web", ForwardProto::Tcp, (80, 80), [10, 0, 0, 0]),
            "to",
        ),
        // The game server's UDP range, enabled.
        (
            forward("voice", ForwardProto::Udp, (27030, 27040), [10, 0, 0, 5]),
            "ports",
        ),
        (
            PortForward {
                to_port: Some(65530),
                ..forward("web", ForwardProto::Tcp, (80, 90), [10, 0, 0, 5])
            },
            "to_port",
        ),
    ];
    for (bad, field) in cases {
        let issues = issues_after(|c| c.firewall.forwards.push(bad.clone()));
        assert_eq!(
            issues,
            [(format!("{at}/{field}"), Severity::Error)],
            "{bad:?}"
        );
    }
    // Disabled forwards don't claim their ports, and TCP doesn't clash with UDP.
    for fine in [
        PortForward {
            enabled: false,
            ..forward("voice", ForwardProto::Udp, (27030, 27040), [10, 0, 0, 5])
        },
        forward("ssh", ForwardProto::Udp, (8443, 8443), [10, 0, 30, 5]),
    ] {
        assert_eq!(issues_after(|c| c.firewall.forwards.push(fine)), []);
    }
}

#[test]
fn the_dmz_host_is_a_client_address() {
    assert_eq!(
        issues_after(|c| c.firewall.dmz = Some(Ipv4Addr::new(10, 0, 0, 50))),
        []
    );
    assert_eq!(
        issues_after(|c| c.firewall.dmz = Some(Ipv4Addr::new(192, 0, 2, 1))),
        [("firewall/dmz".to_owned(), Severity::Error)]
    );
}

#[test]
fn forwards_behind_ds_lite_warn() {
    let issues = issues_after(|c| {
        c.wan.proto = WanProto::DsLite { aftr: None };
        c.wan.ipv6 = super::gateway::WanIpv6::Dhcpv6 { reqprefix: None };
    });
    assert!(
        issues.contains(&("firewall/forwards".to_owned(), Severity::Warning)),
        "{issues:?}"
    );
}

#[test]
fn upnp_networks_are_checked() {
    let at = |i: usize| (format!("firewall/upnp/networks/{i}"), Severity::Error);
    assert_eq!(
        issues_after(|c| c.firewall.upnp.networks.push("nope".to_owned())),
        [at(2)]
    );
    assert_eq!(
        issues_after(|c| c.firewall.upnp.networks.push("lan".to_owned())),
        [at(2)]
    );
    assert_eq!(
        issues_after(|c| c.firewall.upnp.networks.clear()),
        [("firewall/upnp/networks".to_owned(), Severity::Error)]
    );
    // Off, it may serve none.
    assert_eq!(
        issues_after(|c| {
            c.firewall.upnp.enabled = false;
            c.firewall.upnp.networks.clear();
        }),
        []
    );
}
