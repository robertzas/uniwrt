use std::{collections::BTreeSet, net::Ipv4Addr};

use uniwrt_core::MacAddr;

use super::{free_vlan, network, pool, reservation, subnet};
use crate::site::{
    SiteConfig,
    network::{Purpose, Reservation},
    validate::{Severity, validate},
};

fn site() -> SiteConfig {
    SiteConfig::defaults(Some("fd12:3456:789a::/48".parse().unwrap()))
}

fn ip(text: &str) -> Ipv4Addr {
    text.parse().unwrap()
}

#[test]
fn a_network_gets_the_slash_24_of_its_vlan() {
    let mut config = site();
    let iot = network(&config, Purpose::Iot, None).unwrap();
    assert_eq!((iot.id.as_str(), iot.name.as_str()), ("iot", "IoT"));
    assert_eq!(iot.vlan, Some(30));
    let v4 = iot.ipv4.as_ref().unwrap();
    assert_eq!(v4.address.to_string(), "10.0.30.1/24");
    let dhcp = v4.dhcp.as_ref().unwrap();
    assert_eq!(
        (dhcp.start, dhcp.end),
        (ip("10.0.30.100"), ip("10.0.30.249"))
    );
    config.networks.push(iot);
    let guest = network(&config, Purpose::Guest, None).unwrap();
    assert!(guest.isolation && !guest.mdns);
    assert_eq!(
        guest.ipv4.as_ref().unwrap().address.to_string(),
        "10.0.40.1/24"
    );
    config.networks.push(guest);
    // A second IoT network: the next free VLAN and its /24, numbered.
    let iot2 = network(&config, Purpose::Iot, None).unwrap();
    assert_eq!((iot2.id.as_str(), iot2.name.as_str()), ("iot2", "IoT 2"));
    assert_eq!(iot2.vlan, Some(31));
    assert_eq!(
        iot2.ipv4.as_ref().unwrap().address.to_string(),
        "10.0.31.1/24"
    );
    config.networks.push(iot2);
    let custom = network(&config, Purpose::Custom, Some(300)).unwrap();
    assert_eq!(custom.id, "net1");
    // VLAN 300 has no third octet in 10.0.0.0/16: the first free /24.
    assert_eq!(
        custom.ipv4.as_ref().unwrap().address.to_string(),
        "10.0.1.1/24"
    );
    config.networks.push(custom);
    let issues: Vec<_> = validate(&config, &BTreeSet::new())
        .into_iter()
        .filter(|i| i.severity == Severity::Error)
        .collect();
    assert_eq!(issues, vec![]);
}

#[test]
fn vlans_follow_the_purpose_then_the_next_free() {
    let mut config = site();
    assert_eq!(free_vlan(&config, Purpose::Mgmt), Some(99));
    assert_eq!(free_vlan(&config, Purpose::Custom), Some(50));
    config
        .networks
        .push(network(&config, Purpose::Custom, None).unwrap());
    assert_eq!(free_vlan(&config, Purpose::Custom), Some(60));
    // The wired backhaul's VLAN is never handed out.
    config.mesh.wired_vlan = 99;
    assert_eq!(free_vlan(&config, Purpose::Mgmt), Some(100));
}

#[test]
fn subnets_skip_taken_and_factory_space() {
    let mut config = site();
    config.addressing.ipv4 = "192.168.0.0/16".parse().unwrap();
    // VLAN 1 would be 192.168.1.0/24, which factory-default devices use: the first free.
    assert_eq!(
        subnet(&config, Some(1)).unwrap().to_string(),
        "192.168.0.1/24"
    );
    config.addressing.ipv4 = "10.0.0.0/24".parse().unwrap();
    assert_eq!(subnet(&config, Some(30)), None);
    config.addressing.ipv4 = "10.0.0.0/25".parse().unwrap();
    assert_eq!(subnet(&config, None), None);
}

#[test]
fn pools_scale_with_the_subnet() {
    let pool = |net: &str| pool(net.parse().unwrap()).map(|(a, b)| (a.to_string(), b.to_string()));
    assert_eq!(
        pool("10.0.0.1/24"),
        Some(("10.0.0.100".into(), "10.0.0.249".into()))
    );
    assert_eq!(
        pool("10.0.0.1/22"),
        Some(("10.0.0.100".into(), "10.0.3.249".into()))
    );
    assert_eq!(
        pool("10.0.0.1/26"),
        Some(("10.0.0.16".into(), "10.0.0.62".into()))
    );
    assert_eq!(pool("10.0.0.1/30"), None);
}

#[test]
fn reservations_keep_the_current_address_when_free() {
    let mut lan = site().networks.remove(0);
    assert_eq!(
        reservation(&lan, Some(ip("10.0.0.123"))),
        Some(ip("10.0.0.123"))
    );
    assert_eq!(
        reservation(&lan, Some(ip("10.0.0.1"))),
        Some(ip("10.0.0.32"))
    );
    assert_eq!(
        reservation(&lan, Some(ip("10.9.0.5"))),
        Some(ip("10.0.0.32"))
    );
    let dhcp = lan.ipv4.as_mut().unwrap().dhcp.as_mut().unwrap();
    dhcp.reservations.push(Reservation {
        mac: MacAddr::new([2, 0, 0, 0, 0, 1]),
        ip: ip("10.0.0.32"),
        name: None,
    });
    assert_eq!(reservation(&lan, None), Some(ip("10.0.0.33")));
    assert_eq!(
        reservation(&lan, Some(ip("10.0.0.32"))),
        Some(ip("10.0.0.33"))
    );
}
