//! Networks, from the gateway (features/network.md): each LAN interface on the bridge (DSA,
//! with its VLAN from `bridge-vlan`) or on a swconfig CPU port's VLAN device, with its address,
//! IPv6, DHCP pool and reservations; the ULA; the local DNS records; and an address space
//! that holds them all.

use std::collections::{BTreeMap, BTreeSet};

use uniwrt_core::{Ipv4Net, Ipv6Net};
use uniwrt_model::site::{
    SiteConfig,
    network::{Ipv4Config, Ipv6Clients, Ipv6Mode, Network, Purpose},
};
use uniwrt_uci::{Package, Section};

use crate::{Source, dhcp, read};

/// Ids render keeps for its own interfaces.
const RESERVED: [&str; 6] = ["loopback", "wan", "wan6", "wan_6", "wanb", "bat0"];

pub(crate) fn import(site: &mut SiteConfig, gateway: &Source<'_>, renames: &mut read::Renames) {
    let Some(network) = gateway.packages.get("network") else {
        return;
    };
    let vlans = Vlans::of(network, gateway);
    let mut found: Vec<(String, Network)> = Vec::new();
    for section in network.sections_of("interface") {
        let Some(name) = section.name.as_deref() else {
            continue;
        };
        if RESERVED.contains(&name) || !matches!(section.option("proto"), Some("static" | "none")) {
            continue;
        }
        let Some(vlan) = section.option("device").and_then(|d| vlans.of_device(d)) else {
            continue;
        };
        let taken = |id: &str| found.iter().any(|(_, n)| n.id == id);
        let id = if uniwrt_model::site::validate::is_id(name)
            && !name.starts_with("uw_")
            && !taken(name)
        {
            name.to_owned()
        } else {
            read::id(name, "n", |id| taken(id) || RESERVED.contains(&id))
        };
        let ipv4 = (section.option("proto") == Some("static"))
            .then(|| read::ipv4(section))
            .flatten()
            .map(|address| Ipv4Config {
                address,
                dhcp: dhcp::pool(gateway.packages.get("dhcp"), name, address),
            });
        found.push((
            name.to_owned(),
            Network {
                id,
                name: String::new(),
                purpose: Purpose::Custom,
                vlan,
                ipv4,
                ipv6: ipv6(section),
                ipv6_clients: if ipv6(section).is_on() {
                    dhcp::clients(gateway.packages.get("dhcp"), name)
                } else {
                    Ipv6Clients::default()
                },
                isolation: false,
                mdns: true,
                igmp_snooping: false,
            },
        ));
    }
    if found.is_empty() {
        return;
    }
    let default = found
        .iter()
        .position(|(name, _)| name == "lan")
        .unwrap_or(0);
    found[default].1.purpose = Purpose::Default;
    found[default].1.igmp_snooping = vlans.snooping;
    let mut names = BTreeSet::new();
    for (interface, net) in &mut found {
        if net.purpose != Purpose::Default {
            net.purpose = purpose(interface);
        }
        net.name = display_name(net, &mut names);
        if *interface != net.id {
            renames.interfaces.insert(interface.clone(), net.id.clone());
        }
    }
    site.networks = found.into_iter().map(|(_, n)| n).collect();
    if let Some(ula) = network
        .section("globals")
        .and_then(|g| g.option("ula_prefix"))
        .and_then(|p| p.parse::<Ipv6Net>().ok())
        .filter(|p| p.prefix_len() == 48 && p.addr().segments()[0] & 0xff00 == 0xfd00)
    {
        site.addressing.ula = Some(ula);
    }
    if site.addressing.ula.is_none() {
        for net in &mut site.networks {
            net.ipv6 = match net.ipv6 {
                Ipv6Mode::Both => Ipv6Mode::Delegated,
                Ipv6Mode::Ula => Ipv6Mode::Off,
                mode => mode,
            };
        }
    }
    dhcp::reservations(site, gateway.packages.get("dhcp"));
    site.services.dns_records = dhcp::dns_records(gateway.packages.get("dhcp"));
    address_space(site);
}

/// The gateway's VLANs: which devices carry networks, and whether each is tagged.
struct Vlans {
    bridges: BTreeSet<String>,
    /// Per bridge VLAN: tagged on its ports.
    tagged: BTreeMap<u16, bool>,
    /// A swconfig CPU port's device, with its VLANs and whether the user ports tag them.
    switch: Option<(String, BTreeMap<u16, bool>)>,
    snooping: bool,
}

