//! Inputs for the golden tests: the canonical gateway's facts and variants of it for each
//! capability set, a home site, its secrets, and OpenWrt's stock packages as a node's current
//! ones.

use std::{collections::BTreeMap, fmt::Write as _, net::Ipv4Addr};

use uniwrt_core::{Ipv4Net, NodeId};
use uniwrt_model::{
    facts::{
        Channel, CpuPort, NodeFacts, PhyBand, PhyFacts, RadioFacts, Software, SwitchKind,
        SwitchLayout, WifiFeature,
    },
    node::NodeRole,
    site::{
        SecretRef, SiteConfig,
        gateway::{DnsRecord, WanProto, ZoneAction, ZonePolicy},
        network::{Dhcp, Ipv4Config, Ipv6Mode, Network, Purpose, Reservation},
        node::{ManagedPackage, NodeConfig, RadioOverride},
        wireless::{NodeScope, TxPower, Wlan, WlanSecurity},
    },
    wifi::{Band, ChannelWidth, WifiStandard},
};
use uniwrt_render::{Issue, Rendered, Target, render};
use uniwrt_uci::Package;

pub(crate) const GATEWAY: NodeId = NodeId::new(1);
pub(crate) const AP: NodeId = NodeId::new(2);

pub(crate) fn secret(n: u8) -> SecretRef {
    SecretRef::new(format!("config/{n:032x}")).unwrap()
}

pub(crate) fn secrets() -> BTreeMap<SecretRef, String> {
    BTreeMap::from([
        (secret(1), "correct horse battery".to_owned()),
        (secret(2), "things-only-2024".to_owned()),
        (secret(3), "mesh key 0123456789".to_owned()),
        (secret(4), "pppoe-pass".to_owned()),
    ])
}

/// The canonical gateway: a GL-MT6000 (DSA, full wpad, batman-adv) on 25.12.
pub(crate) fn gateway_facts() -> NodeFacts {
    serde_json::from_str(include_str!("../../../model/fixtures/facts/gateway.json")).unwrap()
}

fn channels(numbers: &[u16], band: Band) -> Vec<Channel> {
    numbers
        .iter()
        .map(|&number| Channel {
            number,
            frequency_mhz: band.frequency(number),
            dfs: (52..=144).contains(&number),
            dfs_state: None,
            no_ir: false,
            max_tx_power_dbm: Some(20),
        })
        .collect()
}

fn radio(name: &str, phy: &str, band: Band, path: &str) -> RadioFacts {
    RadioFacts {
        name: name.to_owned(),
        phy: Some(phy.to_owned()),
        path: Some(path.to_owned()),
        band,
        up: true,
        disabled: false,
        channel: Some(if band == Band::G2 { 6 } else { 36 }),
        frequency_mhz: None,
        center_frequency_mhz: None,
        width: None,
        standard: None,
        tx_power_dbm: None,
        noise_dbm: None,
        channel_busy_percent: None,
        country: None,
        ifaces: Vec::new(),
    }
}

fn phy(
    name: &str,
    band: Band,
    standards: &[WifiStandard],
    widths: &[ChannelWidth],
    numbers: &[u16],
) -> PhyFacts {
    PhyFacts {
        name: name.to_owned(),
        bands: vec![PhyBand {
            band,
            standards: standards.iter().copied().collect(),
            widths: widths.iter().copied().collect(),
            spatial_streams: Some(2),
            channels: channels(numbers, band),
        }],
    }
}

/// A tri-band AP: the gateway's hardware plus a second 5 GHz radio on the high band.
pub(crate) fn triband_facts() -> NodeFacts {
    let mut facts = gateway_facts();
    facts.radios.push(radio(
        "radio2",
        "phy2",
        Band::G5,
        "platform/soc/18000000.wifi+2",
    ));
    facts.phys.push(phy(
        "phy2",
        Band::G5,
        &[WifiStandard::Ht, WifiStandard::Vht, WifiStandard::He],
        &[
            ChannelWidth::Mhz20,
            ChannelWidth::Mhz40,
            ChannelWidth::Mhz80,
        ],
        &[100, 149, 165],
    ));
    facts
}

