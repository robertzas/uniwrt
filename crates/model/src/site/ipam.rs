//! The address plan's suggestions (features/network.md, Address plan; D-077): each network a
//! /24 whose third octet is its VLAN ID (10.0.30.0/24 for VLAN 30), its gateway the first host,
//! its pool .100–.249, and reservations .32–.99 ("Fix IP"). They only suggest: any subnet that
//! validates may be chosen.

use std::net::Ipv4Addr;

use uniwrt_core::Ipv4Net;

use super::{
    SiteConfig,
    network::{Dhcp, DhcpOptions, Ipv4Config, Ipv6Clients, Ipv6Mode, Network, Purpose},
};

/// The subnet factory-default OpenWrt devices use, kept free (validate warns about it).
const FACTORY: Ipv4Net = Ipv4Net::new_assert(Ipv4Addr::new(192, 168, 1, 0), 24);
/// Twelve hours, as OpenWrt ships.
const LEASE_TIME_S: u32 = 12 * 3600;

/// A new network of `purpose` for `config`, on `vlan` if given (else the purpose's usual VLAN
/// or the next free one): its id, name, subnet, pool and the policy its purpose seeds. `None`
/// when the VLANs or the address space are used up.
pub fn network(config: &SiteConfig, purpose: Purpose, vlan: Option<u16>) -> Option<Network> {
    let vlan = match vlan {
        Some(vlan) => vlan,
        None => free_vlan(config, purpose)?,
    };
    let (id, name) = naming(config, purpose)?;
    let address = subnet(config, Some(vlan))?;
    let ipv6 = match (config.addressing.ula.is_some(), purpose) {
        (_, Purpose::Guest) | (false, _) => Ipv6Mode::Delegated,
        (true, _) => Ipv6Mode::Both,
    };
    Some(Network {
        id,
        name,
        purpose,
        vlan: Some(vlan),
        ipv4: Some(Ipv4Config {
            address,
            dhcp: pool(address).map(|(start, end)| Dhcp {
                start,
                end,
                lease_time_s: LEASE_TIME_S,
                reservations: Vec::new(),
                options: DhcpOptions::default(),
            }),
        }),
        ipv6,
        ipv6_clients: Ipv6Clients::default(),
        isolation: purpose == Purpose::Guest,
        mdns: !matches!(purpose, Purpose::Guest | Purpose::Mgmt),
        igmp_snooping: false,
    })
}

/// The usual VLAN of a purpose (`iot` 30, `guest` 40, `mgmt` 99, others from 50 in tens), or
/// when taken the next free one after it. The wired backhaul's VLAN is never free (D-094).
pub fn free_vlan(config: &SiteConfig, purpose: Purpose) -> Option<u16> {
    let taken = |vid: u16| {
        vid == config.mesh.wired_vlan || config.networks.iter().any(|n| n.vlan == Some(vid))
    };
    let usual: &[u16] = match purpose {
        Purpose::Iot => &[30],
        Purpose::Guest => &[40],
        Purpose::Mgmt => &[99],
        Purpose::Default | Purpose::Custom => &[50, 60, 70, 80, 90],
    };
    usual
        .iter()
        .copied()
        .find(|&vid| !taken(vid))
        .or_else(|| (usual[0]..4095).chain(2..usual[0]).find(|&vid| !taken(vid)))
}

/// The gateway's address of a new network on `vlan` (untagged: VLAN 0): the /24 of the site's
/// space whose third octet is the VLAN, else the first free /24 in it.
pub fn subnet(config: &SiteConfig, vlan: Option<u16>) -> Option<Ipv4Net> {
    let space = config.addressing.ipv4;
    let used: Vec<Ipv4Net> = config
        .networks
        .iter()
        .filter_map(|n| n.ipv4.as_ref())
        .map(|v4| v4.address.trunc())
        .chain([FACTORY])
        .collect();
    let free = |candidate: &Ipv4Net| {
        space.contains(candidate)
            && !used
                .iter()
                .any(|u| u.contains(&candidate.network()) || candidate.contains(&u.network()))
    };
    let base = u32::from(space.network());
    let at = |third: u32| {
        Ipv4Net::new(Ipv4Addr::from(base.checked_add(third << 8)?), 24)
            .ok()
            .filter(|net| free(net))
    };
    if space.prefix_len() > 24 {
        return None;
    }
    let count = 1u32 << (24 - space.prefix_len());
    let preferred = u32::from(vlan.unwrap_or(0));
    let chosen = (preferred < count)
        .then(|| at(preferred))
        .flatten()
        .or_else(|| (0..count).find_map(at))?;
    Ipv4Net::new(host(chosen, 1), 24).ok()
}

