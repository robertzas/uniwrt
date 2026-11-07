//! The sites the harness renders: a new site's defaults and a home site with VLANs, each behind
//! one of the ISP's uplinks (scripts/system/): DHCP untagged or on VLAN 201, static, or PPPoE;
//! and the hwsim lab's site (scripts/wifi-lab.sh), whose WLAN and mesh client c knows.

use std::{collections::BTreeMap, net::Ipv4Addr};

use uniwrt_core::{Ipv4Net, Ipv6Net, NodeId};
use uniwrt_model::site::{
    SecretRef, SiteConfig,
    gateway::{WanIpv6, WanProto},
    network::{
        Boot, Dhcp, DhcpOptions, Ipv4Config, Ipv6Clients, Ipv6Mode, Network, Purpose, Reservation,
    },
    node::{ManagedPackage, NodeConfig},
    wireless::{Channel, NodeScope, Wlan, WlanSecurity},
};
use uniwrt_model::wifi::Band;

pub const GATEWAY: NodeId = NodeId::new(1);
pub const AP: NodeId = NodeId::new(2);

/// The ISP's DHCP subnet on VLAN 201, its untagged one, and its PPPoE pool.
pub const ISP_VLAN_NET: &str = "198.51.100.";
pub const ISP_NET: &str = "192.0.2.";
pub const ISP_PPPOE: &str = "203.0.113.";

/// How the gateway reaches the ISP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Uplink {
    Dhcp,
    /// DHCP on VLAN 201, as Lumen serves it.
    DhcpVlan,
    Static,
    Pppoe,
}

pub fn secret(n: u8) -> SecretRef {
    SecretRef::new(format!("config/{n:032x}")).expect("a valid secret reference")
}

/// The vault's values of the sites' secrets; the PPPoE password is in scripts/system/pap-secrets.
pub fn secrets() -> BTreeMap<SecretRef, String> {
    BTreeMap::from([
        (secret(1), "correct horse battery".to_owned()),
        (secret(2), "things-only-2024".to_owned()),
        (secret(3), "mesh key 0123456789".to_owned()),
        (secret(4), "pppoe-pass".to_owned()),
        (secret(5), "uniwrt-lab-key".to_owned()),
        (secret(6), "uniwrt-lab-mesh-key".to_owned()),
    ])
}

fn managed(node: NodeId) -> NodeConfig {
    let mut config = NodeConfig::new(node);
    config.managed = [
        ManagedPackage::System,
        ManagedPackage::Network,
        ManagedPackage::Wireless,
        ManagedPackage::Dhcp,
        ManagedPackage::Firewall,
        ManagedPackage::Sqm,
        ManagedPackage::Mwan3,
        ManagedPackage::Upnpd,
    ]
    .into();
    config
}

/// A new site's defaults, with the gateway and the AP managed, behind `uplink`.
pub fn defaults(uplink: Uplink) -> SiteConfig {
    let ula: Option<Ipv6Net> = "fd12:3456:789a::/48".parse().ok();
    let mut site = SiteConfig::defaults(ula);
    site.nodes = vec![managed(GATEWAY), managed(AP)];
    site.wan.ipv6 = WanIpv6::Off; // the ISP serves no DHCPv6
    match uplink {
        Uplink::Dhcp => {}
        Uplink::DhcpVlan => site.wan.vlan = Some(201),
        Uplink::Static => {
            site.wan.proto = WanProto::Static {
                address: Ipv4Net::new(Ipv4Addr::new(192, 0, 2, 2), 24).expect("a /24"),
                gateway: Ipv4Addr::new(192, 0, 2, 1),
                dns: vec![Ipv4Addr::new(192, 0, 2, 1).into()],
            };
        }
        Uplink::Pppoe => {
            site.wan.proto = WanProto::Pppoe {
                username: "user@isp".to_owned(),
                password: Some(secret(4)),
                service: None,
            };
        }
    }
    site
}

fn network(id: &str, purpose: Purpose, vlan: u8, ipv6: Ipv6Mode) -> Network {
    Network {
        id: id.to_owned(),
        name: id.to_owned(),
        purpose,
        vlan: Some(vlan.into()),
        ipv4: Some(Ipv4Config {
            address: Ipv4Net::new(Ipv4Addr::new(10, 0, vlan, 1), 24).expect("a /24"),
            dhcp: Some(Dhcp {
                start: Ipv4Addr::new(10, 0, vlan, 100),
                end: Ipv4Addr::new(10, 0, vlan, 249),
                lease_time_s: 3600,
                reservations: Vec::new(),
                options: DhcpOptions::default(),
            }),
        }),
        ipv6,
        ipv6_clients: Ipv6Clients::default(),
        isolation: false,
        mdns: false,
        igmp_snooping: false,
    }
}

