//! A pool's DHCP options: servers, the domain, network boot and custom options, which dnsmasq's
//! init script writes into its configuration line by line.

use std::net::Ipv4Addr;

use super::{Issues, names};
use crate::site::network::DhcpOptions;

/// The most servers of one kind a pool hands out.
const MAX_SERVERS: usize = 4;

pub(super) fn options(at: &str, options: &DhcpOptions, issues: &mut Issues) {
    let at = format!("{at}/ipv4/dhcp/options");
    for (field, servers) in [("dns", &options.dns), ("ntp", &options.ntp)] {
        if servers.len() > MAX_SERVERS {
            issues.error(
                format!("{at}/{field}"),
                format!("At most {MAX_SERVERS} servers"),
            );
        }
        if let Some(ip) = servers.iter().find(|ip| !usable(**ip)) {
            issues.error(
                format!("{at}/{field}"),
                format!("{ip} is not a server's address"),
            );
        }
    }
    if let Some(domain) = &options.domain
        && !names::is_dns_name(domain)
    {
        issues.error(
            format!("{at}/domain"),
            format!("{domain:?} is not a DNS name"),
        );
    }
    if let Some(boot) = &options.boot {
        if boot.filename.is_empty() || !is_value(&boot.filename) || boot.filename.contains(',') {
            issues.error(
                format!("{at}/boot/filename"),
                "The boot file is a path without spaces or commas, such as pxelinux.0",
            );
        }
        if let Some(name) = &boot.server_name
            && !names::is_dns_name(name)
        {
            issues.error(
                format!("{at}/boot/server_name"),
                format!("{name:?} is not a host name"),
            );
        }
        if boot.server.is_some() && boot.server_name.is_none() {
            // OpenWrt's dnsmasq init skips a boot section with an address but no name.
            issues.error(
                format!("{at}/boot/server_name"),
                "Name the TFTP server too: dnsmasq needs its name with its address",
            );
        }
        if let Some(ip) = boot.server
            && !usable(ip)
        {
            issues.error(
                format!("{at}/boot/server"),
                format!("{ip} is not a server's address"),
            );
        }
    }
    for (i, option) in options.custom.iter().enumerate() {
        let path = format!("{at}/custom/{i}");
        match option.split_once(',') {
            Some((code, _)) if is_dhcp_option(option) => {
                let own = [
                    "6",
                    "15",
                    "42",
                    "option:dns-server",
                    "option:domain-name",
                    "option:ntp-server",
                ];
                if own.contains(&code) {
                    issues.warn(
                        path,
                        format!("Option {code} has a field of its own, which it overrides"),
                    );
                }
            }
            _ => issues.error(
                path,
                format!(
                    "{option:?} is not a DHCP option: a code or option:<name>, a comma, then \
                     its value, such as 26,1400"
                ),
            ),
        }
    }
}

/// Whether `text` is a DHCP option dnsmasq reads and render writes as given: a code (1–254)
/// or `option:<name>`, a comma, then a value without spaces.
pub fn is_dhcp_option(text: &str) -> bool {
    text.split_once(',')
        .is_some_and(|(code, value)| is_code(code) && is_value(value))
}

/// A unicast address a client could reach.
fn usable(ip: Ipv4Addr) -> bool {
    !(ip.is_unspecified() || ip.is_broadcast() || ip.is_multicast() || ip.is_loopback())
}

/// An option's code: 1–254, or dnsmasq's `option:<name>`.
fn is_code(code: &str) -> bool {
    match code.strip_prefix("option:") {
        Some(name) => !name.is_empty() && name.bytes().all(|b| b.is_ascii_lowercase() || b == b'-'),
        None => code.parse::<u8>().is_ok_and(|n| (1..=254).contains(&n)),
    }
}

/// A value that stays on its line of dnsmasq's configuration: printable, no spaces.
fn is_value(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| b.is_ascii_graphic())
}

#[cfg(test)]
mod tests {
    use super::{is_code, is_value};

    #[test]
    fn option_codes_and_values() {
        assert!(is_code("26") && is_code("option:mtu") && is_code("option:ntp-server"));
        assert!(!is_code("0") && !is_code("255") && !is_code("option:") && !is_code("tag:x"));
        assert!(is_value("1400") && is_value("\"a\",b"));
        assert!(!is_value("") && !is_value("a b") && !is_value("a\nb"));
    }
}
