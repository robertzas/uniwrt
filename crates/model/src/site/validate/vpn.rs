//! The VPN (D-140): the WireGuard server's port, tunnel subnet, endpoint and keys, and its
//! peers' addresses, keys and routes.

use std::{collections::BTreeSet, net::IpAddr};

use uniwrt_core::Ipv4Net;

use super::{
    Issues,
    names::{is_dns_name, is_id},
    network::{CGNAT, FACTORY, overlap},
};
use crate::site::{
    SiteConfig,
    firewall::ForwardProto,
    gateway::WanProto,
    vpn::{Endpoint, MAX_PEERS, PeerRole, WireGuard, ZONE, is_key},
};

/// The longest peer name.
const NAME_MAX: usize = 48;

/// The most networks a site peer routes.
const MAX_NETWORKS: usize = 16;

pub(super) fn vpn(config: &SiteConfig, issues: &mut Issues) {
    let Some(wireguard) = &config.vpn.wireguard else {
        return;
    };
    let at = "vpn/wireguard";
    if config.networks.iter().any(|network| network.id == ZONE) {
        issues.error(
            at,
            format!("A network has the id {ZONE}, which the VPN's interface and zone take"),
        );
    }
    port(config, wireguard, issues);
    if let Err(why) = tunnel(config, wireguard.address) {
        issues.error(format!("{at}/address"), why);
    }
    endpoint(config, &wireguard.endpoint, issues);
    match (&wireguard.private_key, &wireguard.public_key) {
        (Some(_), Some(public)) if !is_key(public) => {
            issues.error(
                format!("{at}/public_key"),
                "The public key isn't a WireGuard key",
            );
        }
        (Some(_), Some(_)) => {}
        (None, None) if !wireguard.enabled => {}
        _ => issues.error(at, "The server needs its keypair"),
    }
    peers(config, wireguard, issues);
}

fn port(config: &SiteConfig, wireguard: &WireGuard, issues: &mut Issues) {
    let at = "vpn/wireguard/port";
    if wireguard.port == 0 {
        issues.error(at, "A port is 1 to 65535");
        return;
    }
    let forwarded = config.firewall.forwards.iter().find(|forward| {
        forward.enabled
            && forward.proto != ForwardProto::Tcp
            && (forward.ports.first..=forward.ports.last).contains(&wireguard.port)
    });
    if let Some(forward) = forwarded {
        issues.error(
            at,
            format!(
                "The port forward {} takes UDP port {}",
                forward.name, wireguard.port
            ),
        );
    }
}

/// Why `address` can't be the gateway's in the tunnel, if it can't.
fn tunnel(config: &SiteConfig, address: Ipv4Net) -> Result<(), String> {
    let subnet = address.trunc();
    if !(16..=30).contains(&address.prefix_len()) {
        return Err("The tunnel's prefix is /16 to /30".to_owned());
    }
    if !address.addr().is_private() {
        return Err("The tunnel uses private (RFC 1918) addresses".to_owned());
    }
    if address.addr() == subnet.network() || address.addr() == subnet.broadcast() {
        return Err(format!("The gateway's address is a host of {subnet}"));
    }
    if let Some(network) = config.networks.iter().find(|network| {
        network
            .ipv4
            .as_ref()
            .is_some_and(|ipv4| overlap(ipv4.address.trunc(), subnet))
    }) {
        return Err(format!(
            "The tunnel's subnet overlaps network {}'s",
            network.id
        ));
    }
    if overlap(subnet, CGNAT) {
        return Err("The tunnel's subnet overlaps CGNAT space (100.64.0.0/10)".to_owned());
    }
    if overlap(subnet, FACTORY) {
        return Err("Keep 192.168.1.0/24 free for factory-default OpenWrt devices".to_owned());
    }
    if let WanProto::Static { address, .. } = &config.wan.proto
        && overlap(subnet, address.trunc())
    {
        return Err("The tunnel's subnet overlaps the WAN's".to_owned());
    }
    Ok(())
}

fn endpoint(config: &SiteConfig, endpoint: &Endpoint, issues: &mut Issues) {
    let at = "vpn/wireguard/endpoint";
    match endpoint {
        Endpoint::Ddns { id } => match config.services.ddns.iter().find(|ddns| ddns.id == *id) {
            None => issues.error(at, format!("There is no DDNS name {id}")),
            Some(ddns) if !ddns.enabled => issues.warn(
                at,
                format!(
                    "The DDNS name {} is off: peers may not find the gateway",
                    ddns.hostname
                ),
            ),
            Some(_) => {}
        },
        Endpoint::Host { host } => {
            if !is_host(host) {
                issues.error(at, "The endpoint is a DNS name or an IP address");
            }
        }
    }
}

