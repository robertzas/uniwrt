//! The simulated ISP behind the gateway's WAN port (features/wan.md; D-075): like Lumen's
//! fiber, it serves DHCP on VLAN 201, delegates a /56 over DHCPv6 on it, and runs a 6rd border
//! relay. What the gateway's `/etc/config/network` asks for decides what comes up, so a WAN
//! test of the wrong settings fails as it would on the line: PPPoE finds no server, an
//! untagged DHCP no offer.

use uniwrt_uci::Package;

use crate::world::catalog::WAN_VLAN;

/// The ISP's router, the gateway's lease and the 6rd relay.
pub(crate) const ROUTER: &str = "203.0.113.1";
pub(crate) const LEASE: &str = "203.0.113.57";
pub(crate) const BORDER_RELAY: &str = "205.171.2.64";

/// What comes up on the gateway's WAN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Uplink {
    /// `wan`'s protocol and device, and whether it got its address.
    pub(crate) wan: Option<(String, String, bool)>,
    /// `wan6`'s protocol, and whether IPv6 came up on it.
    pub(crate) wan6: Option<(String, bool)>,
}

impl Uplink {
    /// The stock simulated network, as it ran before any configuration was read.
    pub(crate) fn stock(port: &str) -> Self {
        let vlan = format!("{port}.{WAN_VLAN}");
        Self {
            wan: Some(("dhcp".to_owned(), vlan, true)),
            wan6: Some(("dhcpv6".to_owned(), true)),
        }
    }

    pub(crate) fn ipv4(&self) -> bool {
        self.wan.as_ref().is_some_and(|(_, _, up)| *up)
    }
}

/// What the ISP gives a gateway configured as `network`, its WAN on `port`.
pub(crate) fn uplink(network: Option<&str>, port: &str) -> Uplink {
    let Some(network) = network.and_then(|text| uniwrt_uci::parse("network", text).ok()) else {
        return Uplink::stock(port);
    };
    let tagged = format!("{port}.{WAN_VLAN}");
    let on_vlan = |device: &str| device == tagged || declared_vlan(&network, device, port);
    let wan = network
        .section("wan")
        .filter(|s| s.kind == "interface")
        .map(|wan| {
            let proto = wan.option("proto").unwrap_or("none").to_owned();
            let device = wan.option("device").unwrap_or_default().to_owned();
            let up = match proto.as_str() {
                "dhcp" => on_vlan(&device),
                "static" => on_vlan(&device) && wan.option("gateway") == Some(ROUTER),
                _ => false,
            };
            (proto, device, up)
        });
    let ipv4 = wan.as_ref().is_some_and(|(_, _, up)| *up);
    let wan6 = network
        .section("wan6")
        .filter(|s| s.kind == "interface")
        .map(|wan6| {
            let proto = wan6.option("proto").unwrap_or("none").to_owned();
            let up = match proto.as_str() {
                "dhcpv6" => wan6.option("device").is_some_and(on_vlan),
                "6rd" => ipv4 && wan6.option("peeraddr") == Some(BORDER_RELAY),
                _ => false,
            };
            (proto, up)
        });
    Uplink { wan, wan6 }
}

/// Whether `device` is an 802.1Q device on `port` in the ISP's VLAN.
fn declared_vlan(network: &Package, device: &str, port: &str) -> bool {
    network.sections_of("device").any(|d| {
        d.option("name") == Some(device)
            && d.option("type") == Some("8021q")
            && d.option("ifname") == Some(port)
            && d.option("vid") == Some(&WAN_VLAN.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::{Uplink, uplink};

    fn network(wan: &str, wan6: &str) -> String {
        format!("config interface 'wan'\n{wan}\nconfig interface 'wan6'\n{wan6}\n")
    }

    #[test]
    fn the_isp_answers_only_what_it_serves() {
        assert_eq!(uplink(None, "wan"), Uplink::stock("wan"));
        let lumen = network(
            "\toption device 'wan.201'\n\toption proto 'dhcp'",
            "\toption device 'wan.201'\n\toption proto 'dhcpv6'",
        );
        let up = uplink(Some(&lumen), "wan");
        assert!(up.ipv4());
        assert_eq!(up.wan6, Some(("dhcpv6".to_owned(), true)));
        let untagged = network(
            "\toption device 'wan'\n\toption proto 'dhcp'",
            "\toption device 'wan'\n\toption proto 'dhcpv6'",
        );
        let down = uplink(Some(&untagged), "wan");
        assert!(!down.ipv4());
        assert_eq!(down.wan6, Some(("dhcpv6".to_owned(), false)));
        let pppoe = network(
            "\toption device 'wan.201'\n\toption proto 'pppoe'",
            "\toption device '@wan'\n\toption proto 'dhcpv6'",
        );
        assert!(!uplink(Some(&pppoe), "wan").ipv4());
        let sixrd = network(
            "\toption device 'wan.201'\n\toption proto 'dhcp'",
            "\toption proto '6rd'\n\toption peeraddr '205.171.2.64'",
        );
        assert_eq!(
            uplink(Some(&sixrd), "wan").wan6,
            Some(("6rd".to_owned(), true))
        );
    }
}
