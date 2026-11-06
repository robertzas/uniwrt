//! DHCP and local DNS, from the gateway's `dhcp` package: each network's pool with its options
//! and network boot, how its clients get IPv6 (odhcpd), the reservations, the local records,
//! but the controller's own record (`uniwrt`), which render writes from where the controller is
//! rather than from the site ([`uniwrt_render::held_controller`]).

use std::net::Ipv4Addr;

use uniwrt_core::{Ipv4Net, MacAddr};
use uniwrt_model::site::{
    SiteConfig,
    gateway::{CONTROLLER_HOST, DnsRecord},
    network::{Boot, Dhcp, DhcpOptions, Ipv6Clients, Reservation},
    validate::is_dhcp_option,
};
use uniwrt_uci::{Package, Section};

use crate::read;

/// The `dhcp` section that serves `interface`.
fn section<'p>(dhcp: Option<&'p Package>, interface: &str) -> Option<&'p Section> {
    dhcp?.sections_of("dhcp").find(|s| {
        s.option("interface")
            .or(s.name.as_deref())
            .is_some_and(|i| i == interface)
    })
}

/// The DHCP pool dnsmasq serves on `interface` (OpenWrt's defaults: start 100, limit 150,
/// 12 h), with its options and network boot.
pub(crate) fn pool(dhcp: Option<&Package>, interface: &str, address: Ipv4Net) -> Option<Dhcp> {
    let section = section(dhcp, interface)?;
    if read::flag(section, "ignore") == Some(true) || section.option("dhcpv4") == Some("disabled") {
        return None;
    }
    let number = |name: &str, default: u32| {
        section
            .option(name)
            .map_or(Some(default), |v| v.parse().ok())
    };
    let base = u32::from(address.network());
    let start = base.checked_add(number("start", 100)?)?;
    let end = start.checked_add(number("limit", 150)?.checked_sub(1)?)?;
    let last = u32::from(address.broadcast()).saturating_sub(1);
    let mut options = options(&read::items(section, "dhcp_option"));
    options.boot = dhcp.and_then(|dhcp| boot(dhcp, interface));
    Some(Dhcp {
        start: Ipv4Addr::from(start),
        end: Ipv4Addr::from(end.min(last)),
        lease_time_s: lease_time(section.option("leasetime").unwrap_or("12h")).unwrap_or(12 * 3600),
        reservations: Vec::new(),
        options,
    })
}

/// A pool's `dhcp_option`s: the first DNS (6), NTP (42) and domain (15) options whose values
/// the model holds, and every other option it can render back as written.
fn options(items: &[&str]) -> DhcpOptions {
    let mut options = DhcpOptions::default();
    for item in items {
        let Some((code, value)) = item.split_once(',') else {
            continue;
        };
        let servers = || -> Option<Vec<Ipv4Addr>> {
            value.split(',').map(|ip| ip.trim().parse().ok()).collect()
        };
        let taken = match code {
            "6" | "option:dns-server" if options.dns.is_empty() => {
                servers().map(|ips| options.dns = ips)
            }
            "42" | "option:ntp-server" if options.ntp.is_empty() => {
                servers().map(|ips| options.ntp = ips)
            }
            "15" | "option:domain-name" if options.domain.is_none() && !value.contains(',') => {
                options.domain = Some(value.to_owned());
                Some(())
            }
            _ => None,
        };
        if taken.is_none() && is_dhcp_option(item) {
            options.custom.push((*item).to_owned());
        }
    }
    options
}

/// The network boot of the clients `interface`'s pool tags (`networkid`).
fn boot(dhcp: &Package, interface: &str) -> Option<Boot> {
    let section = dhcp
        .sections_of("boot")
        .find(|s| s.option("networkid") == Some(interface) && s.option("filename").is_some())?;
    Some(Boot {
        filename: section.option("filename")?.to_owned(),
        server_name: section.option("servername").map(str::to_owned),
        server: section
            .option("serveraddress")
            .and_then(|ip| ip.parse().ok()),
    })
}

