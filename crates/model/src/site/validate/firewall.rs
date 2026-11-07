//! The firewall: the policy between networks, port forwards, the DMZ host and UPnP.

use std::{collections::BTreeSet, net::Ipv4Addr};

use super::Issues;
use crate::site::{
    SiteConfig,
    firewall::{DMZ, PortForward},
    gateway::WanProto,
};

/// The longest forward name.
const NAME_MAX: usize = 48;

pub(super) fn firewall(config: &SiteConfig, issues: &mut Issues) {
    policies(config, issues);
    forwards(config, issues);
    if let Some(host) = config.firewall.dmz
        && let Err(why) = host_on_a_network(config, host)
    {
        issues.error("firewall/dmz", why);
    }
    upnp(config, issues);
    let exposed =
        config.firewall.forwards.iter().any(|f| f.enabled) || config.firewall.dmz.is_some();
    if exposed {
        let why = match config.wan.proto {
            WanProto::DsLite { .. } => Some("DS-Lite shares the ISP's IPv4 address"),
            WanProto::None => Some("The WAN has no IPv4"),
            _ => None,
        };
        if let Some(why) = why {
            issues.warn(
                "firewall/forwards",
                format!("{why}: forwarded ports can't be reached from the internet"),
            );
        }
    }
}

fn policies(config: &SiteConfig, issues: &mut Issues) {
    let known = |zone: &str| zone == "wan" || config.networks.iter().any(|n| n.id == zone);
    let mut cells = BTreeSet::new();
    for (i, policy) in config.firewall.policies.iter().enumerate() {
        let at = format!("firewall/policies/{i}");
        for (field, zone) in [("from", &policy.from), ("to", &policy.to)] {
            if !known(zone) {
                issues.error(
                    format!("{at}/{field}"),
                    format!("There is no network {zone}"),
                );
            }
        }
        if policy.from == policy.to {
            issues.error(&at, "A policy is between two different networks");
        } else if !cells.insert((&policy.from, &policy.to)) {
            issues.error(
                &at,
                format!(
                    "Traffic from {} to {} has two policies",
                    policy.from, policy.to
                ),
            );
        }
    }
}

fn forwards(config: &SiteConfig, issues: &mut Issues) {
    let forwards = &config.firewall.forwards;
    let mut names = BTreeSet::new();
    for (i, forward) in forwards.iter().enumerate() {
        let at = format!("firewall/forwards/{i}");
        let name = forward.name.trim();
        if name.is_empty() || name.len() > NAME_MAX || name.chars().any(char::is_control) {
            issues.error(
                format!("{at}/name"),
                format!("A name is 1 to {NAME_MAX} characters"),
            );
        } else if name != forward.name {
            issues.error(format!("{at}/name"), "A name has no spaces around it");
        } else if name.eq_ignore_ascii_case(DMZ) {
            issues.error(format!("{at}/name"), "The DMZ host has this name");
        } else if !names.insert(name.to_lowercase()) {
            issues.error(format!("{at}/name"), "Another forward has this name");
        }
        if forward.ports.first == 0 || forward.ports.first > forward.ports.last {
            issues.error(
                format!("{at}/ports"),
                "Ports are 1 to 65535, the first no larger than the last",
            );
        } else if forward.to_port == Some(0) || forward.to_ports().is_none() {
            issues.error(format!("{at}/to_port"), "The host's ports are 1 to 65535");
        }
        if let Err(why) = host_on_a_network(config, forward.to) {
            issues.error(format!("{at}/to"), why);
        }
        if forward.enabled
            && let Some(other) = forwards[..i]
                .iter()
                .find(|other| other.enabled && clashes(forward, other))
        {
            issues.error(
                format!("{at}/ports"),
                format!("{} already forwards these ports", other.name),
            );
        }
    }
}

/// Whether two forwards claim one of the same WAN ports for one of the same protocols.
fn clashes(a: &PortForward, b: &PortForward) -> bool {
    a.ports.overlaps(b.ports) && a.proto.names().iter().any(|p| b.proto.names().contains(p))
}

/// Whether `host` is an address a client on one of the networks can have.
fn host_on_a_network(config: &SiteConfig, host: Ipv4Addr) -> Result<(), String> {
    let ipv4 = config
        .networks
        .iter()
        .filter_map(|n| n.ipv4.as_ref())
        .find(|ipv4| ipv4.address.trunc().contains(&host))
        .ok_or_else(|| format!("{host} is on none of the networks"))?;
    let subnet = ipv4.address.trunc();
    if host == ipv4.address.addr() {
        Err(format!("{host} is the gateway's own address"))
    } else if subnet.prefix_len() < 31 && (host == subnet.network() || host == subnet.broadcast()) {
        Err(format!("{host} is not a host's address in {subnet}"))
    } else {
        Ok(())
    }
}

fn upnp(config: &SiteConfig, issues: &mut Issues) {
    let upnp = &config.firewall.upnp;
    let mut seen = BTreeSet::new();
    for (i, id) in upnp.networks.iter().enumerate() {
        let at = format!("firewall/upnp/networks/{i}");
        match config.networks.iter().find(|n| &n.id == id) {
            None => issues.error(at, format!("There is no network {id}")),
            Some(network) if network.ipv4.is_none() => issues.error(
                at,
                format!("{} has no IPv4 address on the gateway", network.name),
            ),
            Some(_) if !seen.insert(id) => issues.error(at, format!("{id} is listed twice")),
            Some(_) => {}
        }
    }
    if upnp.enabled && upnp.networks.is_empty() {
        issues.error("firewall/upnp/networks", "UPnP serves at least one network");
    }
}
