//! The WAN, the firewall policy and services.

use std::{
    collections::BTreeSet,
    net::{IpAddr, Ipv4Addr},
};

use uniwrt_core::Ipv4Net;

use super::{Issues, names};
use crate::site::{
    SiteConfig,
    gateway::{
        BackupLink, BackupProto, DhcpClient, DnsRecord, Failover, Sqm, WanConfig, WanIpv6, WanProto,
    },
};

pub(super) fn wan(config: &SiteConfig, issues: &mut Issues) {
    let wan = &config.wan;
    if let Some(vlan) = wan.vlan
        && !(1..=4094).contains(&vlan)
    {
        issues.error("wan/vlan", "A VLAN ID is 1 to 4094");
    }
    if let Some(pcp) = wan.pcp {
        if pcp > 7 {
            issues.error("wan/pcp", "An 802.1p priority is 0 to 7");
        } else if wan.vlan.is_none() {
            issues.error("wan/pcp", "A priority needs the ISP's VLAN to carry it");
        }
    }
    if let Some(mtu) = wan.mtu {
        let least = if wan.ipv6.is_on() { 1280 } else { 576 };
        if !(least..=9000).contains(&mtu) {
            issues.error("wan/mtu", format!("The MTU is {least} to 9000"));
        }
    }
    proto(wan, issues);
    ipv6(wan, issues);
    if let Some(sqm) = &wan.sqm {
        self::sqm(sqm, issues);
    }
    if let Some(failover) = &wan.failover {
        self::failover(failover, wan, issues);
    }
}

/// How IPv4 arrives.
fn proto(wan: &WanConfig, issues: &mut Issues) {
    match &wan.proto {
        WanProto::Dhcp(client) => dhcp(client, issues),
        WanProto::Static {
            address,
            gateway,
            dns,
        } => static_ipv4("wan/proto", *address, *gateway, dns, issues),
        WanProto::Pppoe {
            username,
            password,
            service,
        } => {
            if username.is_empty() {
                issues.error("wan/proto/username", "PPPoE needs the ISP's username");
            } else if !text(username) {
                issues.error("wan/proto/username", PRINTABLE);
            }
            if password.is_none() {
                issues.error("wan/password", "PPPoE needs the ISP's password");
            }
            if service.as_deref().is_some_and(|s| !text(s)) {
                issues.error("wan/proto/service", PRINTABLE);
            }
        }
        WanProto::DsLite { aftr } => {
            if !matches!(wan.ipv6, WanIpv6::Dhcpv6 { .. }) {
                issues.error(
                    "wan/ipv6",
                    "DS-Lite runs over DHCPv6, which also announces the AFTR",
                );
            }
            if aftr
                .as_deref()
                .is_some_and(|a| a.is_empty() || !a.bytes().all(|b| b.is_ascii_graphic()))
            {
                issues.error("wan/proto/aftr", "The AFTR is a host name or an address");
            }
        }
        WanProto::None => {
            if !matches!(wan.ipv6, WanIpv6::Dhcpv6 { .. }) {
                issues.error("wan/ipv6", "Without IPv4, the WAN gets IPv6 by DHCPv6");
            }
        }
    }
}

/// How IPv6 arrives.
fn ipv6(wan: &WanConfig, issues: &mut Issues) {
    match &wan.ipv6 {
        WanIpv6::Off => {}
        WanIpv6::Dhcpv6 { reqprefix } => {
            if reqprefix.is_some_and(|len| !(48..=64).contains(&len)) {
                issues.error("wan/ipv6/reqprefix", "A requested prefix is /48 to /64");
            }
        }
        WanIpv6::SixRd {
            prefix,
            border_relay,
            ipv4_mask_len,
        } => {
            if matches!(wan.proto, WanProto::DsLite { .. } | WanProto::None) {
                issues.error("wan/ipv6", "6rd runs over the WAN's IPv4");
            }
            if *ipv4_mask_len > 32 {
                issues.error("wan/ipv6/ipv4_mask_len", "The IPv4 mask is 0 to 32 bits");
            } else if u32::from(prefix.prefix_len()) + 32 - u32::from(*ipv4_mask_len) > 64 {
                issues.error(
                    "wan/ipv6/prefix",
                    "The 6rd prefix and the IPv4 bits it embeds exceed 64 bits",
                );
            }
            if border_relay.is_unspecified()
                || border_relay.is_broadcast()
                || border_relay.is_multicast()
                || border_relay.is_loopback()
            {
                issues.error(
                    "wan/ipv6/border_relay",
                    "The border relay is a unicast address",
                );
            }
        }
    }
}

