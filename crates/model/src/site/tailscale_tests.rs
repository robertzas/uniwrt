//! The gateway's Tailscale (D-140): its validation, its auth key's slot and the v18 migration.

use std::collections::BTreeSet;

use serde_json::Value;
use uniwrt_core::NodeId;

use super::{
    SecretRef, SiteConfig, check_secret,
    migrate::upgrade,
    validate::{Severity, validate},
    vpn::tailscale::AUTH_KEY,
};

fn fixture() -> SiteConfig {
    upgrade(serde_json::from_str(include_str!("../../fixtures/site/v18.json")).unwrap()).unwrap()
}

/// The VPN's and firewall's errors of the fixture after `edit`, by path.
fn errors_after(edit: impl FnOnce(&mut SiteConfig)) -> Vec<String> {
    let mut config = fixture();
    edit(&mut config);
    let nodes: BTreeSet<NodeId> = [NodeId::new(1), NodeId::new(2)].into();
    validate(&config, &nodes)
        .into_iter()
        .filter(|issue| issue.severity == Severity::Error)
        .filter(|issue| issue.path.starts_with("vpn") || issue.path.starts_with("firewall"))
        .map(|issue| issue.path)
        .collect()
}

fn tailscale(config: &mut SiteConfig) -> &mut super::vpn::tailscale::Tailscale {
    config.vpn.tailscale.as_mut().unwrap()
}

#[test]
fn routes_name_networks_with_an_ipv4_subnet_once() {
    assert_eq!(errors_after(|_| {}), Vec::<String>::new());
    let errors = errors_after(|c| {
        tailscale(c).routes = ["lan", "nope", "guest", "lan"].map(str::to_owned).to_vec();
    });
    assert_eq!(
        errors,
        [
            "vpn/tailscale/routes/1",
            "vpn/tailscale/routes/2",
            "vpn/tailscale/routes/3"
        ]
    );
}

#[test]
fn the_machine_name_is_one_label() {
    assert_eq!(
        errors_after(|c| tailscale(c).hostname = Some("home.gateway".to_owned())),
        ["vpn/tailscale/hostname"]
    );
    assert_eq!(
        errors_after(|c| tailscale(c).hostname = Some("home-gw2".to_owned())),
        Vec::<String>::new()
    );
}

#[test]
fn its_zone_and_port_are_its_own() {
    // Renaming a network trips its references too: Tailscale's is the one this test is about.
    assert!(
        errors_after(|c| c.networks[1].id = "tailscale".to_owned())
            .contains(&"vpn/tailscale".to_owned())
    );
    assert_eq!(
        errors_after(|c| c.vpn.wireguard.as_mut().unwrap().port = 41641),
        ["vpn/wireguard/port"]
    );
    // Off, it takes no port.
    assert_eq!(
        errors_after(|c| {
            c.vpn.wireguard.as_mut().unwrap().port = 41641;
            tailscale(c).enabled = false;
        }),
        Vec::<String>::new()
    );
    // Its zone is known to the policies only while it is set up.
    assert_eq!(
        errors_after(|c| c.vpn.tailscale = None),
        ["firewall/policies/2/from"]
    );
}

#[test]
fn the_auth_key_is_a_secret_slot() {
    let mut config = fixture();
    let key = SecretRef::new("config/0000000000000000000000000000ee05").unwrap();
    assert_eq!(config.secret(AUTH_KEY), Some(&key));
    let other = SecretRef::new("config/0000000000000000000000000000ee09").unwrap();
    config.replace_secret(&key, &other);
    assert_eq!(config.secret(AUTH_KEY), Some(&other));
    *config.secret_mut(AUTH_KEY).unwrap() = None;
    assert!(
        config
            .secrets()
            .iter()
            .any(|slot| slot.path == AUTH_KEY && slot.value.is_none())
    );
    assert!(check_secret(AUTH_KEY, "tskey-auth-kBc1CNTRL-abcdefghij").is_ok());
    assert!(check_secret(AUTH_KEY, "hunter22hunter22").is_err());
    config.vpn.tailscale = None;
    assert!(config.secrets().iter().all(|slot| slot.path != AUTH_KEY));
    assert!(config.secret_mut(AUTH_KEY).is_none());
}

#[test]
fn v18_adds_no_tailscale() {
    let v17: Value = serde_json::from_str(include_str!("../../fixtures/site/v17.json")).unwrap();
    let config = upgrade(v17).unwrap();
    assert_eq!(config.vpn.tailscale, None);
    assert!(config.vpn.wireguard.is_some());
}
