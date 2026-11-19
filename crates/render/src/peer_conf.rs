//! A peer's WireGuard configuration (features/network.md, VPN; D-140): the `.conf` file that
//! `wg-quick` and the WireGuard apps import, which the UI also shows as a QR code.
//!
//! - **`[Interface]`:** the peer's private key (a peer that brought its own keypair gets a
//!   comment in its place), its address in the tunnel as a /32, the gateway's tunnel address as
//!   DNS for a client where the server offers it, and a site's `ListenPort` where the gateway
//!   dials it.
//! - **`[Peer]`:** the gateway: its public key, the preshared key, its endpoint (`host:port`, an
//!   IPv6 address in brackets), the routes, and a 25 s keepalive that keeps NAT mappings open.
//! - **Routes:** a full tunnel routes `0.0.0.0/0, ::/0`. A split tunnel and a site route the
//!   tunnel's subnet, the networks the `vpn` zone reaches in the matrix, and what the other
//!   enabled site peers route (the zone forwards within itself), each once.

use std::{fmt::Write as _, net::IpAddr};

use uniwrt_core::Ipv4Net;
use uniwrt_model::site::{
    SiteConfig,
    network::Purpose,
    vpn::{Endpoint, Peer, PeerRole, SITE_KEEPALIVE_S, WireGuard, ZONE},
};

use crate::{Issue, firewall};

/// The keys a peer's configuration carries, read from the vault.
#[derive(Debug, Clone, Copy)]
pub struct PeerKeys<'a> {
    /// `None` where the peer brought its own keypair.
    pub private_key: Option<&'a str>,
    pub preshared_key: &'a str,
}

/// `peer`'s configuration as `site`'s server sees it, or why there is none: no server, no
/// server keypair, or an endpoint naming a DDNS entry that is gone.
pub fn peer_conf(site: &SiteConfig, peer: &Peer, keys: PeerKeys<'_>) -> Result<String, Issue> {
    let at = "vpn/wireguard";
    let server = site
        .vpn
        .wireguard
        .as_ref()
        .ok_or_else(|| Issue::new(at, "The site has no WireGuard server"))?;
    let public_key = server
        .public_key
        .as_deref()
        .ok_or_else(|| Issue::new(format!("{at}/public_key"), "The server has no keypair yet"))?;
    let host = match &server.endpoint {
        Endpoint::Host { host } => host.as_str(),
        Endpoint::Ddns { id } => site
            .services
            .ddns
            .iter()
            .find(|ddns| ddns.id == *id)
            .map(|ddns| ddns.hostname.as_str())
            .ok_or_else(|| {
                Issue::new(
                    format!("{at}/endpoint"),
                    format!("There is no DDNS name {id}"),
                )
            })?,
    };
    let host = match host.parse::<IpAddr>() {
        Ok(IpAddr::V6(v6)) => format!("[{v6}]"),
        _ => host.to_owned(),
    };

    // Writing to a String never fails.
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# {} (uniwrt: WireGuard peer {})",
        peer.name.trim(),
        peer.id
    );
    out.push_str("[Interface]\n");
    match keys.private_key {
        Some(key) => {
            let _ = writeln!(out, "PrivateKey = {key}");
        }
        None => out.push_str(
            "# PrivateKey = this device's own private key (the controller has only its public key)\n",
        ),
    }
    let _ = writeln!(out, "Address = {}/32", peer.address);
    let routes = match &peer.role {
        PeerRole::Client { full_tunnel: true } => vec!["0.0.0.0/0".to_owned(), "::/0".to_owned()],
        PeerRole::Client { .. } | PeerRole::Site { .. } => routed(site, server, peer)
            .iter()
            .map(ToString::to_string)
            .collect(),
    };
    match &peer.role {
        PeerRole::Client { .. } if server.dns => {
            let _ = writeln!(out, "DNS = {}", server.address.addr());
        }
        PeerRole::Site {
            endpoint: Some(endpoint),
            ..
        } => {
            let _ = writeln!(out, "ListenPort = {}", endpoint.port);
        }
        _ => {}
    }
    out.push_str("\n[Peer]\n");
    let _ = writeln!(out, "PublicKey = {public_key}");
    let _ = writeln!(out, "PresharedKey = {}", keys.preshared_key);
    let _ = writeln!(out, "Endpoint = {host}:{}", server.port);
    let _ = writeln!(out, "AllowedIPs = {}", routes.join(", "));
    let _ = writeln!(out, "PersistentKeepalive = {SITE_KEEPALIVE_S}");
    Ok(out)
}

/// What a split tunnel or a site sends through the gateway: the tunnel's subnet, the networks
/// the `vpn` zone reaches, and the other enabled site peers' networks, each once, in that order.
fn routed(site: &SiteConfig, server: &WireGuard, peer: &Peer) -> Vec<Ipv4Net> {
    let reached = site
        .networks
        .iter()
        .filter(|network| firewall::allowed(site, ZONE, Some(Purpose::Default), &network.id))
        .filter_map(|network| network.ipv4.as_ref())
        .map(|ipv4| ipv4.address.trunc());
    let sites = server
        .peers
        .iter()
        .filter(|other| other.enabled && other.id != peer.id)
        .flat_map(|other| match &other.role {
            PeerRole::Site { networks, .. } => networks.as_slice(),
            PeerRole::Client { .. } => &[],
        })
        .copied();
    let mut routes: Vec<Ipv4Net> = Vec::new();
    for net in std::iter::once(server.address.trunc())
        .chain(reached)
        .chain(sites)
    {
        if !routes.contains(&net) {
            routes.push(net);
        }
    }
    routes
}
