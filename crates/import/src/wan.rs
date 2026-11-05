//! The WAN, from the gateway (features/wan.md): `interface wan` by DHCP, static or PPPoE, the
//! ISP's VLAN (an 802.1Q device, `<port>.<vid>`, or a tagged switch VLAN), a cloned MAC, and
//! IPv6 (`wan6` by DHCPv6, or PPPoE's `ipv6`).

use std::net::IpAddr;

use uniwrt_model::site::{
    SecretRef, SiteConfig,
    gateway::{WanConfig, WanProto},
};
use uniwrt_uci::Section;

use crate::{Source, read};

pub(crate) fn import(
    site: &mut SiteConfig,
    gateway: &Source<'_>,
    renames: &mut read::Renames,
    seal: &mut dyn FnMut(&str) -> SecretRef,
) {
    let Some(network) = gateway.packages.get("network") else {
        return;
    };
    let Some(wan) = network.section("wan").filter(|s| s.kind == "interface") else {
        return;
    };
    let Some(device) = wan.option("device") else {
        renames
            .whole
            .insert(("network".to_owned(), "wan".to_owned()));
        return;
    };
    let proto = match wan.option("proto") {
        Some("dhcp") => Some(WanProto::Dhcp),
        Some("static") => static_proto(wan),
        Some("pppoe") => wan.option("username").map(|username| WanProto::Pppoe {
            username: username.to_owned(),
            password: wan.option("password").map(&mut *seal),
        }),
        _ => None,
    };
    let Some(proto) = proto else {
        renames
            .whole
            .insert(("network".to_owned(), "wan".to_owned()));
        return;
    };
    let layout = &gateway.facts.board.layout;
    let declared = network
        .sections_of("device")
        .find(|d| d.option("name") == Some(device));
    let vlan = declared
        .filter(|d| d.option("type") == Some("8021q"))
        .and_then(|d| d.option("vid"))
        .or_else(|| {
            let (parent, vid) = device.rsplit_once('.')?;
            let port = layout.wan.as_deref()?;
            (parent == port).then_some(vid)
        })
        .or_else(|| switch_vlan(network, gateway, device))
        .and_then(|vid| vid.parse().ok());
    let mac = declared
        .and_then(|d| d.option("macaddr"))
        .and_then(|m| m.parse().ok())
        .filter(|mac| Some(*mac) != layout.wan_mac);
    let ipv6 = if matches!(proto, WanProto::Pppoe { .. }) {
        wan.option("ipv6").is_some_and(|v| v != "0")
    } else {
        network.sections_of("interface").any(|s| {
            s.option("proto") == Some("dhcpv6")
                && matches!(s.option("device"), Some(d) if d == device || d == "@wan")
        })
    };
    site.wan = WanConfig {
        proto,
        vlan,
        mac,
        ipv6,
    };
}

fn static_proto(wan: &Section) -> Option<WanProto> {
    Some(WanProto::Static {
        address: read::ipv4(wan)?,
        gateway: wan.option("gateway")?.parse().ok()?,
        dns: read::items(wan, "dns")
            .into_iter()
            .filter_map(|d| d.parse::<IpAddr>().ok())
            .collect(),
    })
}

/// The ISP's VLAN on a swconfig switch: the WAN CPU port's `<cpu>.<vid>`, when the WAN's user
/// ports tag it.
fn switch_vlan<'p>(
    network: &'p uniwrt_uci::Package,
    gateway: &Source<'_>,
    device: &'p str,
) -> Option<&'p str> {
    let switch = gateway.facts.board.layout.switch.as_ref()?;
    let cpu = switch.wan_cpu.as_ref()?;
    let (parent, vid) = device.rsplit_once('.')?;
    if parent != cpu.device {
        return None;
    }
    let tagged = network.sections_of("switch_vlan").any(|v| {
        v.option("device") == Some(switch.name.as_str())
            && v.option("vid").or(v.option("vlan")) == Some(vid)
            && read::items(v, "ports").iter().any(|p| {
                p.ends_with('t')
                    && switch
                        .wan_ports
                        .iter()
                        .any(|n| p.trim_end_matches('t') == n.to_string())
            })
    });
    tagged.then_some(vid)
}