/// An older ath79 board: a swconfig switch with the WAN on port 1 behind the same CPU port,
/// wpad-basic (SAE, OWE and 11r/w, no mesh or ACS), no batman-adv, and 802.11n/ac radios.
pub(crate) fn swconfig_facts() -> NodeFacts {
    let mut facts = gateway_facts();
    let layout = &mut facts.board.layout;
    layout.lan = vec!["eth0.1".to_owned()];
    layout.wan = Some("eth0.2".to_owned());
    layout.wan_mac = "50:c7:bf:00:00:02".parse().ok();
    layout.compat_version = None;
    let cpu = CpuPort {
        num: 0,
        device: "eth0".to_owned(),
    };
    layout.switch = Some(SwitchLayout {
        name: "switch0".to_owned(),
        lan_ports: vec![2, 3, 4, 5],
        wan_ports: vec![1],
        lan_cpu: Some(cpu.clone()),
        wan_cpu: Some(cpu),
    });
    let caps = &mut facts.capabilities;
    caps.switch = Some(SwitchKind::Swconfig);
    caps.wifi = [
        WifiFeature::Sae,
        WifiFeature::Owe,
        WifiFeature::FastTransition,
        WifiFeature::ProtectedManagementFrames,
    ]
    .into();
    caps.software = [Software::Iw].into();
    facts.radios = vec![
        radio("radio0", "phy0", Band::G5, "pci0000:00/0000:00:00.0"),
        radio("radio1", "phy1", Band::G2, "platform/ahb/18100000.wmac"),
    ];
    facts.phys = vec![
        phy(
            "phy0",
            Band::G5,
            &[WifiStandard::Legacy, WifiStandard::Ht, WifiStandard::Vht],
            &[
                ChannelWidth::Mhz20,
                ChannelWidth::Mhz40,
                ChannelWidth::Mhz80,
            ],
            &[36, 40, 44, 48, 149],
        ),
        phy(
            "phy1",
            Band::G2,
            &[WifiStandard::Legacy, WifiStandard::Ht],
            &[ChannelWidth::Mhz20, ChannelWidth::Mhz40],
            &[1, 6, 11],
        ),
    ];
    facts.stations.clear();
    facts
}

/// A PC router: two plain NICs, no switch and no radios.
pub(crate) fn x86_facts() -> NodeFacts {
    let mut facts = gateway_facts();
    facts.board.layout.lan = vec!["eth0".to_owned()];
    facts.board.layout.wan = Some("eth1".to_owned());
    facts.capabilities.switch = None;
    facts.capabilities.wifi.clear();
    facts.capabilities.software.clear();
    facts.radios.clear();
    facts.phys.clear();
    facts
}

pub(crate) fn managed(node: NodeId) -> NodeConfig {
    let mut config = NodeConfig::new(node);
    config.managed = [
        ManagedPackage::System,
        ManagedPackage::Network,
        ManagedPackage::Wireless,
        ManagedPackage::Dhcp,
        ManagedPackage::Firewall,
    ]
    .into();
    config
}

fn ula() -> Option<uniwrt_core::Ipv6Net> {
    "fd12:3456:789a::/48".parse().ok()
}

/// A new site's defaults, with both nodes managed.
pub(crate) fn defaults_site() -> SiteConfig {
    let mut site = SiteConfig::defaults(ula());
    site.nodes = vec![managed(GATEWAY), managed(AP)];
    site
}

fn net(id: &str, name: &str, purpose: Purpose, vlan: u16, ipv6: Ipv6Mode) -> Network {
    Network {
        id: id.to_owned(),
        name: name.to_owned(),
        purpose,
        vlan: Some(vlan),
        ipv4: Some(Ipv4Config {
            address: Ipv4Net::new(Ipv4Addr::new(10, 0, u8::try_from(vlan).unwrap(), 1), 24)
                .unwrap(),
            dhcp: Some(Dhcp {
                start: Ipv4Addr::new(10, 0, u8::try_from(vlan).unwrap(), 100),
                end: Ipv4Addr::new(10, 0, u8::try_from(vlan).unwrap(), 249),
                lease_time_s: 3600,
                reservations: Vec::new(),
            }),
        }),
        ipv6,
        isolation: false,
        mdns: false,
        igmp_snooping: false,
    }
}

fn wlan(
    id: &str,
    ssid: &str,
    network: &str,
    security: WlanSecurity,
    passphrase: Option<u8>,
    bands: &[Band],
) -> Wlan {
    Wlan {
        id: id.to_owned(),
        ssid: ssid.to_owned(),
        enabled: true,
        network: network.to_owned(),
        security,
        passphrase: passphrase.map(secret),
        bands: bands.to_vec(),
        nodes: NodeScope::All,
        hidden: false,
        isolation: false,
    }
}

