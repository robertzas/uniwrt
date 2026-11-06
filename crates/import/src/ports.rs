//! Port profiles, from every node (features/network.md, Networks): which networks each Ethernet
//! port carries, read from the VLAN memberships of `br-lan` (`bridge-vlan`) or of a swconfig
//! switch (`switch_vlan`). A port that carries what a trunk carries gets no profile; a LAN port
//! that is in neither the bridge nor anything else carries nothing.

use std::collections::{BTreeMap, BTreeSet};

use uniwrt_model::{
    facts::SwitchLayout,
    node::NodeRole,
    site::{
        SiteConfig,
        node::{PortConfig, TaggedNetworks},
    },
};
use uniwrt_uci::{Package, Section};

use crate::{Source, read};

/// The bridge render puts every network in.
const BRIDGE: &str = "br-lan";

/// What a port carries: its untagged VLAN and its tagged ones.
type Carried = (Option<u16>, BTreeSet<u16>);

pub(crate) fn import(site: &SiteConfig, source: &Source<'_>) -> Vec<PortConfig> {
    let Some(network) = source.packages.get("network") else {
        return Vec::new();
    };
    let carried = match &source.facts.board.layout.switch {
        Some(switch) if switch.lan_cpu.is_some() => switch_ports(network, source.role, switch),
        _ => bridge_ports(network, source),
    };
    let Some((carried, listed)) = carried else {
        return Vec::new();
    };
    // Render gives the untagged network the lowest VLAN no network has.
    let untagged_vid = listed
        .iter()
        .copied()
        .filter(|vid| !site.networks.iter().any(|n| n.vlan == Some(*vid)))
        .min();
    let net = |vid: u16| {
        site.networks
            .iter()
            .find(|n| n.vlan.map_or(Some(vid) == untagged_vid, |v| v == vid))
            .map(|n| n.id.clone())
    };
    let untagged = site.networks.iter().find(|n| n.vlan.is_none());
    let all: Vec<&str> = site
        .networks
        .iter()
        .filter(|n| n.vlan.is_some())
        .map(|n| n.id.as_str())
        .collect();
    carried
        .into_iter()
        .filter_map(|(port, (native, tagged))| {
            let native = native.and_then(net);
            let tagged: Vec<String> = site
                .networks
                .iter()
                .filter(|n| n.vlan.is_some_and(|v| tagged.contains(&v)))
                .filter(|n| Some(&n.id) != native.as_ref())
                .map(|n| n.id.clone())
                .collect();
            let others: Vec<&str> = all
                .iter()
                .copied()
                .filter(|id| Some(*id) != native.as_deref())
                .collect();
            let tagged = if tagged.is_empty() && !others.is_empty() {
                TaggedNetworks::None
            } else if tagged == others {
                TaggedNetworks::All
            } else {
                TaggedNetworks::Only { networks: tagged }
            };
            let trunk = native.as_deref() == untagged.map(|n| n.id.as_str())
                && tagged == TaggedNetworks::All;
            (!trunk).then_some(PortConfig {
                port,
                native,
                tagged,
            })
        })
        .collect()
}

/// What each Ethernet port of `br-lan` carries, and the VLANs the bridge lists (a plain
/// bridge: VLAN 0, for the untagged network).
fn bridge_ports(
    network: &Package,
    source: &Source<'_>,
) -> Option<(BTreeMap<String, Carried>, Vec<u16>)> {
    let bridge = network
        .sections_of("device")
        .find(|d| d.option("type") == Some("bridge") && d.option("name") == Some(BRIDGE))?;
    let members = read::items(bridge, "ports");
    let layout = &source.facts.board.layout;
    let mut ethernet: Vec<&str> = layout.lan.iter().map(String::as_str).collect();
    if source.role != NodeRole::Gateway {
        ethernet.extend(layout.wan.as_deref());
    }
    let vlans: Vec<&Section> = network
        .sections_of("bridge-vlan")
        .filter(|v| v.option("device") == Some(BRIDGE))
        .collect();
    let mut carried = BTreeMap::new();
    for port in ethernet {
        if !members.contains(&port) {
            if !used_elsewhere(network, port) {
                carried.insert(port.to_owned(), (None, BTreeSet::new()));
            }
            continue;
        }
        let mut native = None;
        let mut tagged = BTreeSet::new();
        if vlans.is_empty() {
            // A plain bridge: the untagged network, VLAN 0 here.
            native = Some(0);
        }
        for vlan in &vlans {
            let Some(vid) = vlan.option("vlan").and_then(|v| v.parse::<u16>().ok()) else {
                continue;
            };
            for entry in read::items(vlan, "ports") {
                let (device, flags) = entry.split_once(':').unwrap_or((entry, ""));
                if device != port {
                    continue;
                }
                if flags.contains('t') {
                    tagged.insert(vid);
                } else if native.is_none() || flags.contains('*') {
                    native = Some(vid);
                }
            }
        }
        carried.insert(port.to_owned(), (native, tagged));
    }
    let listed = if vlans.is_empty() {
        vec![0]
    } else {
        vlans
            .iter()
            .filter_map(|v| v.option("vlan")?.parse().ok())
            .collect()
    };
    Some((carried, listed))
}

/// What each user port of the swconfig switch carries, and the LAN's VLANs on it.
fn switch_ports(
    network: &Package,
    role: NodeRole,
    switch: &SwitchLayout,
) -> Option<(BTreeMap<String, Carried>, Vec<u16>)> {
    let mut user = switch.lan_ports.clone();
    if role != NodeRole::Gateway {
        user.extend(&switch.wan_ports);
    }
    let lan_cpu = switch.lan_cpu.as_ref()?.num.to_string();
    let vlans: Vec<(u16, Vec<&str>)> = network
        .sections_of("switch_vlan")
        .filter(|v| v.option("device") == Some(switch.name.as_str()))
        .filter_map(|v| {
            let vid = v.option("vid").or(v.option("vlan"))?.parse().ok()?;
            let members = read::items(v, "ports");
            let lan = members.iter().any(|m| m.trim_end_matches('t') == lan_cpu);
            // The gateway's WAN VLAN holds its WAN ports.
            let wan = role == NodeRole::Gateway
                && members.iter().any(|m| {
                    switch
                        .wan_ports
                        .iter()
                        .any(|p| m.trim_end_matches('t') == p.to_string())
                });
            (lan && !wan).then_some((vid, members))
        })
        .collect();
    if vlans.is_empty() {
        return None;
    }
    let carried = user
        .iter()
        .map(|n| {
            let (mut native, mut tagged) = (None, BTreeSet::new());
            for (vid, members) in &vlans {
                if members.contains(&format!("{n}t").as_str()) {
                    tagged.insert(*vid);
                } else if members.contains(&n.to_string().as_str()) {
                    native = Some(*vid);
                }
            }
            (PortConfig::switch_port(&switch.name, *n), (native, tagged))
        })
        .collect();
    Some((carried, vlans.iter().map(|(vid, _)| *vid).collect()))
}

/// Whether the port serves something other than the bridge: an interface's device, another
/// bridge's port, or an 802.1Q device's parent.
fn used_elsewhere(network: &Package, port: &str) -> bool {
    network.sections.iter().any(|s| match s.kind.as_str() {
        "interface" => s.option("device") == Some(port),
        "device" => {
            s.option("name") != Some(BRIDGE)
                && (read::items(s, "ports").contains(&port) || s.option("ifname") == Some(port))
        }
        _ => false,
    })
}