/// A DNS name or an IP address that a peer can dial.
fn is_host(host: &str) -> bool {
    host.parse::<IpAddr>().is_ok() || (host.contains('.') && is_dns_name(host))
}

fn peers(config: &SiteConfig, wireguard: &WireGuard, issues: &mut Issues) {
    let at = "vpn/wireguard/peers";
    if wireguard.peers.len() > MAX_PEERS {
        issues.error(at, format!("A server has up to {MAX_PEERS} peers"));
    }
    let subnet = wireguard.address.trunc();
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut addresses = BTreeSet::from([wireguard.address.addr()]);
    let mut keys: BTreeSet<&str> = wireguard.public_key.iter().map(String::as_str).collect();
    let mut routed: Vec<(&str, Ipv4Net)> = Vec::new();
    for peer in &wireguard.peers {
        let at = format!("{at}/{}", peer.id);
        if !is_id(&peer.id) {
            issues.error(
                &at,
                "A peer's id is a lowercase letter, then up to 11 lowercase letters, digits or \
                 underscores",
            );
        }
        if !ids.insert(peer.id.as_str()) {
            issues.error(&at, format!("Two peers have the id {}", peer.id));
        }
        let name = peer.name.trim();
        if name.is_empty() || name.chars().count() > NAME_MAX || name.chars().any(char::is_control)
        {
            issues.error(
                format!("{at}/name"),
                format!("A peer needs a name of up to {NAME_MAX} characters"),
            );
        } else if !names.insert(name.to_lowercase()) {
            issues.error(format!("{at}/name"), format!("Two peers are named {name}"));
        }
        let address = peer.address;
        if !subnet.contains(&address)
            || address == subnet.network()
            || address == subnet.broadcast()
        {
            issues.error(
                format!("{at}/address"),
                format!("A peer's address is a host of the tunnel's {subnet}"),
            );
        } else if !addresses.insert(address) {
            issues.error(
                format!("{at}/address"),
                format!("{address} is taken in the tunnel"),
            );
        }
        if !is_key(&peer.public_key) {
            issues.error(
                format!("{at}/public_key"),
                "The public key isn't a WireGuard key",
            );
        } else if !keys.insert(peer.public_key.as_str()) {
            issues.error(
                format!("{at}/public_key"),
                "Another peer or the server has this public key",
            );
        }
        if peer.preshared_key.is_none() {
            issues.error(
                format!("{at}/preshared_key"),
                "The peer needs a preshared key",
            );
        }
        if let PeerRole::Site { networks, endpoint } = &peer.role {
            let at = format!("{at}/role");
            if networks.is_empty() || networks.len() > MAX_NETWORKS {
                issues.error(
                    format!("{at}/networks"),
                    format!("A site routes 1 to {MAX_NETWORKS} networks"),
                );
            }
            for (i, &network) in networks.iter().enumerate() {
                let path = format!("{at}/networks/{i}");
                if network != network.trunc() {
                    issues.error(
                        &path,
                        format!("A network is written as {}", network.trunc()),
                    );
                } else if let Some(why) = clash(config, wireguard, network, &routed) {
                    issues.error(&path, why);
                }
                routed.push((peer.name.as_str(), network));
            }
            if let Some(endpoint) = endpoint {
                if !is_host(&endpoint.host) {
                    issues.error(
                        format!("{at}/endpoint/host"),
                        "The endpoint is a DNS name or an IP address",
                    );
                }
                if endpoint.port == 0 {
                    issues.error(format!("{at}/endpoint/port"), "A port is 1 to 65535");
                }
            }
        }
    }
}

/// What a site peer's `network` collides with, if anything: the site's networks, the tunnel, or
/// what another peer routes.
fn clash(
    config: &SiteConfig,
    wireguard: &WireGuard,
    network: Ipv4Net,
    routed: &[(&str, Ipv4Net)],
) -> Option<String> {
    if let Some(local) = config.networks.iter().find(|local| {
        local
            .ipv4
            .as_ref()
            .is_some_and(|ipv4| overlap(ipv4.address.trunc(), network))
    }) {
        return Some(format!("{network} overlaps network {}'s subnet", local.id));
    }
    if overlap(network, wireguard.address.trunc()) {
        return Some(format!("{network} overlaps the tunnel's subnet"));
    }
    routed
        .iter()
        .find(|(_, other)| overlap(*other, network))
        .map(|(peer, _)| format!("{network} overlaps what {peer} routes"))
}
