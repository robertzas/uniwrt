//! Multicast (D-136): the networks the gateway's mDNS repeater serves and each WLAN's
//! multicast to unicast import exactly; a gateway without the repeater reflects nothing.

use uniwrt_model::{node::NodeRole, site::SiteConfig};

use super::{
    golden::round_trip,
    support::{AP, GATEWAY, gateway_facts, home_site},
};

fn mdns(site: &SiteConfig) -> Vec<(&str, bool)> {
    site.networks
        .iter()
        .map(|n| (n.id.as_str(), n.mdns))
        .collect()
}

#[test]
fn mdns_reflection_and_multicast_to_unicast_import_exactly() {
    let mut site = home_site();
    for network in &mut site.networks {
        network.mdns = network.id != "guest";
    }
    site.wlans[0].multicast_to_unicast = false;
    let import = round_trip(
        &site,
        &[
            (GATEWAY, NodeRole::Gateway, gateway_facts()),
            (AP, NodeRole::Ap, gateway_facts()),
        ],
    );
    assert_eq!(mdns(&import.site), mdns(&site));
    let flags = |site: &SiteConfig| -> Vec<(String, bool)> {
        site.wlans
            .iter()
            .map(|w| (w.ssid.clone(), w.multicast_to_unicast))
            .collect()
    };
    assert_eq!(flags(&import.site), flags(&site));
}

#[test]
fn a_gateway_without_the_repeater_reflects_nothing() {
    let import = round_trip(
        &home_site(),
        &[(GATEWAY, NodeRole::Gateway, gateway_facts())],
    );
    assert!(import.site.networks.iter().all(|n| !n.mdns));
}