impl Vlans {
    fn of(network: &Package, gateway: &Source<'_>) -> Self {
        let mut bridges = BTreeSet::new();
        let mut snooping = false;
        for device in network.sections_of("device") {
            if device.option("type") == Some("bridge")
                && let Some(name) = device.option("name")
            {
                bridges.insert(name.to_owned());
                snooping |= read::flag(device, "igmp_snooping").unwrap_or(false);
            }
        }
        let switch = gateway
            .facts
            .board
            .layout
            .switch
            .as_ref()
            .and_then(|layout| {
                let cpu = layout.lan_cpu.as_ref()?;
                let vlans: BTreeMap<u16, bool> = network
                    .sections_of("switch_vlan")
                    .filter(|v| v.option("device") == Some(layout.name.as_str()))
                    .filter_map(|v| {
                        let vid = v.option("vid").or(v.option("vlan"))?.parse().ok()?;
                        let ports = read::items(v, "ports");
                        if !ports
                            .iter()
                            .any(|p| p.trim_end_matches('t') == cpu.num.to_string())
                        {
                            return None;
                        }
                        let user: Vec<&&str> = ports
                            .iter()
                            .filter(|p| {
                                layout
                                    .lan_ports
                                    .iter()
                                    .any(|n| p.trim_end_matches('t') == n.to_string())
                            })
                            .collect();
                        Some((vid, user.iter().any(|p| p.ends_with('t'))))
                    })
                    .collect();
                Some((cpu.device.clone(), one_untagged(vlans)))
            });
        // A swconfig CPU port's VLAN devices are the bridge's access ports: the switch says
        // whether a VLAN is tagged; on the other ports (Ethernet, bat0) a VLAN is tagged when
        // any of them tags it.
        let tagged = network
            .sections_of("bridge-vlan")
            .filter(|v| v.option("device").is_some_and(|d| bridges.contains(d)))
            .filter_map(|v| {
                let vid: u16 = v.option("vlan")?.parse().ok()?;
                let trunks: Vec<&str> = read::items(v, "ports")
                    .into_iter()
                    .filter(|p| {
                        let device = p.split(':').next().unwrap_or_default();
                        !switch.as_ref().is_some_and(|(cpu, _)| {
                            device
                                .strip_prefix(cpu.as_str())
                                .is_some_and(|r| r.starts_with('.'))
                        })
                    })
                    .collect();
                let tagged = if trunks.is_empty() {
                    switch
                        .as_ref()
                        .and_then(|(_, vlans)| vlans.get(&vid).copied())
                        .unwrap_or(true)
                } else {
                    trunks.iter().any(|p| p.ends_with(":t"))
                };
                Some((vid, tagged))
            })
            .collect();
        let tagged = one_untagged(tagged);
        Self {
            bridges,
            tagged,
            switch,
            snooping,
        }
    }

    /// The VLAN of a network on `device` (`None` untagged), if the device is a LAN one.
    #[expect(
        clippy::option_option,
        reason = "not a LAN device, or a LAN one's VLAN"
    )]
    fn of_device(&self, device: &str) -> Option<Option<u16>> {
        if self.bridges.contains(device) {
            return Some(None);
        }
        let (parent, vid) = device.rsplit_once('.')?;
        let vid: u16 = vid.parse().ok().filter(|v| (1..4095).contains(v))?;
        if self.bridges.contains(parent) {
            let tagged = self.tagged.get(&vid).copied().unwrap_or(true);
            return Some(tagged.then_some(vid));
        }
        let (cpu, vlans) = self.switch.as_ref()?;
        (parent == cpu).then(|| {
            let tagged = vlans.get(&vid).copied().unwrap_or(false);
            tagged.then_some(vid)
        })
    }
}

/// Marks every VLAN tagged but the lowest one no port tags. A port profile may carry a tagged
/// network untagged, but none tags the untagged network; a network no port tags at all is told
/// apart by render's choice of the lowest free VLAN for the untagged one.
fn one_untagged(mut vlans: BTreeMap<u16, bool>) -> BTreeMap<u16, bool> {
    let mut first = true;
    for tagged in vlans.values_mut() {
        if !*tagged {
            *tagged = !first;
            first = false;
        }
    }
    vlans
}

/// A network's purpose from its interface's name; the default one is chosen apart.
fn purpose(interface: &str) -> Purpose {
    let name = interface.to_ascii_lowercase();
    if name.contains("guest") {
        Purpose::Guest
    } else if name.contains("iot") || name.contains("smart") {
        Purpose::Iot
    } else if name.contains("mgmt") || name.contains("manage") {
        Purpose::Mgmt
    } else {
        Purpose::Custom
    }
}

fn display_name(net: &Network, names: &mut BTreeSet<String>) -> String {
    let base = match net.purpose {
        Purpose::Default => "Default".to_owned(),
        Purpose::Guest => "Guest".to_owned(),
        Purpose::Iot => "IoT".to_owned(),
        Purpose::Mgmt => "Management".to_owned(),
        Purpose::Custom => {
            let mut chars = net.id.chars();
            chars.next().map_or_else(String::new, |c| {
                c.to_ascii_uppercase().to_string() + chars.as_str()
            })
        }
    };
    let name = (1..1000)
        .map(|n| {
            if n == 1 {
                base.clone()
            } else {
                format!("{base} {n}")
            }
        })
        .find(|name| !names.contains(name))
        .unwrap_or(base);
    names.insert(name.clone());
    name
}

fn ipv6(interface: &Section) -> Ipv6Mode {
    if interface.option("ip6assign").is_none() {
        return Ipv6Mode::Off;
    }
    let classes = read::items(interface, "ip6class");
    let local = classes.contains(&"local");
    let delegated = classes.iter().any(|c| *c != "local");
    match (local, delegated) {
        (true, false) => Ipv6Mode::Ula,
        (false, true) => Ipv6Mode::Delegated,
        _ => Ipv6Mode::Both,
    }
}

/// Widens the address space to hold every network: their common prefix, at least a /16, if
/// that is private.
fn address_space(site: &mut SiteConfig) {
    let subnets: Vec<Ipv4Net> = site
        .networks
        .iter()
        .filter_map(|n| n.ipv4.as_ref())
        .map(|v4| v4.address.trunc())
        .collect();
    if subnets.iter().all(|s| site.addressing.ipv4.contains(s)) {
        return;
    }
    let Some(first) = subnets.first() else { return };
    let mut len = first.prefix_len().min(16);
    let space = loop {
        let Ok(candidate) = Ipv4Net::new(first.addr(), len) else {
            return;
        };
        let candidate = candidate.trunc();
        if subnets.iter().all(|s| candidate.contains(s)) {
            break candidate;
        }
        if len == 8 {
            return;
        }
        len -= 1;
    };
    if space.addr().is_private() {
        site.addressing.ipv4 = space;
    }
}
