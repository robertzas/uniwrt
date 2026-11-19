//! The gateway's WireGuard server (features/network.md, VPN; D-140): the tunnel and its peers in
//! `network`, the `vpn` zone, its matrix row and the WAN's rule in `firewall`, and nothing
//! while it is off or on an AP.

// Tests and their fixtures may panic on what they set up.
#![allow(clippy::unwrap_used)]

mod support;

use std::net::Ipv4Addr;

use insta::assert_snapshot;
use uniwrt_model::{
    node::NodeRole,
    site::{
        SiteConfig,
        firewall::{ZoneAction, ZonePolicy},
        node::ManagedPackage,
        vpn::{Endpoint, Peer, PeerRole, SiteEndpoint, WireGuard},
    },
};
use uniwrt_render::Rendered;
use uniwrt_uci::Package;

use support::*;

fn ok(result: Result<Rendered, Vec<uniwrt_render::Issue>>) -> Rendered {
    result.unwrap_or_else(|issues| panic!("{issues:?}"))
}

fn peer(id: &str, n: u8, key: &str, role: PeerRole) -> Peer {
    Peer {
        id: id.to_owned(),
        name: format!("The {id}"),
        enabled: true,
        address: Ipv4Addr::new(10, 0, 250, n),
        public_key: key.to_owned(),
        private_key: None,
        preshared_key: Some(secret(7)),
        role,
    }
}

/// The home site with a server: a phone, a cabin's router it dials, and a laptop that is off.
fn tunneled() -> SiteConfig {
    let mut site = home_site();
    let mut server = WireGuard::new(Endpoint::Host {
        host: "vpn.example.com".to_owned(),
    });
    server.private_key = Some(secret(6));
    server.public_key = Some("HIgo9xNzJMWLKASShiTqIybxZ0U3wGLiUeJ1PKf8ykw=".to_owned());
    let mut cabin = peer(
        "cabin",
        3,
        "TrMvSoP4jYQlY6RIzBgbssQqY3vxI2Pi+y71lOWWXX0=",
        PeerRole::Site {
            networks: vec!["192.168.50.0/24".parse().unwrap()],
            endpoint: Some(SiteEndpoint {
                host: "cabin.example.net".to_owned(),
                port: 51821,
            }),
        },
    );
    cabin.preshared_key = Some(secret(8));
    let mut laptop = peer(
        "laptop",
        4,
        "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=",
        PeerRole::Client { full_tunnel: false },
    );
    laptop.enabled = false;
    server.peers = vec![
        peer(
            "phone",
            2,
            "gN65BkIKy1eCE9pP1wdc8ROUtkHLF2PfAqYdyYBz6EA=",
            PeerRole::Client { full_tunnel: true },
        ),
        cabin,
        laptop,
    ];
    site.vpn.wireguard = Some(server);
    site
}

/// The VPN's sections of `rendered`: the tunnel and its peers, then the zone, the forwardings
/// to or from it, and the WAN's rule.
fn vpn_sections(rendered: &Rendered) -> String {
    let network = &rendered.packages[&ManagedPackage::Network];
    let firewall = &rendered.packages[&ManagedPackage::Firewall];
    let mut package = Package::new("vpn");
    package.sections = network
        .sections
        .iter()
        .filter(|s| s.name.as_deref() == Some("vpn") || s.kind == "wireguard_vpn")
        .chain(firewall.sections.iter().filter(|s| {
            s.option("name") == Some("vpn")
                || s.option("name") == Some("Allow-WireGuard")
                || (s.kind == "forwarding"
                    && (s.option("src") == Some("vpn") || s.option("dest") == Some("vpn")))
        }))
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
        .filter(|(src, dest)| src == "vpn" || dest == "vpn")
        .collect()
}

#[test]
fn gateway_wireguard() {
    let rendered = ok(run(
        &tunneled(),
        GATEWAY,
        NodeRole::Gateway,
        &gateway_facts(),
    ));
    assert_snapshot!("gateway_wireguard", vpn_sections(&rendered));
    assert!(
        rendered
            .requires
            .iter()
            .any(|r| r.package == "wireguard-tools")
    );
}

#[test]
fn the_tunnel_s_row_is_seeded_like_the_lan_s_and_follows_the_policies() {
    let pairs = |list: &[(&str, &str)]| -> Vec<(String, String)> {
        list.iter()
            .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
            .collect()
    };
    let rendered = ok(run(
        &tunneled(),
        GATEWAY,
        NodeRole::Gateway,
        &gateway_facts(),
    ));
    // The default LAN reaches the tunnel, and the tunnel every network and the WAN.
    assert_eq!(
        forwardings(&rendered),
        pairs(&[
            ("lan", "vpn"),
            ("vpn", "lan"),
            ("vpn", "iot"),
            ("vpn", "guest"),
            ("vpn", "wan"),
        ])
    );
    let mut site = tunneled();
    site.firewall.policies.extend([
        ZonePolicy {
            from: "vpn".to_owned(),
            to: "guest".to_owned(),
            action: ZoneAction::Block,
        },
        ZonePolicy {
            from: "iot".to_owned(),
            to: "vpn".to_owned(),
            action: ZoneAction::Allow,
        },
    ]);
    let rendered = ok(run(&site, GATEWAY, NodeRole::Gateway, &gateway_facts()));
    assert_eq!(
        forwardings(&rendered),
        pairs(&[
            ("lan", "vpn"),
            ("iot", "vpn"),
            ("vpn", "lan"),
            ("vpn", "iot"),
            ("vpn", "wan"),
        ])
    );
}

#[test]
fn nothing_renders_while_the_server_is_off_or_on_an_ap() {
    let mut site = tunneled();
    let ap = ok(run(&site, AP, NodeRole::Ap, &gateway_facts()));
    site.vpn.wireguard.as_mut().unwrap().enabled = false;
    let off = ok(run(&site, GATEWAY, NodeRole::Gateway, &gateway_facts()));
    for rendered in [&ap, &off] {
        assert_eq!(vpn_sections(rendered).trim(), "");
        assert!(
            rendered
                .requires
                .iter()
                .all(|r| r.package != "wireguard-tools")
        );
    }
}

#[test]
fn a_key_the_vault_lacks_is_an_error() {
    let mut site = tunneled();
    let server = site.vpn.wireguard.as_mut().unwrap();
    server.private_key = Some(secret(9));
    server.peers[0].preshared_key = Some(secret(9));
    // A peer that is off needs no key.
    server.peers[2].preshared_key = None;
    let issues = run(&site, GATEWAY, NodeRole::Gateway, &gateway_facts()).unwrap_err();
    let paths: Vec<&str> = issues.iter().map(|i| i.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "vpn/wireguard/private_key",
            "vpn/wireguard/peers/phone/preshared_key"
        ]
    );
}
