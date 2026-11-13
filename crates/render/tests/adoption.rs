//! The adoption port (features/adoption.md, Path C; D-105): a gateway port taken out of the LAN
//! into `br-adopt`, with its own zone and no DHCP, and the ports and uplinks that can't serve.

// Tests and their fixtures may panic on what they set up.
#![allow(clippy::unwrap_used)]

mod support;

use insta::assert_snapshot;
use uniwrt_model::{
    node::NodeRole,
    site::{
        SiteConfig,
        adoption::AdoptionPort,
        gateway::{BackupLink, BackupProto, Failover},
        node::ManagedPackage,
    },
};
use uniwrt_render::{ADOPT_BRIDGE, ADOPT_INTERFACE, Issue, Rendered};

use support::*;

fn with_port(port: &str) -> SiteConfig {
    let mut site = home_site();
    site.adoption_port = Some(AdoptionPort::new(port));
    site
}

fn paths(issues: &[Issue]) -> Vec<&str> {
    issues.iter().map(|issue| issue.path.as_str()).collect()
}

fn rendered(result: &Result<Rendered, Vec<Issue>>) -> &Rendered {
    result.as_ref().unwrap()
}

#[test]
fn the_gateways_port_becomes_an_isolated_segment() {
    let facts = gateway_facts();
    let result = run(&with_port("lan2"), GATEWAY, NodeRole::Gateway, &facts);
    let rendered = rendered(&result);
    let network = &rendered.packages[&ManagedPackage::Network];
    let bridge = |name: &str| {
        network
            .sections_of("device")
            .find(|d| d.option("name") == Some(name))
            .unwrap()
            .get("ports")
            .unwrap()
            .items()
            .to_vec()
    };
    assert!(
        !bridge("br-lan").contains(&"lan2".to_owned()),
        "it leaves the LAN"
    );
    assert_eq!(bridge(ADOPT_BRIDGE), ["lan2"]);
    let interface = network.section(ADOPT_INTERFACE).unwrap();
    assert_eq!(interface.option("proto"), Some("static"));
    assert_eq!(
        interface.get("ipaddr").unwrap().items(),
        ["192.168.1.250/24"]
    );
    let dhcp = rendered.packages[&ManagedPackage::Dhcp]
        .section(ADOPT_INTERFACE)
        .unwrap();
    assert_eq!(dhcp.option("ignore"), Some("1"));
    let firewall = &rendered.packages[&ManagedPackage::Firewall];
    let zone = firewall
        .sections_of("zone")
        .find(|z| z.option("name") == Some(ADOPT_INTERFACE))
        .unwrap();
    assert_eq!(zone.option("input"), Some("REJECT"));
    assert_eq!(zone.option("forward"), Some("REJECT"));
    assert!(
        !firewall.sections_of("forwarding").any(|f| {
            f.option("src") == Some(ADOPT_INTERFACE) || f.option("dest") == Some(ADOPT_INTERFACE)
        }),
        "nothing is forwarded to or from it"
    );
    assert!(
        !paths(&rendered.warnings).contains(&"adoption_port/port"),
        "{:?}",
        rendered.warnings
    );
    // Only the segment's sections: the rest is the gateway's golden render.
    let shown = show(&result);
    let segment: Vec<&str> = shown
        .split("\n\n")
        .filter(|block| block.contains("adopt"))
        .collect();
    assert_snapshot!("gateway_adoption_port", segment.join("\n\n"));
}

#[test]
fn access_points_render_no_adoption_port() {
    let result = run(&with_port("lan4"), AP, NodeRole::Ap, &gateway_facts());
    let network = &rendered(&result).packages[&ManagedPackage::Network];
    assert!(network.section(ADOPT_INTERFACE).is_none());
    assert!(
        network
            .sections_of("device")
            .all(|d| d.option("name") != Some(ADOPT_BRIDGE))
    );
}

#[test]
fn the_wan_port_and_a_swconfig_port_cant_serve() {
    let facts = gateway_facts();
    let wan = facts.board.layout.wan.clone().unwrap();
    let issues = run(&with_port(&wan), GATEWAY, NodeRole::Gateway, &facts).unwrap_err();
    assert_eq!(paths(&issues), ["adoption_port/port"]);
    let facts = swconfig_facts();
    let lan = facts.board.layout.lan[0].clone();
    let issues = run(&with_port(&lan), GATEWAY, NodeRole::Gateway, &facts).unwrap_err();
    assert!(
        issues.iter().any(|i| i.message.contains("swconfig")),
        "{issues:?}"
    );
}

#[test]
fn an_uplink_in_the_factory_subnet_is_refused() {
    let mut facts = gateway_facts();
    let wan = facts
        .interfaces
        .iter_mut()
        .find(|i| i.name == "wan")
        .unwrap();
    wan.ipv4 = vec!["192.168.1.77/24".parse().unwrap()];
    let issues = run(&with_port("lan4"), GATEWAY, NodeRole::Gateway, &facts).unwrap_err();
    assert_eq!(paths(&issues), ["adoption_port/port"]);
    // A modem's network elsewhere in 192.168.0.0/16 is fine.
    facts
        .interfaces
        .iter_mut()
        .find(|i| i.name == "wan")
        .unwrap()
        .ipv4 = vec!["192.168.0.77/24".parse().unwrap()];
    assert!(run(&with_port("lan4"), GATEWAY, NodeRole::Gateway, &facts).is_ok());
}

#[test]
fn a_missing_port_warns_and_a_profile_of_the_port_is_left_out() {
    let mut site = with_port("lan9");
    let result = run(&site, GATEWAY, NodeRole::Gateway, &gateway_facts());
    assert!(paths(&rendered(&result).warnings).contains(&"adoption_port/port"));
    site.adoption_port = Some(AdoptionPort::new("lan4"));
    let gateway = site.nodes.iter_mut().find(|n| n.node == GATEWAY).unwrap();
    gateway.ports.push(uniwrt_model::site::node::PortConfig {
        port: "lan4".to_owned(),
        native: Some("lan".to_owned()),
        tagged: uniwrt_model::site::node::TaggedNetworks::None,
        backhaul: false,
    });
    let result = run(&site, GATEWAY, NodeRole::Gateway, &gateway_facts());
    assert!(
        rendered(&result)
            .warnings
            .iter()
            .any(|w| w.path == "nodes/1/ports/lan4" && w.message.contains("adoption port")),
        "{:?}",
        rendered(&result).warnings
    );
}

#[test]
fn the_backup_lines_port_cant_serve() {
    let mut site = with_port("lan4");
    site.wan.failover = Some(Failover {
        link: BackupLink::Port {
            port: "lan4".to_owned(),
            vlan: Some(300),
        },
        proto: BackupProto::Dhcp,
    });
    let issues = run(&site, GATEWAY, NodeRole::Gateway, &gateway_facts()).unwrap_err();
    assert_eq!(paths(&issues), ["adoption_port/port"]);
}
