//! The gateway's uplink in the `network` package (features/wan.md, Rendering on the gateway):
//! the WAN device (an ISP's VLAN, a cloned or the board's MAC), `wan` by DHCP, static or PPPoE,
//! and `wan6` by DHCPv6 with prefix delegation.

use uniwrt_model::site::gateway::WanProto;
use uniwrt_uci::Section;

use crate::{ext::SectionExt, plan::Plan};

/// The device sections the WAN needs: an 802.1Q device on its port for an ISP's VLAN, and the
/// MAC (cloned, else the board's) on the device the interface uses.
pub(crate) fn devices(plan: &Plan<'_>) -> Vec<Section> {
    let Some(wan) = &plan.wan else {
        return Vec::new();
    };
    let mac = plan
        .site
        .wan
        .mac
        .or(plan.facts.board.layout.wan_mac)
        .map(|mac| mac.to_string());
    match &wan.vlan_device {
        Some((port, vid)) => vec![
            Section::anonymous("device")
                .with("type", "8021q")
                .with("ifname", port.as_str())
                .with("vid", vid.to_string())
                .with("name", wan.device.as_str())
                .maybe("macaddr", mac),
        ],
        None => mac
            .map(|mac| {
                Section::anonymous("device")
                    .with("name", wan.device.as_str())
                    .with("macaddr", mac)
            })
            .into_iter()
            .collect(),
    }
}

/// `wan`, and `wan6` unless IPv6 is off or PPPoE brings it (`ipv6 auto` adds `wan_6`).
pub(crate) fn interfaces(plan: &Plan<'_>) -> Vec<Section> {
    let Some(wan) = &plan.wan else {
        return Vec::new();
    };
    let config = &plan.site.wan;
    let section = Section::named("interface", "wan").with("device", wan.device.as_str());
    let section = match &config.proto {
        WanProto::Dhcp => section.with("proto", "dhcp"),
        WanProto::Static {
            address,
            gateway,
            dns,
        } => section
            .with("proto", "static")
            .list("ipaddr", [address.to_string()])
            .with("gateway", gateway.to_string())
            .list("dns", dns.iter().map(ToString::to_string)),
        WanProto::Pppoe { username, .. } => section
            .with("proto", "pppoe")
            .with("username", username.as_str())
            .maybe("password", plan.wan_password.as_deref())
            .with("ipv6", if config.ipv6 { "auto" } else { "0" }),
    };
    let mut sections = vec![section];
    if config.ipv6 && !matches!(config.proto, WanProto::Pppoe { .. }) {
        sections.push(
            Section::named("interface", "wan6")
                .with("device", wan.device.as_str())
                .with("proto", "dhcpv6"),
        );
    }
    sections
}