/// The owner's kind of home: a LAN, smart-home devices on VLAN 30 and guests on VLAN 40, three WLANs, the
/// mesh on 5 GHz, and the WAN on the ISP's VLAN 201.
pub(crate) fn home_site() -> SiteConfig {
    let mut site = defaults_site();
    site.site.country = Some("US".to_owned());
    "America/Denver".clone_into(&mut site.site.timezone);
    let lan = &mut site.networks[0];
    lan.ipv6 = Ipv6Mode::Both;
    lan.igmp_snooping = true;
    if let Some(dhcp) = lan.ipv4.as_mut().and_then(|ipv4| ipv4.dhcp.as_mut()) {
        dhcp.reservations.push(Reservation {
            mac: "a4:cf:12:ab:cd:ef".parse().unwrap(),
            ip: Ipv4Addr::new(10, 0, 0, 40),
            name: Some("nas".to_owned()),
        });
    }
    site.networks
        .push(net("iot", "IoT", Purpose::Iot, 30, Ipv6Mode::Delegated));
    let mut guest = net("guest", "Guest", Purpose::Guest, 40, Ipv6Mode::Off);
    guest.isolation = true;
    site.networks.push(guest);
    site.wlans = vec![
        wlan(
            "home",
            "Home",
            "lan",
            WlanSecurity::Wpa2Wpa3,
            Some(1),
            &[Band::G2, Band::G5],
        ),
        wlan(
            "things",
            "Things",
            "iot",
            WlanSecurity::Wpa2,
            Some(2),
            &[Band::G2],
        ),
        wlan(
            "guest",
            "Guest",
            "guest",
            WlanSecurity::Owe,
            None,
            &[Band::G2, Band::G5],
        ),
    ];
    site.mesh.enabled = true;
    "uw-home".clone_into(&mut site.mesh.mesh_id);
    site.mesh.key = Some(secret(3));
    site.wan.vlan = Some(201);
    site.services.dns_records = vec![
        DnsRecord::Host {
            name: "nas.lan".to_owned(),
            ip: "10.0.0.40".parse().unwrap(),
        },
        DnsRecord::Cname {
            name: "media.lan".to_owned(),
            target: "nas.lan".to_owned(),
        },
    ];
    site.firewall.policies = vec![ZonePolicy {
        from: "lan".to_owned(),
        to: "guest".to_owned(),
        action: ZoneAction::Block,
    }];
    let ap = site.nodes.iter_mut().find(|n| n.node == AP).unwrap();
    ap.hostname = Some("ap-garden".to_owned());
    ap.radios.push(RadioOverride {
        radio: "radio0".to_owned(),
        disabled: false,
        channel: None,
        width: None,
        tx_power: Some(TxPower::Dbm(14)),
    });
    site
}

/// The home site behind PPPoE, without the mesh.
pub(crate) fn pppoe_site() -> SiteConfig {
    let mut site = home_site();
    site.mesh.enabled = false;
    site.wan.vlan = None;
    site.wan.mac = "50:c7:bf:aa:bb:cc".parse().ok();
    site.wan.proto = WanProto::Pppoe {
        username: "user@isp".to_owned(),
        password: Some(secret(4)),
    };
    site
}

/// OpenWrt 25.12's stock packages, as a node holds them before uniwrt manages it.
pub(crate) fn stock() -> BTreeMap<String, Package> {
    [
        (
            "network",
            include_str!("../../../uci/fixtures/openwrt/25.12/network"),
        ),
        (
            "system",
            include_str!("../../../uci/fixtures/openwrt/25.12/system"),
        ),
        (
            "dhcp",
            include_str!("../../../uci/fixtures/openwrt/25.12/dhcp"),
        ),
        (
            "firewall",
            include_str!("../../../uci/fixtures/openwrt/25.12/firewall"),
        ),
    ]
    .into_iter()
    .map(|(name, text)| (name.to_owned(), uniwrt_uci::parse(name, text).unwrap()))
    .collect()
}

pub(crate) fn run(
    site: &SiteConfig,
    node: NodeId,
    role: NodeRole,
    facts: &NodeFacts,
) -> Result<Rendered, Vec<Issue>> {
    let current = stock();
    render(
        site,
        &secrets(),
        Target {
            node,
            role,
            facts,
            current: &current,
        },
    )
}

/// The golden text: services and warnings, then each package as `uci commit` writes it.
pub(crate) fn show(result: &Result<Rendered, Vec<Issue>>) -> String {
    let mut out = String::new();
    match result {
        Err(issues) => {
            out.push_str("errors:\n");
            for issue in issues {
                let _ = writeln!(out, "  {}: {}", issue.path, issue.message);
            }
        }
        Ok(rendered) => {
            let services: Vec<String> = rendered
                .services
                .iter()
                .map(|(name, on)| format!("{name} {}", if *on { "on" } else { "off" }))
                .collect();
            let _ = writeln!(out, "services: {}", services.join(", "));
            for issue in &rendered.warnings {
                let _ = writeln!(out, "warning: {}: {}", issue.path, issue.message);
            }
            for package in rendered.packages.values() {
                let _ = write!(out, "\n# {}\n{}", package.name, package.to_file());
            }
        }
    }
    out
}