const PRINTABLE: &str = "Use printable characters only";

/// A non-empty value without control characters.
/// A static IPv4 address, its gateway and DNS servers, under `at`.
fn static_ipv4(at: &str, address: Ipv4Net, gateway: Ipv4Addr, dns: &[IpAddr], issues: &mut Issues) {
    let subnet = address.trunc();
    if address.prefix_len() > 30
        || address.addr() == subnet.network()
        || address.addr() == subnet.broadcast()
    {
        issues.error(
            format!("{at}/address"),
            "The address is a host of its subnet",
        );
    }
    if !subnet.contains(&gateway) || gateway == address.addr() {
        issues.error(
            format!("{at}/gateway"),
            format!("The ISP's gateway is another host of {subnet}"),
        );
    }
    if dns.is_empty() {
        issues.warn(
            format!("{at}/dns"),
            "Without DNS servers, names do not resolve",
        );
    }
}

/// The fastest line SQM shapes: 10 Gbit/s, far past what a router's CPU shapes.
const SQM_MAX_KBPS: u32 = 10_000_000;

fn sqm(sqm: &Sqm, issues: &mut Issues) {
    for (field, rate) in [
        ("download_kbps", sqm.download_kbps),
        ("upload_kbps", sqm.upload_kbps),
    ] {
        if !(64..=SQM_MAX_KBPS).contains(&rate) {
            issues.error(
                format!("wan/sqm/{field}"),
                "A shaped rate is 64 kbit/s to 10 Gbit/s",
            );
        }
    }
    if sqm.overhead.is_some_and(|overhead| overhead > 128) {
        issues.error("wan/sqm/overhead", "The overhead is 0 to 128 bytes");
    }
}

fn failover(failover: &Failover, wan: &WanConfig, issues: &mut Issues) {
    let device = match &failover.link {
        BackupLink::Port { port, vlan } => {
            if vlan.is_some_and(|vid| !(1..=4094).contains(&vid)) {
                issues.error("wan/failover/link/vlan", "A VLAN ID is 1 to 4094");
            }
            port
        }
        BackupLink::Usb { device } => device,
    };
    if !device_name(device) {
        issues.error(
            "wan/failover/link",
            "A device name is 1 to 15 letters, digits, '-', '_' or '.'",
        );
    }
    if let BackupProto::Static {
        address,
        gateway,
        dns,
    } = &failover.proto
    {
        static_ipv4("wan/failover/proto", *address, *gateway, dns, issues);
    }
    if matches!(wan.proto, WanProto::None) {
        issues.error(
            "wan/failover",
            "Failover backs up the WAN's IPv4, which an IPv6-only WAN lacks",
        );
    }
}

/// A Linux interface name netifd accepts.
fn device_name(name: &str) -> bool {
    (1..=15).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn text(value: &str) -> bool {
    !value.is_empty() && !value.chars().any(char::is_control)
}

fn dhcp(client: &DhcpClient, issues: &mut Issues) {
    for (field, value) in [
        ("vendor_class", &client.vendor_class),
        ("hostname", &client.hostname),
    ] {
        if value.as_deref().is_some_and(|v| !text(v)) {
            issues.error(format!("wan/proto/{field}"), PRINTABLE);
        }
    }
    if let Some(id) = &client.client_id
        && (id.is_empty() || id.len() % 2 != 0 || !id.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        issues.error(
            "wan/proto/client_id",
            "The client ID is hexadecimal, two digits a byte",
        );
    }
    for (i, option) in client.send_options.iter().enumerate() {
        let valid = option.split_once(':').is_some_and(|(code, value)| {
            code.parse::<u8>().is_ok_and(|c| (1..=254).contains(&c))
                && !value.is_empty()
                && value.bytes().all(|b| b.is_ascii_graphic())
        });
        if !valid {
            issues.error(
                format!("wan/proto/send_options/{i}"),
                "An option is <code>:<value>, its code 1 to 254, without spaces",
            );
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