/// A subnet's DHCP pool: .100–.249 of a /24, and in proportion for other sizes.
pub fn pool(address: Ipv4Net) -> Option<(Ipv4Addr, Ipv4Addr)> {
    let subnet = address.trunc();
    let size = 1u64 << (32 - u32::from(subnet.prefix_len()));
    let (first, last) = if size >= 256 {
        (100, size - 7)
    } else {
        ((size / 4).max(2), size - 2)
    };
    if size < 8 || first > last {
        return None;
    }
    let to = |n: u64| u32::try_from(n).ok().map(|n| host(subnet, n));
    Some((to(first)?, to(last)?))
}

/// A free address to reserve on `network` for a client: `prefer` (the address it holds now)
/// when no other reservation has it and it is a host other than the gateway's, else the first
/// free one of .32–.99 (below the pool), else any free host outside the pool.
pub fn reservation(network: &Network, prefer: Option<Ipv4Addr>) -> Option<Ipv4Addr> {
    let ipv4 = network.ipv4.as_ref()?;
    let subnet = ipv4.address.trunc();
    let gateway = ipv4.address.addr();
    let reservations = ipv4.dhcp.as_ref().map_or(&[][..], |d| &d.reservations);
    let pool = ipv4.dhcp.as_ref().map(|d| d.start..=d.end);
    let free = |ip: Ipv4Addr| {
        subnet.contains(&ip)
            && ip != subnet.network()
            && ip != subnet.broadcast()
            && ip != gateway
            && !reservations.iter().any(|r| r.ip == ip)
    };
    if let Some(ip) = prefer.filter(|ip| free(*ip)) {
        return Some(ip);
    }
    let base = u32::from(subnet.network());
    let last = u32::from(subnet.broadcast());
    let outside = |ip: &Ipv4Addr| pool.as_ref().is_none_or(|p| !p.contains(ip));
    (base + 32..=(base + 99).min(last))
        .chain(base + 2..last)
        .map(Ipv4Addr::from)
        .find(|ip| free(*ip) && outside(ip))
}

/// A new network's id and display name from its purpose (`iot`, `guest`, …), both numbered
/// when taken (`iot2`).
fn naming(config: &SiteConfig, purpose: Purpose) -> Option<(String, String)> {
    let (id, name) = match purpose {
        Purpose::Default => ("lan", "Default"),
        Purpose::Iot => ("iot", "IoT"),
        Purpose::Guest => ("guest", "Guest"),
        Purpose::Mgmt => ("mgmt", "Management"),
        Purpose::Custom => ("net", "Network"),
    };
    let taken = |candidate: &str, label: &str| {
        config
            .networks
            .iter()
            .any(|n| n.id == candidate || n.name.eq_ignore_ascii_case(label))
    };
    // A site has at most 4094 VLANs, so one of these is free.
    (1u32..=4096)
        .map(|n| match n {
            1 if purpose != Purpose::Custom => (id.to_owned(), name.to_owned()),
            1 => (format!("{id}1"), format!("{name} 1")),
            n => (format!("{id}{n}"), format!("{name} {n}")),
        })
        .find(|(id, name)| !taken(id, name))
}

/// The `n`th address of `subnet`.
fn host(subnet: Ipv4Net, n: u32) -> Ipv4Addr {
    Ipv4Addr::from(u32::from(subnet.network()).saturating_add(n))
}

#[cfg(test)]
mod tests;
