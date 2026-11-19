//! The gateway's Tailscale (features/network.md, VPN; D-140): the `tailscale` zone over
//! `tailscale0`, its matrix row and the WAN's rule in `firewall`, the package it needs, and
//! nothing while it is off or on an AP.

// Tests and their fixtures may panic on what they set up.
#![allow(clippy::unwrap_used)]

mod support;

use uniwrt_model::{
    node::NodeRole,
    site::{
        SiteConfig,
        firewall::{ZoneAction, ZonePolicy},
        node::ManagedPackage,
        vpn::tailscale::Tailscale,
    },
};
use uniwrt_render::Rendered;
use uniwrt_uci::Package;

use support::*;

fn ok(result: Result<Rendered, Vec<uniwrt_render::Issue>>) -> Rendered {
    result.unwrap_or_else(|issues| panic!("{issues:?}"))
}

/// The home site with Tailscale advertising its LAN.
fn joined() -> SiteConfig {
    let mut site = home_site();
    site.vpn.tailscale = Some(Tailscale::new(vec!["lan".to_owned()]));
    site
}

/// Tailscale's zone and the WAN's rule for it.
fn sections(rendered: &Rendered) -> String {
    let firewall = &rendered.packages[&ManagedPackage::Firewall];
    let mut package = Package::new("tailscale");
    package.sections = firewall
        .sections
        .iter()
        .filter(|s| {
            s.option("name") == Some("tailscale") || s.option("name") == Some("Allow-Tailscale")
        })
        .cloned()
        .collect();
    package.to_file()
}

fn forwardings(rendered: &Rendered) -> Vec<(String, String)> {
    rendered.packages[&ManagedPackage::Firewall]
        .sections_of("forwarding")
        .map(|s| {
            let get = |o: &str| s.option(o).unwrap_or_default().to_owned();
            (get("src"), get("dest"))
        })
        .filter(|(src, dest)| src == "tailscale" || dest == "tailscale")
        .collect()
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect()
}

#[test]
fn the_gateway_gets_the_zone_the_rule_and_the_package() {
    let rendered = ok(run(&joined(), GATEWAY, NodeRole::Gateway, &gateway_facts()));
    assert_eq!(
        sections(&rendered),
        "\nconfig zone\n\toption name 'tailscale'\n\tlist device 'tailscale0'\n\
         \toption input 'ACCEPT'\n\toption output 'ACCEPT'\n\toption forward 'ACCEPT'\n\n\
         config rule\n\toption name 'Allow-Tailscale'\n\toption src 'wan'\n\
         \toption proto 'udp'\n\toption dest_port '41641'\n\toption target 'ACCEPT'\n\n"
    );
    assert!(rendered.requires.iter().any(|r| r.package == "tailscale"));
    // Its row is seeded like the default LAN's: the LAN reaches it, it reaches every network and
    // the WAN.
    assert_eq!(
        forwardings(&rendered),
        pairs(&[
            ("lan", "tailscale"),
            ("tailscale", "lan"),
            ("tailscale", "iot"),
            ("tailscale", "guest"),
            ("tailscale", "wan"),
        ])
    );
}

#[test]
fn the_policies_override_its_row() {
    let mut site = joined();
    site.firewall.policies.extend([
        ZonePolicy {
            from: "tailscale".to_owned(),
            to: "guest".to_owned(),
            action: ZoneAction::Block,
        },
        ZonePolicy {
            from: "iot".to_owned(),
            to: "tailscale".to_owned(),
            action: ZoneAction::Allow,
        },
    ]);
    let rendered = ok(run(&site, GATEWAY, NodeRole::Gateway, &gateway_facts()));
    assert_eq!(
        forwardings(&rendered),
        pairs(&[
            ("lan", "tailscale"),
            ("iot", "tailscale"),
            ("tailscale", "lan"),
            ("tailscale", "iot"),
            ("tailscale", "wan"),
        ])
    );
}

#[test]
fn off_or_on_an_ap_it_renders_nothing() {
    let mut site = joined();
    if let Some(tailscale) = &mut site.vpn.tailscale {
        tailscale.enabled = false;
    }
    let rendered = ok(run(&site, GATEWAY, NodeRole::Gateway, &gateway_facts()));
    assert_eq!(sections(&rendered).trim(), "");
    assert_eq!(forwardings(&rendered), []);
    assert!(!rendered.requires.iter().any(|r| r.package == "tailscale"));
    let rendered = ok(run(&joined(), AP, NodeRole::Ap, &gateway_facts()));
    assert!(!rendered.requires.iter().any(|r| r.package == "tailscale"));
    assert!(
        rendered
            .packages
            .get(&ManagedPackage::Firewall)
            .is_none_or(|f| !f.to_file().contains("tailscale"))
    );
}