/// A home: the LAN untagged (the AP's management network), smart-home devices on VLAN 30,
/// isolated guests on VLAN 40, and cameras on VLAN 50 under a 12-character id, the longest a
/// network may have (fw4 names the zone after it).
pub fn home(uplink: Uplink) -> SiteConfig {
    let mut site = defaults(uplink);
    site.site.country = Some("US".to_owned());
    "America/Denver".clone_into(&mut site.site.timezone);
    let lan = &mut site.networks[0];
    lan.ipv6 = Ipv6Mode::Both;
    lan.igmp_snooping = true;
    lan.ipv6_clients = Ipv6Clients::SlaacStateful;
    if let Some(dhcp) = lan.ipv4.as_mut().and_then(|ipv4| ipv4.dhcp.as_mut()) {
        dhcp.reservations.push(Reservation {
            mac: "a4:cf:12:ab:cd:ef".parse().expect("a MAC"),
            ip: Ipv4Addr::new(10, 0, 0, 40),
            name: Some("nas".to_owned()),
        });
        // The pool's options and network boot, which dnsmasq's init script must take.
        dhcp.options = DhcpOptions {
            dns: vec![Ipv4Addr::new(10, 0, 0, 1)],
            ntp: vec![Ipv4Addr::new(10, 0, 0, 1)],
            domain: Some("home.arpa".to_owned()),
            boot: Some(Boot {
                filename: "netboot.xyz.efi".to_owned(),
                server_name: Some("nas".to_owned()),
                server: Some(Ipv4Addr::new(10, 0, 0, 40)),
            }),
            custom: vec!["26,1400".to_owned()],
        };
    }
    let mut iot = network("iot", Purpose::Iot, 30, Ipv6Mode::Delegated);
    iot.ipv6_clients = Ipv6Clients::Slaac;
    site.networks.push(iot);
    let mut guest = network("guest", Purpose::Guest, 40, Ipv6Mode::Off);
    guest.isolation = true;
    site.networks.push(guest);
    site.networks
        .push(network("securitycams", Purpose::Iot, 50, Ipv6Mode::Off));
    site
}

/// The hwsim lab as a site: the gateway is node a, the AP node b. The LAN keeps a's Docker
/// address (`lan`), with neither DHCP nor IPv6, so nothing answers on Docker's network; the lab's
/// clients live on `labnet` (VLAN 99, 10.99.0.0/24 as in the lab's own config), whose WLAN is
/// the one client c joins on both bands. The mesh is the lab's, on channel 36; the WAN, a stub.
/// UPnP is off: the lab's image has no miniupnpd (the system harness checks it).
pub fn lab(lan: Ipv4Net) -> SiteConfig {
    let mut site = defaults(Uplink::Static);
    site.site.country = Some("US".to_owned());
    site.firewall.upnp.enabled = false;
    let lan_net = &mut site.networks[0];
    lan_net.ipv6 = Ipv6Mode::Off;
    lan_net.ipv4 = Some(Ipv4Config {
        address: lan,
        dhcp: None,
    });
    let mut labnet = network("labnet", Purpose::Default, 99, Ipv6Mode::Off);
    if let Some(ipv4) = labnet.ipv4.as_mut() {
        ipv4.address = Ipv4Net::new(Ipv4Addr::new(10, 99, 0, 1), 24).expect("a /24");
        if let Some(dhcp) = ipv4.dhcp.as_mut() {
            dhcp.start = Ipv4Addr::new(10, 99, 0, 100);
            dhcp.end = Ipv4Addr::new(10, 99, 0, 149);
        }
    }
    site.networks.push(labnet);
    site.wlans = vec![Wlan {
        id: "lab".to_owned(),
        ssid: "uniwrt-lab".to_owned(),
        enabled: true,
        network: "labnet".to_owned(),
        security: WlanSecurity::Wpa2Wpa3,
        passphrase: Some(secret(5)),
        bands: vec![Band::G2, Band::G5],
        nodes: NodeScope::All,
        hidden: false,
        isolation: false,
    }];
    site.mesh.enabled = true;
    "uniwrt-lab-mesh".clone_into(&mut site.mesh.mesh_id);
    site.mesh.key = Some(secret(6));
    site.mesh.band = Some(Band::G5);
    for policy in &mut site.radio.bands {
        policy.channel = match policy.band {
            Band::G2 => Channel::Fixed(1),
            Band::G5 => Channel::Fixed(36),
            _ => policy.channel,
        };
    }
    for (node, hostname) in site.nodes.iter_mut().zip(["lab-gw", "lab-ap"]) {
        node.hostname = Some(hostname.to_owned());
    }
    site
}
