//! The address plan and the networks: ids, VLANs, subnets, DHCP pools and reservations.

use std::{
    collections::{BTreeMap, BTreeSet},
    net::Ipv4Addr,
};

use uniwrt_core::Ipv4Net;

use super::{Issues, names};
use crate::site::{
    SiteConfig,
    gateway::WanProto,
    network::{Ipv4Config, Ipv6Mode, Network, Purpose},
};

const CGNAT: Ipv4Net = Ipv4Net::new_assert(Ipv4Addr::new(100, 64, 0, 0), 10);
const FACTORY: Ipv4Net = Ipv4Net::new_assert(Ipv4Addr::new(192, 168, 1, 0), 24);

pub(super) fn addressing(config: &SiteConfig, issues: &mut Issues) {
    let plan = &config.addressing;
    if !plan.ipv4.addr().is_private() || plan.ipv4.addr() != plan.ipv4.network() {
        issues.error(
            "addressing/ipv4",
            "The address space is a private (RFC 1918) prefix, such as 10.0.0.0/16",
        );
    }
    if let Some(ula) = plan.ula
        && (ula.prefix_len() != 48
            || ula.addr().segments()[0] & 0xff00 != 0xfd00
            || ula.addr() != ula.network())
    {
        issues.error("addressing/ula", "The ULA is an fdxx:xxxx:xxxx::/48 prefix");
    }
}

/// Network ids that render's own UCI interfaces and firewall zones use, besides the `uw_`
/// prefix.
pub(super) const RESERVED_IDS: [&str; 6] = ["loopback", "wan", "wan6", "wan_6", "wanb", "bat0"];

pub(super) fn networks(config: &SiteConfig, issues: &mut Issues) {
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut vlans = BTreeMap::new();
    let mut subnets: Vec<(&str, Ipv4Net)> = Vec::new();
    let defaults = config
        .networks
        .iter()
        .filter(|network| network.purpose == Purpose::Default)
        .count();
    if defaults != 1 {
        issues.error(
            "networks",
            "The site has exactly one network with the default purpose",
        );
    }
    for network in &config.networks {
        let at = format!("networks/{}", network.id);
        if !names::is_id(&network.id) {
            issues.error(
                &at,
                format!(
                    "Network id {:?} must be a lowercase letter, then up to 11 lowercase \
                     letters, digits or underscores",
                    network.id
                ),
            );
        }
        if RESERVED_IDS.contains(&network.id.as_str()) || network.id.starts_with("uw_") {
            issues.error(
                &at,
                format!(
                    "The network id {} is reserved for uniwrt's own interfaces",
                    network.id
                ),
            );
        }
        if !ids.insert(network.id.as_str()) {
            issues.error(&at, format!("Two networks have the id {}", network.id));
        }
        let name = network.name.trim();
        if name.is_empty() || name.chars().count() > 32 {
            issues.error(
                format!("{at}/name"),
                "A network needs a name of up to 32 characters",
            );
        } else if !names.insert(name.to_lowercase()) {
            issues.error(
                format!("{at}/name"),
                format!("Two networks are named {name}"),
            );
        }
        if let Some(vlan) = network.vlan
            && !(1..=4094).contains(&vlan)
        {
            issues.error(format!("{at}/vlan"), "A VLAN ID is 1 to 4094");
        }
        if let Some(other) = vlans.insert(network.vlan, network.id.as_str()) {
            let what = network
                .vlan
                .map_or_else(|| "untagged".to_owned(), |vlan| format!("on VLAN {vlan}"));
            issues.error(
                format!("{at}/vlan"),
                format!("Network {other} is {what} too"),
            );
        }
        if let Some(ipv4) = &network.ipv4 {
            ipv4_config(config, &at, ipv4, issues);
            let subnet = ipv4.address.trunc();
            if let Some((other, _)) = subnets.iter().find(|(_, other)| overlap(*other, subnet)) {
                issues.error(
                    format!("{at}/ipv4/address"),
                    format!("Its subnet {subnet} overlaps network {other}'s"),
                );
            }
            subnets.push((network.id.as_str(), subnet));
        }
        ipv6(config, network, &at, issues);
    }
}

