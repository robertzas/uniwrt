//! The gateway's uplink in the `network` package (features/wan.md, Rendering on the gateway):
//! the WAN device (an ISP's VLAN with its 802.1p priority, a cloned or the board's MAC, the
//! MTU), `wan` by DHCP, static, PPPoE or DS-Lite, and `wan6` by DHCPv6 with prefix delegation
//! or by 6rd. The delegated prefix always arrives on `wan6`.

use uniwrt_model::site::gateway::{DhcpClient, WanIpv6, WanProto};
use uniwrt_uci::Section;

use crate::{ext::SectionExt, plan::Plan};

/// The interface IPv6 arrives on, and the delegated prefix with it.
pub(crate) const WAN6: &str = "wan6";

/// What 6rd's IPv4 header takes from the MTU.
const SIXRD_OVERHEAD: u16 = 20;

/// The device sections the WAN needs: an 802.1Q device on its port for an ISP's VLAN (with the
/// priority every frame leaves with), and the MAC (cloned, else the board's) and MTU on the
/// device the interface uses. PPPoE takes its MTU on the interface instead.
pub(crate) fn devices(plan: &Plan<'_>) -> Vec<Section> {
    let Some(wan) = &plan.wan else {
        return Vec::new();
    };
    let config = &plan.site.wan;
    let mac = config
        .mac
        .or(plan.facts.board.layout.wan_mac)
        .map(|mac| mac.to_string());
    let mtu = config
        .mtu
        .filter(|_| !matches!(config.proto, WanProto::Pppoe { .. }))
        .map(|mtu| mtu.to_string());
    match &wan.vlan_device {
        Some((port, vid)) => vec![
            Section::anonymous("device")
                .with("type", "8021q")
                .with("ifname", port.as_str())
                .with("vid", vid.to_string())
                .with("name", wan.device.as_str())
                .maybe("macaddr", mac)
                .maybe("mtu", mtu)
                .list(
                    "egress_qos_mapping",
                    config
                        .pcp
                        .filter(|&pcp| pcp > 0)
                        .into_iter()
                        .flat_map(|pcp| (0..8).map(move |prio| format!("{prio}:{pcp}"))),
                ),
        ],
        None if mac.is_some() || mtu.is_some() => vec![
            Section::anonymous("device")
                .with("name", wan.device.as_str())
                .maybe("macaddr", mac)
                .maybe("mtu", mtu),
        ],
        None => Vec::new(),
    }
}

/// `wan` (unless the WAN is IPv6-only, or DS-Lite's AFTR comes by DHCPv6 and netifd adds it),
/// and `wan6` unless IPv6 is off.
pub(crate) fn interfaces(plan: &Plan<'_>) -> Vec<Section> {
    let Some(wan) = &plan.wan else {
        return Vec::new();
    };
    let config = &plan.site.wan;
    let device = wan.device.as_str();
    let ipv4 = Section::named("interface", "wan").with("device", device);
    let ipv4 = match &config.proto {
        WanProto::Dhcp(client) => Some(dhcp(ipv4.with("proto", "dhcp"), client)),
        WanProto::Static {
            address,
            gateway,
            dns,
        } => Some(
            ipv4.with("proto", "static")
                .list("ipaddr", [address.to_string()])
                .with("gateway", gateway.to_string())
                .list("dns", dns.iter().map(ToString::to_string)),
        ),
        WanProto::Pppoe {
            username, service, ..
        } => Some(
            ipv4.with("proto", "pppoe")
                .with("username", username.as_str())
                .maybe("password", plan.wan_password.as_deref())
                .maybe("service", service.as_deref())
                .maybe("mtu", config.mtu.map(|mtu| mtu.to_string()))
                // IPv6 on the PPP link; `wan6` asks for the prefix over it.
                .with("ipv6", if config.ipv6.is_on() { "1" } else { "0" }),
        ),
        WanProto::DsLite { aftr: Some(aftr) } => Some(
            Section::named("interface", "wan")
                .with("proto", "dslite")
                .with("peeraddr", aftr.as_str())
                .with("tunlink", WAN6)
                .with("encaplimit", "ignore"),
        ),
        WanProto::DsLite { aftr: None } | WanProto::None => None,
    };
    let ipv6 = match &config.ipv6 {
        WanIpv6::Off => None,
        WanIpv6::Dhcpv6 { reqprefix } => {
            let on = match config.proto {
                WanProto::Pppoe { .. } => "@wan",
                _ => device,
            };
            let section = Section::named("interface", WAN6)
                .with("device", on)
                .with("proto", "dhcpv6")
                .maybe("reqprefix", reqprefix.map(|len| len.to_string()));
            Some(match config.proto {
                // netifd adds `wan` as the tunnel to the AFTR DHCPv6 announces.
                WanProto::DsLite { aftr: None } => section
                    .with("iface_dslite", "wan")
                    .with("zone_dslite", "wan")
                    .with("encaplimit_dslite", "ignore"),
                _ => section,
            })
        }
        WanIpv6::SixRd {
            prefix,
            border_relay,
            ipv4_mask_len,
        } => {
            let base = config.mtu.unwrap_or(match config.proto {
                WanProto::Pppoe { .. } => 1492,
                _ => 1500,
            });
            Some(
                Section::named("interface", WAN6)
                    .with("proto", "6rd")
                    .with("peeraddr", border_relay.to_string())
                    .with("ip6prefix", prefix.network().to_string())
                    .with("ip6prefixlen", prefix.prefix_len().to_string())
                    .with("ip4prefixlen", ipv4_mask_len.to_string())
                    .with("tunlink", "wan")
                    .with("mtu", (base - SIXRD_OVERHEAD).to_string()),
            )
        }
    };
    ipv4.into_iter().chain(ipv6).collect()
}

/// What the DHCP client sends besides its defaults.
fn dhcp(section: Section, client: &DhcpClient) -> Section {
    section
        .maybe("vendorid", client.vendor_class.as_deref())
        .maybe("clientid", client.client_id.as_deref())
        .maybe(
            "sendopts",
            (!client.send_options.is_empty()).then(|| client.send_options.join(" ")),
        )
        .maybe("hostname", client.hostname.as_deref())
}
