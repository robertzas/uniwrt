//! The VPN (D-140): its validation, secret slots, migration and the subnet IPAM keeps free.

use std::{collections::BTreeSet, net::Ipv4Addr};

use serde_json::{Value, json};
use uniwrt_core::NodeId;

use super::{
    SecretRef, SiteConfig, ipam,
    migrate::upgrade,
    validate::{Severity, validate},
    vpn::{Endpoint, PeerRole, SiteEndpoint},
};

fn fixture() -> SiteConfig {
    upgrade(serde_json::from_str(include_str!("../../fixtures/site/v17.json")).unwrap()).unwrap()
}

/// The issues of the fixture after `edit`, as (path, severity).
fn issues_after(edit: impl FnOnce(&mut SiteConfig)) -> Vec<(String, Severity)> {
    let mut config = fixture();
    edit(&mut config);
    let nodes: BTreeSet<NodeId> = [NodeId::new(1), NodeId::new(2)].into();
    validate(&config, &nodes)
        .into_iter()
        .filter(|issue| issue.path.starts_with("vpn") || issue.path.starts_with("firewall"))
        .map(|issue| (issue.path, issue.severity))
        .collect()
}

fn errors(list: &[&str]) -> Vec<(String, Severity)> {
    list.iter()
        .map(|path| ((*path).to_owned(), Severity::Error))
        .collect()
}

#[test]
fn the_server_and_its_peers_validate() {
    assert_eq!(issues_after(|_| {}), []);
}

#[test]
fn the_port_the_tunnel_and_the_endpoint_are_checked() {
    let at = |path: &str| format!("vpn/wireguard/{path}");
    // The game server's forward takes UDP 27015–27030.
    let issues = issues_after(|c| c.vpn.wireguard.as_mut().unwrap().port = 27020);
    assert_eq!(issues, errors(&[&at("port")]));
    for address in [
        "10.0.0.9/24",
        "10.0.250.0/24",
        "10.0.250.1/31",
        "100.64.0.1/24",
    ] {
        let issues = issues_after(|c| {
            c.vpn.wireguard.as_mut().unwrap().address = address.parse().unwrap();
            for peer in &mut c.vpn.wireguard.as_mut().unwrap().peers {
                peer.address = "10.0.0.250".parse().unwrap();
            }
        });
        assert!(
            issues.contains(&(at("address"), Severity::Error)),
            "{address}"
        );
    }
    let issues = issues_after(|c| {
        c.vpn.wireguard.as_mut().unwrap().endpoint = Endpoint::Ddns {
            id: "duck".to_owned(),
        };
    });
    assert_eq!(issues, [(at("endpoint"), Severity::Warning)]);
    for endpoint in [
        Endpoint::Ddns {
            id: "none".to_owned(),
        },
        Endpoint::Host {
            host: "localhost".to_owned(),
        },
    ] {
        let issues = issues_after(|c| c.vpn.wireguard.as_mut().unwrap().endpoint = endpoint);
        assert_eq!(issues, errors(&[&at("endpoint")]));
    }
    let issues = issues_after(|c| {
        c.vpn.wireguard.as_mut().unwrap().endpoint = Endpoint::Host {
            host: "203.0.113.7".to_owned(),
        };
    });
    assert_eq!(issues, []);
}

#[test]
fn the_server_needs_its_keypair_while_on() {
    let issues = issues_after(|c| {
        let server = c.vpn.wireguard.as_mut().unwrap();
        server.private_key = None;
        server.public_key = None;
    });
    assert_eq!(issues, errors(&["vpn/wireguard"]));
    let issues = issues_after(|c| {
        let server = c.vpn.wireguard.as_mut().unwrap();
        server.private_key = None;
        server.public_key = None;
        server.enabled = false;
    });
    assert_eq!(issues, []);
    let issues = issues_after(|c| {
        c.vpn.wireguard.as_mut().unwrap().public_key = Some("not a key".to_owned());
    });
    assert_eq!(issues, errors(&["vpn/wireguard/public_key"]));
}

