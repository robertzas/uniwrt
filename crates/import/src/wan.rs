//! The WAN, from the gateway (features/wan.md): `interface wan` by DHCP (with its client
//! options), static, PPPoE or DS-Lite, else a `wan6` alone (IPv6-only, or DS-Lite whose AFTR
//! DHCPv6 announces); the ISP's VLAN (an 802.1Q device, `<port>.<vid>`, or a tagged switch
//! VLAN) and its priority, a cloned MAC, the MTU; and IPv6 by 6rd, by DHCPv6 on the WAN's
//! device or over PPP (`@wan`, or PPPoE's own `ipv6 auto`).

use std::net::IpAddr;

use uniwrt_model::site::{
    SecretRef, SiteConfig,
    gateway::{DhcpClient, WanConfig, WanIpv6, WanProto},
};
use uniwrt_uci::{Package, Section};

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
    let interface = |name: &str| network.section(name).filter(|s| s.kind == "interface");
    let mut keep_whole = || {
        renames
            .whole
            .insert(("network".to_owned(), "wan".to_owned()));
    };
    let (proto, device, carrier) = match interface("wan") {
        Some(wan) if wan.option("proto") == Some("dslite") => {
            let tunnel = wan.option("tunlink").and_then(interface);
            let Some(device) = tunnel.and_then(|t| t.option("device")) else {
                return keep_whole();
            };
            let aftr = wan.option("peeraddr").map(str::to_owned);
            (WanProto::DsLite { aftr }, device, tunnel)
        }
        Some(wan) => {
            let Some(device) = wan.option("device") else {
                return keep_whole();
            };
            let Some(proto) = ipv4_proto(wan, seal) else {
                return keep_whole();
            };
            (proto, device, Some(wan))
        }
        None => {
            let Some(wan6) = interface("wan6").filter(|s| s.option("proto") == Some("dhcpv6"))
            else {
                return;
            };
            let Some(device) = wan6.option("device") else {
                return;
            };
            let proto = if wan6.option("iface_dslite").is_some_and(|v| v != "0") {
                WanProto::DsLite { aftr: None }
            } else {
                WanProto::None
            };
            (proto, device, Some(wan6))
        }
    };
    let layout = &gateway.facts.board.layout;
    let declared = network
        .sections_of("device")
        .find(|d| d.option("name") == Some(device));
    let vlan_device = declared.filter(|d| d.option("type") == Some("8021q"));
    let vlan = vlan_device
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
    let mtu = match proto {
        WanProto::Pppoe { .. } => carrier.and_then(|s| s.option("mtu")),
        _ => declared
            .and_then(|d| d.option("mtu"))
            .or_else(|| carrier.and_then(|s| s.option("mtu"))),
    }
    .and_then(|mtu| mtu.parse().ok());
    let (ipv6, sixrd) = ipv6(network, &proto, device, carrier);
    if let Some(name) = sixrd.filter(|name| name != WAN6) {
        renames.interfaces.insert(name, WAN6.to_owned());
    }
    site.wan = WanConfig {
        proto,
        vlan,
        pcp: vlan_device.and_then(pcp),
        mtu,
        mac,
        ipv6,
        preset: None,
    };
}

fn ipv4_proto(wan: &Section, seal: &mut dyn FnMut(&str) -> SecretRef) -> Option<WanProto> {
    let owned = |name: &str| wan.option(name).map(str::to_owned);
    match wan.option("proto")? {
        "dhcp" => Some(WanProto::Dhcp(DhcpClient {
            vendor_class: owned("vendorid"),
            client_id: owned("clientid"),
            send_options: read::items(wan, "sendopts")
                .into_iter()
                .map(str::to_owned)
                .collect(),
            hostname: owned("hostname"),
        })),
        "static" => Some(WanProto::Static {
            address: read::ipv4(wan)?,
            gateway: wan.option("gateway")?.parse().ok()?,
            dns: read::items(wan, "dns")
                .into_iter()
                .filter_map(|d| d.parse::<IpAddr>().ok())
                .collect(),
        }),
        "pppoe" => Some(WanProto::Pppoe {
            username: owned("username")?,
            password: wan.option("password").map(seal),
            service: owned("service"),
        }),
        _ => None,
    }
}

const WAN6: &str = "wan6";

/// IPv6: 6rd on `wan6`, or on another interface when there is no `wan6` (render names it
/// `wan6`: its name comes back with it), else DHCPv6 on the WAN's device or over PPP, else
/// PPPoE's own (`ipv6` unset or `auto` adds `wan_6`).
fn ipv6(
    network: &Package,
    proto: &WanProto,
    device: &str,
    carrier: Option<&Section>,
) -> (WanIpv6, Option<String>) {
    let interfaces = || network.sections_of("interface");
    let has_wan6 = interfaces().any(|s| s.name.as_deref() == Some(WAN6));
    let sixrd = interfaces().find(|s| {
        s.option("proto") == Some("6rd") && (!has_wan6 || s.name.as_deref() == Some(WAN6))
    });
    if let Some(sixrd) = sixrd {
        let prefix = sixrd
            .option("ip6prefix")
            .zip(sixrd.option("ip6prefixlen"))
            .and_then(|(addr, len)| format!("{addr}/{len}").parse().ok());
        let relay = sixrd.option("peeraddr").and_then(|a| a.parse().ok());
        let mask = sixrd
            .option("ip4prefixlen")
            .map_or(Some(0), |m| m.parse().ok());
        if let (Some(prefix), Some(border_relay), Some(ipv4_mask_len)) = (prefix, relay, mask) {
            let sixrd_ipv6 = WanIpv6::SixRd {
                prefix,
                border_relay,
                ipv4_mask_len,
            };
            return (sixrd_ipv6, sixrd.name.clone());
        }
    }
    let pppoe = matches!(proto, WanProto::Pppoe { .. });
    let dhcpv6 = interfaces().find(|s| {
        s.option("proto") == Some("dhcpv6")
            && matches!(s.option("device"), Some(d) if d == device || d == "@wan")
    });
    if let Some(dhcpv6) = dhcpv6 {
        let ppp_off = pppoe && carrier.and_then(|c| c.option("ipv6")) == Some("0");
        if !ppp_off {
            let reqprefix = dhcpv6.option("reqprefix").and_then(|r| r.parse().ok());
            return (WanIpv6::Dhcpv6 { reqprefix }, None);
        }
    }
    let ipv6 = match carrier.and_then(|c| c.option("ipv6")) {
        None | Some("auto") if pppoe => WanIpv6::Dhcpv6 { reqprefix: None },
        _ => WanIpv6::Off,
    };
    (ipv6, None)
}

/// The priority an 802.1Q device gives every frame: its egress mappings all to one PCP.
fn pcp(device: &Section) -> Option<u8> {
    let mut pcps = read::items(device, "egress_qos_mapping")
        .into_iter()
        .map(|m| {
            m.split_once(':')
                .and_then(|(_, pcp)| pcp.parse::<u8>().ok())
        });
    let first = pcps.next()??;
    pcps.all(|p| p == Some(first)).then_some(first)
}

/// The ISP's VLAN on a swconfig switch: the WAN CPU port's `<cpu>.<vid>`, when the WAN's user
/// ports tag it.
fn switch_vlan<'p>(network: &'p Package, gateway: &Source<'_>, device: &'p str) -> Option<&'p str> {
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