fn ipv4_config(config: &SiteConfig, at: &str, ipv4: &Ipv4Config, issues: &mut Issues) {
    let path = format!("{at}/ipv4/address");
    let subnet = ipv4.address.trunc();
    let gateway = ipv4.address.addr();
    if !(8..=30).contains(&ipv4.address.prefix_len()) {
        issues.error(&path, "A network's prefix is /8 to /30");
        return;
    }
    if !gateway.is_private() {
        issues.error(&path, "A network uses private (RFC 1918) addresses");
    }
    if gateway == subnet.network() || gateway == subnet.broadcast() {
        issues.error(
            &path,
            format!(
                "The gateway's address is a host of {subnet}, such as {}",
                host(subnet, 1)
            ),
        );
    }
    if overlap(subnet, CGNAT) {
        issues.error(
            &path,
            "The subnet overlaps CGNAT space (100.64.0.0/10) that ISPs use",
        );
    }
    if let WanProto::Static { address, .. } = &config.wan.proto
        && overlap(subnet, address.trunc())
    {
        issues.error(&path, "The subnet overlaps the WAN's");
    }
    if overlap(subnet, FACTORY) {
        issues.warn(
            &path,
            "Keep 192.168.1.0/24 free: factory-default OpenWrt devices use it until adopted",
        );
    }
    if !config.addressing.ipv4.contains(&subnet) {
        issues.warn(
            &path,
            format!(
                "The subnet is outside the site's space {}",
                config.addressing.ipv4
            ),
        );
    }
    let Some(dhcp) = &ipv4.dhcp else { return };
    let inside =
        |ip: Ipv4Addr| subnet.contains(&ip) && ip != subnet.network() && ip != subnet.broadcast();
    for (field, ip) in [("start", dhcp.start), ("end", dhcp.end)] {
        if !inside(ip) {
            issues.error(
                format!("{at}/ipv4/dhcp/{field}"),
                format!("The pool's {field} {ip} is not a host of {subnet}"),
            );
        }
    }
    if dhcp.start > dhcp.end {
        issues.error(
            format!("{at}/ipv4/dhcp/end"),
            "The pool ends before it starts",
        );
    } else if (dhcp.start..=dhcp.end).contains(&gateway) {
        issues.error(
            format!("{at}/ipv4/dhcp/start"),
            format!("The pool holds the gateway's address {gateway}"),
        );
    }
    if dhcp.lease_time_s < 120 {
        issues.error(
            format!("{at}/ipv4/dhcp/lease_time_s"),
            "Leases last 2 minutes at least",
        );
    }
    let (mut macs, mut ips) = (BTreeSet::new(), BTreeSet::new());
    for reservation in &dhcp.reservations {
        let path = format!("{at}/ipv4/dhcp/reservations/{}", reservation.mac);
        if !macs.insert(reservation.mac) {
            issues.error(&path, format!("{} has two reservations", reservation.mac));
        }
        if !inside(reservation.ip) || reservation.ip == gateway {
            issues.error(
                format!("{path}/ip"),
                format!("{} is not a free host of {subnet}", reservation.ip),
            );
        } else if !ips.insert(reservation.ip) {
            issues.error(
                format!("{path}/ip"),
                format!("{} is reserved twice", reservation.ip),
            );
        }
        if let Some(name) = &reservation.name
            && !names::is_hostname(name)
        {
            issues.error(
                format!("{path}/name"),
                format!("{name:?} is not a host name"),
            );
        }
    }
}

fn ipv6(config: &SiteConfig, network: &Network, at: &str, issues: &mut Issues) {
    let (ula, delegated) = match network.ipv6 {
        Ipv6Mode::Off => (false, false),
        Ipv6Mode::Ula => (true, false),
        Ipv6Mode::Delegated => (false, true),
        Ipv6Mode::Both => (true, true),
    };
    if ula && config.addressing.ula.is_none() {
        issues.error(
            format!("{at}/ipv6"),
            "The site has no ULA prefix to hand out",
        );
    }
    if delegated && !config.wan.ipv6.is_on() {
        issues.warn(
            format!("{at}/ipv6"),
            "IPv6 is off on the WAN, so no prefix is delegated",
        );
    }
}

fn overlap(a: Ipv4Net, b: Ipv4Net) -> bool {
    a.contains(&b.network()) || b.contains(&a.network())
}

/// The `n`th host of `subnet`.
fn host(subnet: Ipv4Net, n: u32) -> Ipv4Addr {
    Ipv4Addr::from(u32::from(subnet.network()).saturating_add(n))
}