/// How `interface`'s clients get IPv6 from odhcpd: DHCPv6 off is SLAAC only; the M flag
/// (`managed-config`) gives stateful addresses, beside SLAAC unless `ra_slaac` is off. odhcpd
/// sets only the O flag (`other-config`) when `ra_flags` is missing.
pub(crate) fn clients(dhcp: Option<&Package>, interface: &str) -> Ipv6Clients {
    let Some(section) = section(dhcp, interface) else {
        return Ipv6Clients::default();
    };
    if section.option("dhcpv6") != Some("server") {
        return Ipv6Clients::Slaac;
    }
    let managed = read::items(section, "ra_flags").contains(&"managed-config");
    match (managed, read::flag(section, "ra_slaac")) {
        (true, Some(false)) => Ipv6Clients::Stateful,
        (true, _) => Ipv6Clients::SlaacStateful,
        (false, _) => Ipv6Clients::SlaacStateless,
    }
}

/// dnsmasq's lease time: seconds, or with a unit (`m`, `h`, `d`, `w`).
fn lease_time(text: &str) -> Option<u32> {
    let (digits, unit) = match text.char_indices().last()? {
        (i, c) if c.is_ascii_alphabetic() => (&text[..i], c.to_ascii_lowercase()),
        _ => (text, 's'),
    };
    let n: u32 = digits.parse().ok()?;
    let scale = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86_400,
        'w' => 604_800,
        _ => return None,
    };
    n.checked_mul(scale)
}

/// `host` sections with one MAC address and an IPv4 address, in the pool's network.
pub(crate) fn reservations(site: &mut SiteConfig, dhcp: Option<&Package>) {
    let Some(dhcp) = dhcp else { return };
    for host in dhcp.sections_of("host") {
        let (Some(mac), Some(ip)) = (
            host.option("mac").and_then(|m| m.parse::<MacAddr>().ok()),
            host.option("ip").and_then(|ip| ip.parse::<Ipv4Addr>().ok()),
        ) else {
            continue;
        };
        let pool = site
            .networks
            .iter_mut()
            .filter_map(|n| n.ipv4.as_mut())
            .find(|v4| v4.address.contains(&ip))
            .and_then(|v4| v4.dhcp.as_mut());
        if let Some(pool) = pool {
            pool.reservations.push(Reservation {
                mac,
                ip,
                name: host.option("name").map(str::to_owned),
            });
        }
    }
}

/// The local records, but the controller's.
pub(crate) fn dns_records(dhcp: Option<&Package>) -> Vec<DnsRecord> {
    let Some(dhcp) = dhcp else {
        return Vec::new();
    };
    let hosts = dhcp.sections_of("domain").filter_map(|d| {
        let name = d.option("name")?;
        (name != CONTROLLER_HOST).then_some(())?;
        Some(DnsRecord::Host {
            name: name.to_owned(),
            ip: d.option("ip")?.parse().ok()?,
        })
    });
    let cnames = dhcp.sections_of("cname").filter_map(|c| {
        Some(DnsRecord::Cname {
            name: c.option("cname")?.to_owned(),
            target: c.option("target")?.to_owned(),
        })
    });
    hosts.chain(cnames).collect()
}

#[cfg(test)]
mod tests {
    use super::{lease_time, options};

    #[test]
    fn lease_times_read_as_dnsmasq_reads_them() {
        assert_eq!(lease_time("12h"), Some(43_200));
        assert_eq!(lease_time("30m"), Some(1_800));
        assert_eq!(lease_time("90"), Some(90));
        assert_eq!(lease_time("1d"), Some(86_400));
        assert_eq!(lease_time("infinite"), None);
    }

    #[test]
    fn options_take_their_fields_and_keep_the_rest() {
        let found = options(&[
            "option:dns-server,10.0.0.53,10.0.0.54",
            "42,10.0.0.1",
            "15,home.arpa",
            "6,10.9.9.9",
            "26,1400",
            "tag:x,3,10.0.0.2",
        ]);
        assert_eq!(found.dns.len(), 2);
        assert_eq!(found.ntp.len(), 1);
        assert_eq!(found.domain.as_deref(), Some("home.arpa"));
        // The second DNS option can't be a field: kept as written. A tagged one can't be held.
        assert_eq!(found.custom, ["6,10.9.9.9", "26,1400"]);
    }
}
