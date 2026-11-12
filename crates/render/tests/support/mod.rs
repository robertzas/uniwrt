//! Inputs for the golden tests: the canonical gateway's facts and variants of it for each
//! capability set, a home site, its secrets, and OpenWrt's stock packages as a node's current
//! ones.

#![allow(dead_code)] // each test binary uses part of it

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
        firewall::{ForwardProto, PortForward, PortRange, ZoneAction, ZonePolicy},
        gateway::{DnsRecord, WanProto},
        network::{
            Boot, Dhcp, DhcpOptions, Ipv4Config, Ipv6Clients, Ipv6Mode, Network, Purpose,
            Reservation,
        },
        node::{ManagedPackage, NodeConfig, PortConfig, RadioOverride, TaggedNetworks},
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

/// The vault's values, and an 802.11r key for every WLAN: its id, repeated to 64 characters.
pub(crate) struct TestSecrets {
    values: BTreeMap<SecretRef, String>,
    ft_keys: BTreeMap<String, String>,
}

impl uniwrt_render::Secrets for TestSecrets {
    fn get(&self, secret: &SecretRef) -> Option<&str> {
        self.values.get(secret).map(String::as_str)
    }

    fn ft_key(&self, wlan: &str) -> Option<&str> {
        self.ft_keys.get(wlan).map(String::as_str)
    }
}

pub(crate) fn secrets() -> TestSecrets {
    TestSecrets {
        values: BTreeMap::from([
            (secret(1), "correct horse battery".to_owned()),
            (secret(2), "things-only-2024".to_owned()),
            (secret(3), "mesh key 0123456789".to_owned()),
            (secret(4), "pppoe-pass".to_owned()),
        ]),
        ft_keys: ["home", "things", "guest"]
            .into_iter()
            .map(|id| (id.to_owned(), id.repeat(64).chars().take(64).collect()))
            .collect(),
    }
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
/// wpad-basic (SAE, OWE and 11r/k/w, no mesh, ACS or 11v), no batman-adv, and 802.11n/ac
/// radios.
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
        WifiFeature::RadioMeasurement,
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
        ManagedPackage::Usteer,
        ManagedPackage::Sqm,
        ManagedPackage::Mwan3,
        ManagedPackage::Upnpd,
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
        max_clients: None,
        schedule: None,
        roaming: uniwrt_model::site::wireless::Roaming::default(),
    }
}

/// The home's port forwards: a NAS from one network only, a game server's range and a disabled
/// camera.
fn home_forwards() -> Vec<PortForward> {
    vec![
        PortForward {
            name: "NAS".to_owned(),
            enabled: true,
            proto: ForwardProto::Tcp,
            ports: PortRange::one(8443),
            to: Ipv4Addr::new(10, 0, 0, 40),
            to_port: Some(443),
            source: Some("203.0.113.0/24".parse().unwrap()),
            reflection: true,
        },
        PortForward {
            name: "Game server".to_owned(),
            enabled: true,
            proto: ForwardProto::TcpUdp,
            ports: PortRange {
                first: 27015,
                last: 27030,
            },
            to: Ipv4Addr::new(10, 0, 30, 20),
            to_port: None,
            source: None,
            reflection: false,
        },
        PortForward {
            name: "Old camera".to_owned(),
            enabled: false,
            proto: ForwardProto::Udp,
            ports: PortRange::one(554),
            to: Ipv4Addr::new(10, 0, 30, 21),
            to_port: None,
            source: None,
            reflection: true,
        },
    ]
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
    // 802.11r with SAE (keys from the controller) on home, as WPA2 suggests on things (keys
    // generated locally); no 11k/v hints on guest.
    site.wlans[0].roaming.fast = Some(true);
    site.wlans[2].roaming.hints = false;
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
    site.firewall.forwards = home_forwards();
    site.firewall.dmz = Some(Ipv4Addr::new(10, 0, 0, 50));
    site.firewall.upnp.networks = vec!["lan".to_owned(), "iot".to_owned()];
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
/// The home site with the LAN's services and port profiles (M4.2a): DHCP options and network
/// boot, two IPv6 clients modes, and on each node a port that carries one network untagged, one
/// that carries only a tagged one, and one that carries nothing (by name on DSA boards, by
/// number on a swconfig switch).
pub(crate) fn lan_site(swconfig: bool) -> SiteConfig {
    let mut site = home_site();
    let lan = &mut site.networks[0];
    lan.ipv6_clients = Ipv6Clients::SlaacStateful;
    if let Some(dhcp) = lan.ipv4.as_mut().and_then(|ipv4| ipv4.dhcp.as_mut()) {
        dhcp.options = DhcpOptions {
            dns: vec![Ipv4Addr::new(10, 0, 0, 53)],
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
    site.networks[1].ipv6_clients = Ipv6Clients::Slaac;
    let [untagged, tagged, none] = if swconfig {
        ["switch0:3", "switch0:4", "switch0:5"]
    } else {
        ["lan3", "lan4", "lan5"]
    };
    let profile = |port: &str, native: Option<&str>, tagged: TaggedNetworks| PortConfig {
        port: port.to_owned(),
        native: native.map(str::to_owned),
        tagged,
    };
    for config in &mut site.nodes {
        config.ports = vec![
            profile(untagged, Some("iot"), TaggedNetworks::None),
            profile(
                tagged,
                None,
                TaggedNetworks::Only {
                    networks: vec!["guest".to_owned()],
                },
            ),
            profile(none, None, TaggedNetworks::None),
        ];
    }
    site
}

pub(crate) fn pppoe_site() -> SiteConfig {
    let mut site = home_site();
    site.mesh.enabled = false;
    site.wan.vlan = None;
    site.wan.mac = "50:c7:bf:aa:bb:cc".parse().ok();
    site.wan.proto = WanProto::Pppoe {
        username: "user@isp".to_owned(),
        password: Some(secret(4)),
        service: None,
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
    run_on(site, node, role, facts, &stock())
}

/// [`run`] on a node that holds `current`.
pub(crate) fn run_on(
    site: &SiteConfig,
    node: NodeId,
    role: NodeRole,
    facts: &NodeFacts,
    current: &BTreeMap<String, Package>,
) -> Result<Rendered, Vec<Issue>> {
    render(
        site,
        &secrets(),
        Target {
            node,
            role,
            facts,
            current,
            controller: None,
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
            if !rendered.requires.is_empty() {
                let requires: Vec<&str> = rendered.requires.iter().map(|r| r.package).collect();
                let _ = writeln!(out, "requires: {}", requires.join(", "));
            }
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
