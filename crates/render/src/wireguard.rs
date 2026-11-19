//! WireGuard on the gateway (features/network.md, VPN; D-140), through netifd's `wireguard`
//! protocol: in `network` the interface `vpn` (the server's key, port and tunnel address)
//! and a `wireguard_vpn` section per enabled peer, `wg_<id>` (its keys, its address and, for
//! a site, the networks routed to it, where to dial it and a keepalive); in `firewall` the
//! `vpn` zone, whose row in the matrix is seeded like the default LAN's
//! ([`matrix`](crate::firewall::matrix)), and the WAN's rule for the port. `wireguard-tools`
//! brings the kernel module and the `wg` tool the protocol runs. A server that is off renders
//! nothing, so netifd takes the tunnel down.

use uniwrt_model::{
    node::NodeRole,
    site::{
        SiteConfig,
        vpn::{Peer, PeerRole, SITE_KEEPALIVE_S, WireGuard, ZONE},
    },
};
use uniwrt_uci::Section;

use crate::{Issue, Requirement, Secrets, ext::SectionExt, plan::Plan};

/// The `wg` tool, which brings `kmod-wireguard` and netifd's protocol script.
pub(crate) const REQUIRES: Requirement = Requirement {
    package: "wireguard-tools",
    service: None,
};

/// The UCI type of the tunnel's peers.
const PEER_TYPE: &str = "wireguard_vpn";

/// The server with its keys from the vault.
pub(crate) struct Tunnel<'a> {
    pub server: &'a WireGuard,
    private_key: String,
    /// The enabled peers, each with its preshared key.
    peers: Vec<(&'a Peer, String)>,
}

/// The gateway's tunnel, where the server is on; a key the vault lacks is an error.
pub(crate) fn plan<'a>(
    site: &'a SiteConfig,
    secrets: &dyn Secrets,
    role: NodeRole,
    errors: &mut Vec<Issue>,
) -> Option<Tunnel<'a>> {
    let server = site.vpn.wireguard.as_ref().filter(|w| w.enabled)?;
    if role != NodeRole::Gateway {
        return None;
    }
    let mut lost = |path: String| {
        errors.push(Issue::new(path, "The key isn't set or isn't in the vault"));
    };
    let private_key = server.private_key.as_ref().and_then(|s| secrets.get(s));
    if private_key.is_none() {
        lost("vpn/wireguard/private_key".to_owned());
    }
    let mut peers = Vec::new();
    for peer in server.peers.iter().filter(|peer| peer.enabled) {
        match peer.preshared_key.as_ref().and_then(|s| secrets.get(s)) {
            Some(psk) => peers.push((peer, psk.to_owned())),
            None => lost(format!("vpn/wireguard/peers/{}/preshared_key", peer.id)),
        }
    }
    Some(Tunnel {
        server,
        private_key: private_key?.to_owned(),
        peers,
    })
}

/// The interface and its peers.
pub(crate) fn network(plan: &Plan<'_>) -> Vec<Section> {
    let Some(tunnel) = &plan.vpn else {
        return Vec::new();
    };
    let server = tunnel.server;
    let mut sections = vec![
        Section::named("interface", ZONE)
            .with("proto", "wireguard")
            .with("private_key", tunnel.private_key.as_str())
            .with("listen_port", server.port.to_string())
            .list("addresses", [server.address.to_string()]),
    ];
    sections.extend(
        tunnel
            .peers
            .iter()
            .map(|(peer, psk)| peer_section(peer, psk)),
    );
    sections
}

fn peer_section(peer: &Peer, psk: &str) -> Section {
    let mut allowed = vec![format!("{}/32", peer.address)];
    let section = Section::named(PEER_TYPE, format!("wg_{}", peer.id))
        .with("description", peer.name.as_str())
        .with("public_key", peer.public_key.as_str())
        .with("preshared_key", psk);
    match &peer.role {
        PeerRole::Client { .. } => section.list("allowed_ips", allowed),
        PeerRole::Site { networks, endpoint } => {
            allowed.extend(networks.iter().map(ToString::to_string));
            let section = section
                .list("allowed_ips", allowed)
                .with("route_allowed_ips", "1");
            let section = match endpoint {
                Some(endpoint) => section
                    .with("endpoint_host", endpoint.host.as_str())
                    .with("endpoint_port", endpoint.port.to_string()),
                None => section,
            };
            section.with("persistent_keepalive", SITE_KEEPALIVE_S.to_string())
        }
    }
}

/// The tunnel's zone: trusted, as the default LAN's.
pub(crate) fn zone(plan: &Plan<'_>) -> Option<Section> {
    plan.vpn.as_ref()?;
    Some(
        Section::anonymous("zone")
            .with("name", ZONE)
            .with("network", [ZONE])
            .with("input", "ACCEPT")
            .with("output", "ACCEPT")
            .with("forward", "ACCEPT"),
    )
}

/// The WAN's rule that lets peers reach the port.
pub(crate) fn rules(plan: &Plan<'_>, wan: &str) -> Option<Section> {
    let tunnel = plan.vpn.as_ref()?;
    Some(
        Section::anonymous("rule")
            .with("name", "Allow-WireGuard")
            .with("src", wan)
            .with("proto", "udp")
            .with("dest_port", tunnel.server.port.to_string())
            .with("target", "ACCEPT"),
    )
}
