//! Multicast (features/network.md, Other services; D-136): the gateway's mDNS repeater between
//! the networks that reflect mDNS, the firewall's way in for the untrusted ones, and each
//! WLAN's multicast to unicast.

// Tests and their fixtures may panic on what they set up.
#![allow(clippy::unwrap_used)]

mod support;

use std::collections::BTreeMap;

use uniwrt_model::{node::NodeRole, site::node::ManagedPackage};
use uniwrt_render::Rendered;
use uniwrt_uci::{Package, Section};

use support::*;

fn ok(result: Result<Rendered, Vec<uniwrt_render::Issue>>) -> Rendered {
    result.unwrap_or_else(|issues| panic!("{issues:?}"))
}

fn interfaces(rendered: &Rendered) -> Option<Vec<String>> {
    let package = rendered.packages.get(&ManagedPackage::MdnsRepeater)?;
    let main = package.section("main").unwrap();
    Some(
        main.get("interface")
            .map_or_else(Vec::new, |v| v.items().to_vec()),
    )
}

fn mdns_rules(rendered: &Rendered) -> Vec<String> {
    rendered.packages[&ManagedPackage::Firewall]
        .sections_of("rule")
        .filter(|rule| rule.option("dest_port") == Some("5353"))
        .map(|rule| rule.option("src").unwrap().to_owned())
        .collect()
}

#[test]
fn the_gateway_reflects_mdns_between_the_networks_that_have_it_on() {
    let mut site = home_site();
    for network in &mut site.networks {
        network.mdns = network.id != "guest";
    }
    let rendered = ok(run(&site, GATEWAY, NodeRole::Gateway, &gateway_facts()));
    assert_eq!(
        interfaces(&rendered).unwrap(),
        ["br-lan.1", "br-lan.30"],
        "the default network's VLAN, then IoT's"
    );
    assert_eq!(rendered.services.get("mdns-repeater"), Some(&true));
    assert!(
        rendered
            .requires
            .iter()
            .any(|r| r.package == "mdns-repeater")
    );
    // The default network reaches the gateway freely; IoT needs a rule.
    assert_eq!(mdns_rules(&rendered), ["iot"]);
}

#[test]
fn one_network_reflects_nothing_and_a_held_package_stays_empty_and_off() {
    let mut site = home_site();
    for network in &mut site.networks {
        network.mdns = network.id == "lan";
    }
    let rendered = ok(run(&site, GATEWAY, NodeRole::Gateway, &gateway_facts()));
    assert_eq!(interfaces(&rendered), None);
    assert!(!rendered.services.contains_key("mdns-repeater"));
    assert_eq!(mdns_rules(&rendered), Vec::<String>::new());

    let mut held = stock();
    let mut repeater = Package::new("mdns_repeater");
    repeater.sections =
        vec![Section::named("mdns_repeater", "main").with("interface", "br-lan.1 br-lan.30")];
    held.insert("mdns_repeater".to_owned(), repeater);
    let rendered = ok(run_on(
        &site,
        GATEWAY,
        NodeRole::Gateway,
        &gateway_facts(),
        &held,
    ));
    assert_eq!(interfaces(&rendered), Some(Vec::new()));
    assert_eq!(rendered.services.get("mdns-repeater"), Some(&false));
    assert!(
        !rendered
            .requires
            .iter()
            .any(|r| r.package == "mdns-repeater")
    );
}

#[test]
fn an_ap_never_runs_the_repeater() {
    let site = home_site();
    let rendered = ok(run(&site, AP, NodeRole::Ap, &gateway_facts()));
    assert_eq!(interfaces(&rendered), None);
    assert!(!rendered.services.contains_key("mdns-repeater"));
}

#[test]
fn multicast_to_unicast_is_written_only_when_off() {
    let mut site = home_site();
    site.wlans[0].multicast_to_unicast = false;
    let rendered = ok(run(&site, AP, NodeRole::Ap, &gateway_facts()));
    let flags: BTreeMap<String, Option<String>> = rendered.packages[&ManagedPackage::Wireless]
        .sections_of("wifi-iface")
        .filter(|iface| iface.option("mode") == Some("ap"))
        .map(|iface| {
            (
                iface.name.clone().unwrap(),
                iface.option("multicast_to_unicast").map(str::to_owned),
            )
        })
        .collect();
    let off = format!("uw_{}_", site.wlans[0].id);
    assert!(flags.keys().any(|name| name.starts_with(&off)));
    for (name, flag) in flags {
        let want = name.starts_with(&off).then(|| "0".to_owned());
        assert_eq!(flag, want, "{name}");
    }
}