#[test]
fn peers_have_their_own_address_key_and_routes() {
    let peer = |path: &str| format!("vpn/wireguard/peers/{path}");
    let issues = issues_after(|c| {
        let server = c.vpn.wireguard.as_mut().unwrap();
        let server_key = server.public_key.clone().unwrap();
        let [phone, cabin] = server.peers.as_mut_slice() else {
            unreachable!()
        };
        phone.address = Ipv4Addr::new(10, 0, 250, 1);
        phone.public_key = server_key;
        phone.preshared_key = None;
        cabin.address = Ipv4Addr::new(10, 0, 251, 3);
        cabin.name = " Phone".to_owned();
        cabin.role = PeerRole::Site {
            networks: vec![
                "10.0.30.0/24".parse().unwrap(),
                "192.168.60.1/24".parse().unwrap(),
                "10.0.250.0/23".parse().unwrap(),
            ],
            endpoint: Some(SiteEndpoint {
                host: "cabin".to_owned(),
                port: 0,
            }),
        };
    });
    assert_eq!(
        issues,
        errors(&[
            &peer("phone/address"),
            &peer("phone/public_key"),
            &peer("phone/preshared_key"),
            &peer("cabin/address"),
            &peer("cabin/role/networks/0"),
            &peer("cabin/role/networks/1"),
            &peer("cabin/role/networks/2"),
            &peer("cabin/role/endpoint/host"),
            &peer("cabin/role/endpoint/port"),
        ])
    );
    let issues = issues_after(|c| {
        let peers = &mut c.vpn.wireguard.as_mut().unwrap().peers;
        let mut twin = peers[1].clone();
        twin.id = "cabin".to_owned();
        twin.address = Ipv4Addr::new(10, 0, 250, 9);
        twin.public_key = "yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=".to_owned();
        peers.push(twin);
    });
    assert_eq!(
        issues,
        errors(&[
            &peer("cabin"),
            &peer("cabin/name"),
            &peer("cabin/role/networks/0"),
        ])
    );
}

#[test]
fn the_vpn_zone_exists_only_with_the_server() {
    assert_eq!(
        issues_after(|c| c.vpn.wireguard = None),
        errors(&["firewall/policies/1/from"])
    );
    let issues = issues_after(|c| {
        let mut network = c.networks[1].clone();
        network.id = "vpn".to_owned();
        network.name = "VPN".to_owned();
        network.vlan = Some(99);
        network.ipv4 = None;
        c.networks.push(network);
    });
    assert!(issues.contains(&("vpn/wireguard".to_owned(), Severity::Error)));
}

#[test]
fn the_server_and_peer_keys_are_secret_slots() {
    let mut config = fixture();
    let paths: Vec<String> = config
        .secrets()
        .into_iter()
        .map(|slot| slot.path)
        .filter(|path| path.starts_with("vpn"))
        .collect();
    assert_eq!(
        paths,
        [
            "vpn/wireguard/private_key",
            "vpn/wireguard/peers/phone/private_key",
            "vpn/wireguard/peers/phone/preshared_key",
            "vpn/wireguard/peers/cabin/private_key",
            "vpn/wireguard/peers/cabin/preshared_key",
        ]
    );
    let psk = config
        .secret("vpn/wireguard/peers/cabin/preshared_key")
        .cloned()
        .unwrap();
    let sealed = SecretRef::numbered(0xabc);
    config.replace_secret(&psk, &sealed);
    assert_eq!(
        config.secret("vpn/wireguard/peers/cabin/preshared_key"),
        Some(&sealed)
    );
    *config
        .secret_mut("vpn/wireguard/peers/cabin/private_key")
        .unwrap() = Some(psk.clone());
    assert_eq!(
        config.vpn.wireguard.as_ref().unwrap().peers[1].private_key,
        Some(psk)
    );
    assert!(
        config
            .secret_mut("vpn/wireguard/peers/cabin/public_key")
            .is_none()
    );
    let key = "yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=";
    assert_eq!(
        super::check_secret("vpn/wireguard/private_key", key),
        Ok(())
    );
    assert!(super::check_secret("vpn/wireguard/private_key", "hunter22").is_err());
}

#[test]
fn v17_drops_wireguard_from_the_managed_packages() {
    let mut v16: Value =
        serde_json::from_str(include_str!("../../fixtures/site/v16.json")).unwrap();
    v16["nodes"][0]["managed"]
        .as_array_mut()
        .unwrap()
        .push(json!("wireguard"));
    let config = upgrade(v16).unwrap();
    assert_eq!(config.vpn.wireguard, None);
    assert!(
        config.nodes[0]
            .managed
            .iter()
            .all(|p| p.name() != "wireguard")
    );
}

#[test]
fn new_networks_keep_off_the_tunnel() {
    let config = fixture();
    let subnet = ipam::subnet(&config, Some(250)).unwrap();
    assert_ne!(subnet.trunc(), "10.0.250.0/24".parse().unwrap());
}
