//! Who clients are (names and addresses from the DHCP leases and the neighbor caches), and the
//! last way to place one: a node that has it in its neighbor cache, off its uplink.

use std::{
    collections::{BTreeMap, BTreeSet},
    net::IpAddr,
};

use uniwrt_core::MacAddr;
use uniwrt_model::facts::DeviceKind;

use crate::{
    Medium,
    fleet::{Fleet, device, unicast, uplink_devices},
    links::{Graph, Link},
};

/// What the nodes know about an address.
#[derive(Debug, Default)]
pub(crate) struct Host {
    pub(crate) name: Option<String>,
    pub(crate) ips: BTreeSet<IpAddr>,
}

/// Names and addresses by MAC address, from every node's leases and neighbor caches; the
/// gateway's name for a client wins.
#[derive(Debug, Default)]
pub(crate) struct Directory(BTreeMap<MacAddr, Host>);

impl Directory {
    pub(crate) fn new(fleet: &Fleet<'_>) -> Self {
        let mut hosts = BTreeMap::<MacAddr, Host>::new();
        for (_, _, facts) in fleet.with_facts() {
            for lease in &facts.leases {
                let Some(mac) = lease.mac else { continue };
                let host = hosts.entry(mac).or_default();
                host.ips.insert(lease.ip);
                let name = lease
                    .hostname
                    .as_deref()
                    .filter(|name| !name.is_empty() && *name != "*");
                if host.name.is_none() {
                    host.name = name.map(str::to_owned);
                }
            }
            let uplinks = uplink_devices(facts);
            for neighbor in &facts.neighbors {
                if !uplinks.contains(&neighbor.device.as_str()) {
                    hosts
                        .entry(neighbor.mac)
                        .or_default()
                        .ips
                        .insert(neighbor.ip);
                }
            }
        }
        Self(hosts)
    }

    pub(crate) fn get(&self, mac: MacAddr) -> Option<&Host> {
        self.0.get(&mac)
    }
}

/// Places the clients no other source placed but a node's neighbor cache knows, off the
/// node's uplinks: on the device they were seen on.
pub(crate) fn place_neighbors(graph: &mut Graph, fleet: &Fleet<'_>, directory: &Directory) {
    for (member, _, facts) in fleet.with_facts() {
        let uplinks = uplink_devices(facts);
        for neighbor in &facts.neighbors {
            let mac = neighbor.mac;
            if uplinks.contains(&neighbor.device.as_str())
                || !unicast(mac)
                || fleet.owner(mac).is_some()
                || graph.is_placed(mac)
            {
                continue;
            }
            let wireless =
                device(facts, &neighbor.device).is_some_and(|dev| dev.kind == DeviceKind::Wireless);
            let medium = if wireless {
                Medium::Wifi
            } else {
                Medium::Ethernet
            };
            let client = graph.client(mac, directory);
            let mut link = Link::new(fleet.vertex(member), client, medium);
            link.ports[0] = Some(neighbor.device.clone());
            graph.link(link);
        }
    }
}
