//! The WAN, the firewall policy and services.

use std::collections::BTreeSet;

use super::{Issues, names};
use crate::site::{
    SiteConfig,
    gateway::{DnsRecord, WanProto},
};

pub(super) fn wan(config: &SiteConfig, issues: &mut Issues) {
    let wan = &config.wan;
    if let Some(vlan) = wan.vlan
        && !(1..=4094).contains(&vlan)
    {
        issues.error("wan/vlan", "A VLAN ID is 1 to 4094");
    }
    match &wan.proto {
        WanProto::Dhcp => {}
        WanProto::Static {
            address,
            gateway,
            dns,
        } => {
            let subnet = address.trunc();
            if address.prefix_len() > 30
                || address.addr() == subnet.network()
                || address.addr() == subnet.broadcast()
            {
                issues.error("wan/proto/address", "The address is a host of its subnet");
            }
            if !subnet.contains(gateway) || gateway == &address.addr() {
                issues.error(
                    "wan/proto/gateway",
                    format!("The ISP's gateway is another host of {subnet}"),
                );
            }
            if dns.is_empty() {
                issues.warn("wan/proto/dns", "Without DNS servers, names do not resolve");
            }
        }
        WanProto::Pppoe { username, password } => {
            if username.is_empty() {
                issues.error("wan/proto/username", "PPPoE needs the ISP's username");
            }
            if password.is_none() {
                issues.error("wan/password", "PPPoE needs the ISP's password");
            }
        }
    }
}

pub(super) fn firewall(config: &SiteConfig, issues: &mut Issues) {
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

pub(super) fn services(config: &SiteConfig, issues: &mut Issues) {
    for (i, record) in config.services.dns_records.iter().enumerate() {
        let at = format!("services/dns_records/{i}");
        let (name, target) = match record {
            DnsRecord::Host { name, .. } => (name, None),
            DnsRecord::Cname { name, target } => (name, Some(target)),
        };
        if !names::is_dns_name(name) {
            issues.error(format!("{at}/name"), format!("{name:?} is not a DNS name"));
        }
        if let Some(target) = target
            && !names::is_dns_name(target)
        {
            issues.error(
                format!("{at}/target"),
                format!("{target:?} is not a DNS name"),
            );
        }
    }
}
