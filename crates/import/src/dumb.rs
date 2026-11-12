//! What a router's configuration loses when it becomes an access point or a bridge (D-100's
//! dumb AP, adoption.md's Roles): its DHCP server, WAN and static LAN address are the
//! gateway's job, so render never writes them on such a node. The import drops them by design
//! instead of keeping them raw, and the residual shows what the first apply removes. The
//! firewall's zones and rules on such a node are dropped in `fit::pair`.

use uniwrt_model::node::NodeRole;
use uniwrt_uci::Section;

/// The interfaces a router's WAN goes by in OpenWrt's defaults, and render's backup line.
const WAN_NAMES: [&str; 3] = ["wan", "wan6", "wanb"];

/// The protocols of an IP uplink: an interface on the WAN port with one of them is the
/// router's WAN (a batman-adv hard interface there is a wired backhaul, and stays).
const UPLINK_PROTOS: [&str; 11] = [
    "dhcp",
    "dhcpv6",
    "static",
    "pppoe",
    "pppoa",
    "ppp",
    "3g",
    "qmi",
    "mbim",
    "ncm",
    "modemmanager",
];

/// IPv6 transition tunnels: they ride the router's WAN, wherever their device is.
const TUNNEL_PROTOS: [&str; 6] = ["6rd", "6in4", "6to4", "dslite", "map", "464xlat"];

/// An interface's addressing: an access point takes its address from the gateway's DHCP.
const ADDRESSING: [&str; 13] = [
    "ipaddr",
    "netmask",
    "gateway",
    "broadcast",
    "dns",
    "peerdns",
    "ip6addr",
    "ip6gw",
    "ip6prefix",
    "ip6assign",
    "ip6hint",
    "ip6class",
    "ip6ifaceid",
];

/// Whether `section` of `package`, which render has nothing to pair with, goes on a node of
/// `role` whose WAN port is `wan`: a WAN interface (by name, an IP uplink on the WAN port, a
/// VLAN of it or a WAN interface, or an IPv6 transition tunnel), an 802.1Q device on the WAN
/// port (the ISP's VLAN), or anything of
/// the DHCP server's (pools render doesn't write, reservations, local names, boot entries).
pub(crate) fn dropped(role: NodeRole, package: &str, section: &Section, wan: Option<&str>) -> bool {
    if role == NodeRole::Gateway {
        return false;
    }
    let on_wan = |device: &str| {
        wan.is_some_and(|wan| {
            device == wan
                || device
                    .strip_prefix(wan)
                    .is_some_and(|rest| rest.starts_with('.'))
        }) || WAN_NAMES.iter().any(|w| device == format!("@{w}"))
    };
    match (package, section.kind.as_str()) {
        ("network", "interface") => {
            let proto = section.option("proto").unwrap_or_default();
            section
                .name
                .as_deref()
                .is_some_and(|name| WAN_NAMES.contains(&name))
                || TUNNEL_PROTOS.contains(&proto)
                || UPLINK_PROTOS.contains(&proto) && section.option("device").is_some_and(on_wan)
        }
        ("network", "device") => {
            matches!(section.option("type"), Some("8021q" | "8021ad"))
                && section.option("ifname").is_some_and(on_wan)
        }
        ("dhcp", kind) => kind != "dnsmasq" && kind != "odhcpd",
        _ => false,
    }
}

/// Whether an option render doesn't write in a section paired with render's goes on a node of
/// `role`: an interface's addressing, or a DHCP pool's settings (render writes `ignore`).
pub(crate) fn option_dropped(role: NodeRole, package: &str, kind: &str, option: &str) -> bool {
    role != NodeRole::Gateway
        && match (package, kind) {
            ("network", "interface") => ADDRESSING.contains(&option),
            ("dhcp", "dhcp") => true,
            _ => false,
        }
}

#[cfg(test)]
mod tests {
    use uniwrt_model::node::NodeRole;
    use uniwrt_uci::Section;

    use super::{dropped, option_dropped};

    #[test]
    fn only_a_non_gateway_drops_the_router_parts() {
        let wan = Section::named("interface", "wan").with("device", "eth1");
        let on_port = Section::named("interface", "uplink")
            .with("device", "eth1")
            .with("proto", "dhcp");
        let alias = Section::named("interface", "up6")
            .with("device", "@wan")
            .with("proto", "dhcpv6");
        let backhaul = Section::named("interface", "batwired")
            .with("device", "eth1")
            .with("proto", "batadv_hardif");
        let vpn = Section::named("interface", "vpn").with("proto", "wireguard");
        let host = Section::anonymous("host").with("mac", "02:00:00:00:00:01");
        let dnsmasq = Section::anonymous("dnsmasq");
        for role in [NodeRole::Ap, NodeRole::Bridge] {
            assert!(dropped(role, "network", &wan, Some("eth1")));
            assert!(dropped(role, "network", &on_port, Some("eth1")));
            assert!(dropped(role, "network", &alias, None));
            assert!(!dropped(role, "network", &vpn, Some("eth1")));
            assert!(!dropped(role, "network", &backhaul, Some("eth1")));
            let isp = Section::anonymous("device")
                .with("type", "8021q")
                .with("ifname", "eth1")
                .with("vid", "201");
            let on_vlan = Section::named("interface", "up")
                .with("device", "eth1.201")
                .with("proto", "pppoe");
            let tunnel = Section::named("interface", "v6").with("proto", "6rd");
            assert!(dropped(role, "network", &isp, Some("eth1")));
            assert!(dropped(role, "network", &on_vlan, Some("eth1")));
            assert!(!dropped(role, "network", &on_vlan, Some("eth10")));
            assert!(dropped(role, "network", &tunnel, None));
            assert!(dropped(role, "dhcp", &host, None));
            assert!(!dropped(role, "dhcp", &dnsmasq, None));
            assert!(option_dropped(role, "network", "interface", "ipaddr"));
            assert!(!option_dropped(role, "network", "interface", "mtu"));
            assert!(option_dropped(role, "dhcp", "dhcp", "limit"));
            assert!(!option_dropped(role, "dhcp", "dnsmasq", "local"));
        }
        assert!(!dropped(NodeRole::Gateway, "network", &wan, Some("eth1")));
        assert!(!dropped(NodeRole::Gateway, "dhcp", &host, None));
        assert!(!option_dropped(
            NodeRole::Gateway,
            "network",
            "interface",
            "ipaddr"
        ));
    }
}
