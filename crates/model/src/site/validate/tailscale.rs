//! The gateway's Tailscale (D-140): the networks it advertises, its machine name, and the zone
//! and UDP port it takes.

use std::collections::BTreeSet;

use super::{Issues, names::is_hostname};
use crate::site::{
    SiteConfig,
    firewall::ForwardProto,
    vpn::tailscale::{PORT, ZONE},
};

pub(super) fn tailscale(config: &SiteConfig, issues: &mut Issues) {
    let Some(tailscale) = &config.vpn.tailscale else {
        return;
    };
    let at = "vpn/tailscale";
    if config.networks.iter().any(|network| network.id == ZONE) {
        issues.error(
            at,
            format!("A network has the id {ZONE}, which Tailscale's zone takes"),
        );
    }
    let mut seen = BTreeSet::new();
    for (i, id) in tailscale.routes.iter().enumerate() {
        let path = format!("{at}/routes/{i}");
        match config.networks.iter().find(|network| network.id == *id) {
            None => issues.error(path, format!("No network has the id {id}")),
            Some(network) if network.ipv4.is_none() => {
                issues.error(
                    path,
                    format!("The network {id} has no IPv4 subnet to route"),
                );
            }
            Some(_) if !seen.insert(id) => {
                issues.error(path, format!("The network {id} is listed twice"));
            }
            Some(_) => {}
        }
    }
    if let Some(name) = &tailscale.hostname
        && !is_hostname(name)
    {
        issues.error(
            format!("{at}/hostname"),
            "A machine name is one DNS label: letters, digits and -, up to 63",
        );
    }
    if !tailscale.enabled {
        return;
    }
    let forwarded = config.firewall.forwards.iter().find(|forward| {
        forward.enabled
            && forward.proto != ForwardProto::Tcp
            && (forward.ports.first..=forward.ports.last).contains(&PORT)
    });
    if let Some(forward) = forwarded {
        issues.error(
            at,
            format!(
                "The port forward {} takes Tailscale's UDP port {PORT}",
                forward.name
            ),
        );
    }
    let wireguard = config.vpn.wireguard.as_ref();
    if wireguard.is_some_and(|w| w.enabled && w.port == PORT) {
        issues.error(
            "vpn/wireguard/port",
            format!("Tailscale takes UDP port {PORT}"),
        );
    }
}
