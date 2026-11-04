//! Where the controller finds a node once an apply renumbers it (the owner's choice: follow the
//! new address, features/lifecycle.md). The gateway's address is its static one on the
//! network that holds it now; an access point's comes from DHCP on the management network,
//! so it stays only while that subnet holds it, else it is the node's reservation.

use std::net::Ipv4Addr;

use uniwrt_model::{
    facts::NodeFacts,
    node::{Node, NodeRole},
    site::SiteConfig,
};

/// The node's address once `site` applies: `Ok(None)` while it stays `node.host` (or that is
/// a name, which follows by itself), `Ok(Some(new))` when it moves, and `Err` with what keeps
/// the controller from following it.
pub(crate) fn expected(
    node: &Node,
    facts: &NodeFacts,
    site: &SiteConfig,
) -> Result<Option<Ipv4Addr>, String> {
    let Ok(ip) = node.host.parse::<Ipv4Addr>() else {
        return Ok(None);
    };
    let moved = |new: Ipv4Addr| Ok((new != ip).then_some(new));
    if node.role == NodeRole::Gateway {
        let Some(iface) = facts
            .interfaces
            .iter()
            .find(|iface| iface.ipv4.iter().any(|net| net.addr() == ip))
        else {
            // Reached through something else (a NAT, another router): nothing here moves it.
            return Ok(None);
        };
        if iface.name.starts_with("wan") {
            return Ok(None);
        }
        return match site.networks.iter().find(|net| net.id == iface.name) {
            Some(net) => match &net.ipv4 {
                Some(ipv4) => moved(ipv4.address.addr()),
                None => Err(format!(
                    "The controller reaches the gateway at {ip} on network {}, which would \
                     have no IPv4 address",
                    net.id
                )),
            },
            None => Err(format!(
                "The controller reaches the gateway at {ip} on interface {}, which the site \
                 doesn't have",
                iface.name
            )),
        };
    }
    let Some(net) = uniwrt_render::management_network(site) else {
        return Err(
            "The site has no management or default network to reach the access points on"
                .to_owned(),
        );
    };
    let Some(ipv4) = &net.ipv4 else {
        return Err(format!(
            "The management network {} has no IPv4 subnet to reach the access points on",
            net.id
        ));
    };
    let macs: Vec<_> = std::iter::once(node.mac)
        .chain(facts.devices.iter().filter_map(|device| device.mac))
        .collect();
    let reserved = ipv4.dhcp.as_ref().and_then(|dhcp| {
        dhcp.reservations
            .iter()
            .find(|reservation| macs.contains(&reservation.mac))
    });
    match reserved {
        Some(reservation) => moved(reservation.ip),
        None if ipv4.address.trunc().contains(&ip) => Ok(None),
        None => Err(format!(
            "{} would take a new address from DHCP on {} ({}): reserve one for {} so the \
             controller can follow it",
            node.name,
            net.id,
            ipv4.address.trunc(),
            node.mac
        )),
    }
}

#[cfg(test)]
mod tests {
    use uniwrt_core::{MacAddr, NodeId, Timestamp};
    use uniwrt_model::{
        facts::NodeFacts,
        node::{Node, NodeMode, NodeRole},
        site::{SiteConfig, network::Reservation},
    };

    use super::expected;

    const MAC: MacAddr = MacAddr::new([0x02, 0, 0, 0, 0, 0x14]);

    fn node(role: NodeRole, host: &str) -> Node {
        Node {
            id: NodeId::new(1),
            name: "ap-hall".to_owned(),
            mac: MAC,
            board_name: "x86".to_owned(),
            host: host.to_owned(),
            port: 22,
            role,
            mode: NodeMode::Managed,
            created_at: Timestamp::from_unix_ms(0),
            updated_at: Timestamp::from_unix_ms(0),
        }
    }

    fn facts(iface: &str, addr: &str) -> NodeFacts {
        let mut facts = NodeFacts::default();
        facts.interfaces.push(crate::apply::tests::facts::interface(
            iface, addr, true, false,
        ));
        facts
    }

    /// The defaults: the LAN at 10.0.0.1/24.
    fn site() -> SiteConfig {
        SiteConfig::defaults(None)
    }

    #[test]
    fn the_gateway_follows_the_network_that_holds_its_address() {
        let gw = node(NodeRole::Gateway, "192.168.1.1");
        assert_eq!(
            expected(&gw, &facts("lan", "192.168.1.1/24"), &site()),
            Ok(Some("10.0.0.1".parse().unwrap()))
        );
        let gw = node(NodeRole::Gateway, "10.0.0.1");
        assert_eq!(
            expected(&gw, &facts("lan", "10.0.0.1/24"), &site()),
            Ok(None)
        );
        assert_eq!(
            expected(&gw, &facts("wan", "10.0.0.1/24"), &site()),
            Ok(None)
        );
        assert!(
            expected(&gw, &facts("guest", "10.0.0.1/24"), &site())
                .unwrap_err()
                .contains("interface guest")
        );
        let named = node(NodeRole::Gateway, "router.home.arpa");
        assert_eq!(expected(&named, &NodeFacts::default(), &site()), Ok(None));
    }

    #[test]
    fn an_access_point_stays_in_its_subnet_or_needs_a_reservation() {
        let facts = facts("lan", "192.168.1.20/24");
        let inside = node(NodeRole::Ap, "10.0.0.120");
        assert_eq!(expected(&inside, &facts, &site()), Ok(None));
        let outside = node(NodeRole::Ap, "192.168.1.20");
        let refused = expected(&outside, &facts, &site()).unwrap_err();
        assert!(
            refused.contains("reserve one for 02:00:00:00:00:14"),
            "{refused}"
        );
        let mut site = site();
        let dhcp = site.networks[0]
            .ipv4
            .as_mut()
            .unwrap()
            .dhcp
            .as_mut()
            .unwrap();
        dhcp.reservations.push(Reservation {
            mac: MAC,
            ip: "10.0.0.20".parse().unwrap(),
            name: None,
        });
        assert_eq!(
            expected(&outside, &facts, &site),
            Ok(Some("10.0.0.20".parse().unwrap()))
        );
    }
}
