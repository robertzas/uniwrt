//! Wake-on-LAN from the gateway (features/network.md, Other services; D-137): `etherwake` sends
//! the magic packet (`EtherType` `0x0842`) to the broadcast address, so a bridge's stale entry for
//! the sleeping client can't send it elsewhere. Stock images have no sender: the package comes
//! from the feeds ([`PACKAGE`]).

use std::{collections::BTreeSet, net::IpAddr};

use uniwrt_core::MacAddr;
use uniwrt_model::facts::{InterfaceFacts, NodeFacts};

/// The package that brings the sender.
pub const PACKAGE: &str = "etherwake";

/// What `sh` exits with for a command it cannot find: the gateway lacks [`PACKAGE`].
pub const NOT_FOUND: u32 = 127;

/// Sends the magic packet for `mac` on `device`.
pub fn command(device: &str, mac: MacAddr) -> Vec<String> {
    ["etherwake", "-b", "-i", device, &mac.to_string()]
        .map(str::to_owned)
        .to_vec()
}

/// The gateway's devices to send on, sorted: the network of the client's IPv4 address, else
/// the one its lease or neighbor entry names, else every network the gateway serves (a static
/// address without a gateway: never a WAN).
pub fn devices(facts: &NodeFacts, mac: MacAddr, ips: &[IpAddr]) -> Vec<String> {
    let lans: Vec<(&InterfaceFacts, &str)> = facts
        .interfaces
        .iter()
        .filter(|i| i.up && i.proto == "static" && i.ipv4_gateway.is_none())
        .filter(|i| !i.ipv4.is_empty() && i.name != "loopback")
        .filter_map(|i| Some((i, i.device.as_deref()?)))
        .collect();
    let by_address: BTreeSet<&str> = lans
        .iter()
        .filter(|(i, _)| {
            ips.iter().any(|ip| match ip {
                IpAddr::V4(ip) => i.ipv4.iter().any(|net| net.contains(ip)),
                IpAddr::V6(_) => false,
            })
        })
        .map(|&(_, device)| device)
        .collect();
    let served = |device: &str| lans.iter().any(|&(_, d)| d == device);
    let seen: BTreeSet<&str> = facts
        .leases
        .iter()
        .filter(|lease| lease.mac == Some(mac))
        .filter_map(|lease| lease.device.as_deref())
        .chain(
            facts
                .neighbors
                .iter()
                .filter(|n| n.mac == mac)
                .map(|n| n.device.as_str()),
        )
        .filter(|device| served(device))
        .collect();
    let chosen = if !by_address.is_empty() {
        by_address
    } else if !seen.is_empty() {
        seen
    } else {
        lans.iter().map(|&(_, device)| device).collect()
    };
    chosen.into_iter().map(str::to_owned).collect()
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use uniwrt_core::MacAddr;
    use uniwrt_model::facts::{InterfaceFacts, LeaseFacts, NeighborFacts, NodeFacts};

    use super::{command, devices};

    fn interface(name: &str, proto: &str, device: &str, ipv4: &str) -> InterfaceFacts {
        InterfaceFacts {
            name: name.to_owned(),
            proto: proto.to_owned(),
            up: true,
            device: Some(device.to_owned()),
            uptime_s: None,
            ipv4: vec![ipv4.parse().unwrap()],
            ipv6: Vec::new(),
            delegated_prefixes: Vec::new(),
            ipv4_gateway: None,
            ipv6_gateway: None,
            dns_servers: Vec::new(),
            errors: Vec::new(),
        }
    }

    fn gateway() -> NodeFacts {
        let mut wan = interface("wan", "static", "eth1", "203.0.113.2/24");
        wan.ipv4_gateway = Some("203.0.113.1".parse().unwrap());
        NodeFacts {
            interfaces: vec![
                interface("loopback", "static", "lo", "127.0.0.1/8"),
                interface("lan", "static", "br-lan", "192.168.1.1/24"),
                interface("iot", "static", "br-lan.30", "192.168.30.1/24"),
                wan,
            ],
            ..NodeFacts::default()
        }
    }

    const MAC: &str = "02:11:22:33:44:55";

    fn mac() -> MacAddr {
        MAC.parse().unwrap()
    }

    #[test]
    fn the_packet_goes_out_where_the_client_lives() {
        let facts = gateway();
        let ip: IpAddr = "192.168.30.20".parse().unwrap();
        assert_eq!(devices(&facts, mac(), &[ip]), ["br-lan.30"]);
        assert_eq!(
            command("br-lan.30", mac()),
            ["etherwake", "-b", "-i", "br-lan.30", MAC]
        );
    }

    #[test]
    fn without_an_address_its_lease_or_neighbor_entry_tells() {
        let mut facts = gateway();
        facts.leases.push(LeaseFacts {
            ip: "192.168.30.21".parse().unwrap(),
            mac: Some(mac()),
            hostname: None,
            client_id: None,
            device: Some("br-lan.30".to_owned()),
            expires_in_s: None,
        });
        assert_eq!(devices(&facts, mac(), &[]), ["br-lan.30"]);
        facts.leases.clear();
        facts.neighbors.push(NeighborFacts {
            ip: "192.168.1.9".parse().unwrap(),
            mac: mac(),
            device: "br-lan".to_owned(),
            state: None,
            router: false,
        });
        assert_eq!(devices(&facts, mac(), &[]), ["br-lan"]);
    }

    #[test]
    fn an_unknown_client_is_woken_on_every_served_network() {
        let facts = gateway();
        let v6: IpAddr = "fd00::5".parse().unwrap();
        assert_eq!(devices(&facts, mac(), &[v6]), ["br-lan", "br-lan.30"]);
    }
}
