//! The gateway's WireGuard server (features/network.md, VPN; D-140): the tunnel and its peers in
//! `network`, the `vpn` zone, its matrix row and the WAN's rule in `firewall`, and nothing
//! while it is off or on an AP; the peers' `.conf` files.

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
use uniwrt_render::{PeerKeys, Rendered, peer_conf};
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

const PHONE_KEY: &str = "cGhvbmUtcHJpdmF0ZS1rZXktMzItYnl0ZXMtbG9uZyE=";

#[test]
fn a_full_tunnel_client_s_configuration_routes_everything() {
    let site = tunneled();
    let server = site.vpn.wireguard.as_ref().unwrap();
    let keys = PeerKeys {
        private_key: Some(PHONE_KEY),
        preshared_key: "psk",
    };
    let conf = peer_conf(&site, &server.peers[0], keys).unwrap();
    assert_eq!(
        conf,
        "# The phone (uniwrt: WireGuard peer phone)\n\
         [Interface]\n\
         PrivateKey = cGhvbmUtcHJpdmF0ZS1rZXktMzItYnl0ZXMtbG9uZyE=\n\
         Address = 10.0.250.2/32\n\
         DNS = 10.0.250.1\n\
         \n\
         [Peer]\n\
         PublicKey = HIgo9xNzJMWLKASShiTqIybxZ0U3wGLiUeJ1PKf8ykw=\n\
         PresharedKey = psk\n\
         Endpoint = vpn.example.com:51820\n\
         AllowedIPs = 0.0.0.0/0, ::/0\n\
         PersistentKeepalive = 25\n"
    );
}

fn line<'a>(conf: &'a str, key: &str) -> Option<&'a str> {
    conf.lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix(" = "))
}

#[test]
fn a_split_tunnel_routes_what_the_vpn_zone_reaches_and_the_sites() {
    let mut site = tunneled();
    let keys = PeerKeys {
        private_key: None,
        preshared_key: "psk",
    };
    let laptop = site.vpn.wireguard.as_ref().unwrap().peers[2].clone();
    let conf = peer_conf(&site, &laptop, keys).unwrap();
    assert_eq!(
        line(&conf, "AllowedIPs"),
        Some("10.0.250.0/24, 10.0.0.0/24, 10.0.30.0/24, 10.0.40.0/24, 192.168.50.0/24")
    );
    // A peer with its own keypair: the controller has no private key to write.
    assert_eq!(line(&conf, "PrivateKey"), None);
    assert!(conf.contains("# PrivateKey = this device's own private key"));

    // A blocked cell leaves its network out, a disabled site its networks; no DNS when the
    // server offers none.
    site.firewall.policies.push(ZonePolicy {
        from: "vpn".to_owned(),
        to: "guest".to_owned(),
        action: ZoneAction::Block,
    });
    let server = site.vpn.wireguard.as_mut().unwrap();
    server.peers[1].enabled = false;
    server.dns = false;
    let conf = peer_conf(&site, &laptop, keys).unwrap();
    assert_eq!(
        line(&conf, "AllowedIPs"),
        Some("10.0.250.0/24, 10.0.0.0/24, 10.0.30.0/24")
    );
    assert_eq!(line(&conf, "DNS"), None);
}

#[test]
fn a_site_s_configuration_listens_where_it_is_dialed_and_routes_the_site() {
    let mut site = tunneled();
    let keys = PeerKeys {
        private_key: Some(PHONE_KEY),
        preshared_key: "psk",
    };
    let server = site.vpn.wireguard.as_mut().unwrap();
    server.endpoint = Endpoint::Host {
        host: "2001:db8::1".to_owned(),
    };
    let cabin = server.peers[1].clone();
    let conf = peer_conf(&site, &cabin, keys).unwrap();
    assert_eq!(line(&conf, "ListenPort"), Some("51821"));
    assert_eq!(line(&conf, "DNS"), None);
    assert_eq!(line(&conf, "Endpoint"), Some("[2001:db8::1]:51820"));
    // Its own networks aren't routed back to it.
    assert_eq!(
        line(&conf, "AllowedIPs"),
        Some("10.0.250.0/24, 10.0.0.0/24, 10.0.30.0/24, 10.0.40.0/24")
    );
}

#[test]
fn a_configuration_needs_the_server_s_key_and_its_endpoint() {
    let mut site = tunneled();
    let keys = PeerKeys {
        private_key: None,
        preshared_key: "psk",
    };
    let server = site.vpn.wireguard.as_mut().unwrap();
    let phone = server.peers[0].clone();
    server.endpoint = Endpoint::Ddns {
        id: "gone".to_owned(),
    };
    let issue = peer_conf(&site, &phone, keys).unwrap_err();
    assert_eq!(issue.path, "vpn/wireguard/endpoint");
    site.vpn.wireguard.as_mut().unwrap().public_key = None;
    let issue = peer_conf(&site, &phone, keys).unwrap_err();
    assert_eq!(issue.path, "vpn/wireguard/public_key");
    site.vpn.wireguard = None;
    assert_eq!(
        peer_conf(&site, &phone, keys).unwrap_err().path,
        "vpn/wireguard"
    );
}
