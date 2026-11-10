//! Where the controller finds a node once an apply renumbers it (the owner's choice: follow the
//! new address, features/lifecycle.md). The gateway's address is its static one on the
//! network that holds it now; an access point's comes from DHCP on the management network,
//! so it stays only while that subnet holds it, else it is the node's reservation. And whether
//! the network the controller itself sits on moves ([`renumbering`]), which the apply does
//! make-before-break (D-081).

use std::net::{IpAddr, Ipv4Addr};

use uniwrt_core::Ipv4Net;
use uniwrt_model::{
    apply::Renumbering,
    facts::NodeFacts,
    node::{Node, NodeRole},
    site::SiteConfig,
};

/// The network the controller sits on moving under it: the gateway's address on it now and
/// once applied, both with the prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Bridge {
    pub(crate) network: String,
    pub(crate) old: Ipv4Net,
    pub(crate) new: Ipv4Net,
}

impl Bridge {
    pub(crate) fn plan(&self) -> Renumbering {
        Renumbering {
            network: self.network.clone(),
            from: self.old.trunc().to_string(),
            to: self.new.trunc().to_string(),
        }
    }
}

/// Whether applying `site` moves the network `controller` (the gateway's view of its login)
/// sits on from under it: `Ok(Some)` when the network's new subnet won't hold the controller's
/// address, `Err` when the network loses its IPv4 address (the controller would lose the
/// gateway), `Ok(None)` when the controller stays inside, sits on the WAN or on no network of
/// the site's.
pub(crate) fn renumbering(
    facts: &NodeFacts,
    site: &SiteConfig,
    controller: IpAddr,
) -> Result<Option<Bridge>, String> {
    let IpAddr::V4(controller) = controller else {
        return Ok(None);
    };
    let Some((iface, old)) = facts.interfaces.iter().find_map(|iface| {
        let net = iface.ipv4.iter().find(|net| net.contains(&controller))?;
        Some((iface, *net))
    }) else {
        return Ok(None);
    };
    if iface.name.starts_with("wan") {
        return Ok(None);
    }
    let Some(net) = site.networks.iter().find(|net| net.id == iface.name) else {
        return Ok(None);
    };
    let Some(ipv4) = &net.ipv4 else {
        return Err(format!(
            "This controller reaches the gateway from {controller} on network {}, which would \
             have no IPv4 address",
            net.id
        ));
    };
    if ipv4.address.contains(&controller) {
        return Ok(None);
    }
    Ok(Some(Bridge {
        network: net.id.clone(),
        old,
        new: ipv4.address,
    }))
}

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

    use super::{Bridge, expected, renumbering};

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
    fn the_controllers_network_moves_make_before_break() {
        let stock = facts("lan", "192.168.1.1/24");
        let controller = "192.168.1.50".parse().unwrap();
        assert_eq!(
            renumbering(&stock, &site(), controller),
            Ok(Some(Bridge {
                network: "lan".to_owned(),
                old: "192.168.1.1/24".parse().unwrap(),
                new: "10.0.0.1/24".parse().unwrap(),
            }))
        );
        let bridge = renumbering(&stock, &site(), controller).unwrap().unwrap();
        assert_eq!(
            (bridge.plan().from, bridge.plan().to),
            ("192.168.1.0/24".to_owned(), "10.0.0.0/24".to_owned())
        );
        // Inside the new subnet, over the WAN, or on no network of the site's: nothing moves.
        let inside = facts("lan", "10.0.0.1/24");
        assert_eq!(
            renumbering(&inside, &site(), "10.0.0.50".parse().unwrap()),
            Ok(None)
        );
        assert_eq!(
            renumbering(&facts("wan", "192.168.1.1/24"), &site(), controller),
            Ok(None)
        );
        assert_eq!(
            renumbering(&facts("guest", "192.168.1.1/24"), &site(), controller),
            Ok(None)
        );
        assert_eq!(
            renumbering(&stock, &site(), "fd12::50".parse().unwrap()),
            Ok(None)
        );
        let mut site = site();
        site.networks[0].ipv4 = None;
        assert!(
            renumbering(&stock, &site, controller)
                .unwrap_err()
                .contains("would have no IPv4 address")
        );
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
