//! Sightings: the devices the adopted nodes see in their DHCP leases, their IPv4 neighbors on
//! non-WAN interfaces and their batman-adv meshes, less the adopted nodes themselves. Pure.

use std::{
    collections::{BTreeMap, BTreeSet},
    net::IpAddr,
    sync::Arc,
};

use uniwrt_core::{MacAddr, NodeId};
use uniwrt_model::{discovery::DiscoverySource, facts::NodeFacts, node::Node};

use crate::adopt::check::wan_like;

/// A device the adopted nodes see, one per MAC address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Sighting {
    pub(crate) mac: MacAddr,
    /// The best address seen: IPv4, else a routable IPv6 one.
    pub(crate) ip: Option<IpAddr>,
    pub(crate) hostname: Option<String>,
    pub(crate) sources: BTreeSet<DiscoverySource>,
    /// The first adopted node that knows it.
    pub(crate) seen_by: NodeId,
    /// The batman-adv originator that announces it, or that it is.
    pub(crate) originator: Option<MacAddr>,
}

impl Sighting {
    /// Named as OpenWrt names itself by default, or as uniwrt's images name nodes.
    pub(crate) fn openwrt_name(&self) -> bool {
        self.hostname
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case("openwrt") || name.starts_with("uniwrt-"))
    }
}

/// What the adopted nodes see, by MAC address. `adopted` pairs each node with its latest facts.
pub(crate) fn sightings(adopted: &[(Node, Option<Arc<NodeFacts>>)]) -> Vec<Sighting> {
    let owned = Owned::of(adopted);
    let mut seen = Seen::default();
    for (node, facts) in adopted {
        let Some(facts) = facts.as_deref() else {
            continue;
        };
        for lease in &facts.leases {
            if let Some(mac) = lease.mac {
                let sighting = seen.add(mac, node.id, DiscoverySource::Lease, Some(lease.ip));
                let hostname = lease.hostname.as_deref().filter(|name| !name.is_empty());
                if sighting.hostname.is_none() {
                    sighting.hostname = hostname.map(str::to_owned);
                }
            }
        }
        let wan: BTreeSet<&str> = facts
            .interfaces
            .iter()
            .filter(|interface| wan_like(interface))
            .filter_map(|interface| interface.device.as_deref())
            .collect();
        for neighbor in &facts.neighbors {
            if neighbor.ip.is_ipv4() && !neighbor.router && !wan.contains(neighbor.device.as_str())
            {
                seen.add(
                    neighbor.mac,
                    node.id,
                    DiscoverySource::Neighbor,
                    Some(neighbor.ip),
                );
            }
        }
        for mesh in &facts.batman {
            let originators: BTreeSet<MacAddr> = mesh
                .originators
                .iter()
                .map(|originator| originator.mac)
                .filter(|mac| !owned.macs.contains(mac))
                .collect();
            for originator in originators {
                // Wired devices behind it, the node's own bridge among them; wireless clients
                // cannot be the node.
                let wired: Vec<MacAddr> = mesh
                    .remote_clients
                    .iter()
                    .filter(|client| client.originator == originator && !client.wifi)
                    .map(|client| client.mac)
                    .collect();
                // It announces an adopted node's device: it is that node, facts or not yet.
                if wired.iter().any(|mac| owned.macs.contains(mac)) {
                    continue;
                }
                if wired.is_empty() {
                    seen.add(originator, node.id, DiscoverySource::Mesh, None)
                        .originator = Some(originator);
                }
                for mac in wired {
                    seen.add(mac, node.id, DiscoverySource::Mesh, None)
                        .originator = Some(originator);
                }
            }
        }
    }
    seen.0
        .into_values()
        .filter(|sighting| {
            !owned.macs.contains(&sighting.mac)
                && sighting.ip.is_none_or(|ip| !owned.ips.contains(&ip))
        })
        .collect()
}

#[derive(Default)]
struct Seen(BTreeMap<MacAddr, Sighting>);

impl Seen {
    fn add(
        &mut self,
        mac: MacAddr,
        by: NodeId,
        source: DiscoverySource,
        ip: Option<IpAddr>,
    ) -> &mut Sighting {
        let sighting = self.0.entry(mac).or_insert_with(|| Sighting {
            mac,
            ip: None,
            hostname: None,
            sources: BTreeSet::new(),
            seen_by: by,
            originator: None,
        });
        sighting.sources.insert(source);
        if let Some(new) = ip.and_then(rank)
            && sighting.ip.and_then(rank).is_none_or(|best| new < best)
        {
            sighting.ip = ip;
        }
        sighting
    }
}

/// How good an address is to reach a node by, lower first; `None` for one it cannot be reached
/// by (a link-local IPv6 address needs a scope).
fn rank(ip: IpAddr) -> Option<u8> {
    if ip.is_unspecified() || ip.is_loopback() || ip.is_multicast() {
        return None;
    }
    match ip {
        IpAddr::V4(ip) if ip.is_broadcast() => None,
        IpAddr::V4(_) => Some(0),
        IpAddr::V6(ip) if ip.is_unicast_link_local() => None,
        IpAddr::V6(_) => Some(1),
    }
}

/// The MAC and IP addresses of the adopted nodes.
struct Owned {
    macs: BTreeSet<MacAddr>,
    ips: BTreeSet<IpAddr>,
}

impl Owned {
    fn of(adopted: &[(Node, Option<Arc<NodeFacts>>)]) -> Self {
        let mut owned = Self {
            macs: BTreeSet::new(),
            ips: BTreeSet::new(),
        };
        for (node, facts) in adopted {
            owned.macs.insert(node.mac);
            if let Ok(ip) = node.host.parse() {
                owned.ips.insert(ip);
            }
            let Some(facts) = facts.as_deref() else {
                continue;
            };
            owned
                .macs
                .extend(facts.devices.iter().filter_map(|device| device.mac));
            for interface in &facts.interfaces {
                owned
                    .ips
                    .extend(interface.ipv4.iter().map(|net| IpAddr::V4(net.addr())));
                owned
                    .ips
                    .extend(interface.ipv6.iter().map(|net| IpAddr::V6(net.addr())));
            }
            for mesh in &facts.batman {
                owned.macs.extend(mesh.mac);
                owned
                    .macs
                    .extend(mesh.hard_ifs.iter().filter_map(|hard_if| hard_if.mac));
            }
        }
        owned
    }
}
