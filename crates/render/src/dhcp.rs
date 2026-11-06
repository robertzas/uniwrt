//! The `dhcp` package (features/network.md, DHCP & local DNS): on the gateway dnsmasq serves
//! each network's pool and reservations and the local DNS records, and odhcpd advertises IPv6
//! as each network's clients mode says; on APs every network is ignored, as their DHCP services
//! are off. The gateway's dnsmasq resolves `uniwrt.<domain>` to the controller when it sits on
//! one of the networks.

use std::net::IpAddr;

use uniwrt_model::{
    node::NodeRole,
    site::{
        gateway::{CONTROLLER_HOST, DnsRecord},
        network::{DhcpOptions, Ipv6Clients},
    },
};
use uniwrt_uci::{Package, Section};

use crate::{
    ext::SectionExt,
    plan::{Net, Plan},
};

pub(crate) fn render(plan: &Plan<'_>) -> Package {
    let domain = plan.site.site.domain.as_str();
    let gateway = plan.role == NodeRole::Gateway;
    let mut sections = vec![dnsmasq(domain)];
    for net in &plan.nets {
        sections.push(if gateway {
            pool(net)
        } else {
            ignored(net.id())
        });
    }
    if gateway {
        sections.extend(plan.nets.iter().filter_map(boot));
    }
    if gateway {
        sections.push(ignored("wan"));
    }
    sections.push(
        Section::named("odhcpd", "odhcpd")
            .with("maindhcp", "0")
            .with("leasetrigger", "/usr/sbin/odhcpd-update")
            .with("loglevel", "4"),
    );
    if gateway {
        for net in &plan.nets {
            let reservations = net
                .network
                .ipv4
                .as_ref()
                .and_then(|ipv4| ipv4.dhcp.as_ref())
                .map_or(&[][..], |dhcp| &dhcp.reservations);
            sections.extend(reservations.iter().map(|r| {
                Section::anonymous("host")
                    .maybe("name", r.name.as_deref())
                    .with("mac", r.mac.to_string())
                    .with("ip", r.ip.to_string())
            }));
        }
        if let Some(ip) = controller(plan) {
            sections.push(
                Section::anonymous("domain")
                    .with("name", CONTROLLER_HOST)
                    .with("ip", ip.to_string()),
            );
        }
        sections.extend(plan.site.services.dns_records.iter().map(|record| {
            match record {
                DnsRecord::Host { name, ip } => Section::anonymous("domain")
                    .with("name", name.as_str())
                    .with("ip", ip.to_string()),
                DnsRecord::Cname { name, target } => Section::anonymous("cname")
                    .with("cname", name.as_str())
                    .with("target", target.as_str()),
            }
        }));
    }
    let mut package = Package::new("dhcp");
    package.sections = sections;
    package
}

/// dnsmasq as OpenWrt ships it, for the site's domain.
fn dnsmasq(domain: &str) -> Section {
    Section::anonymous("dnsmasq")
        .with("domainneeded", "1")
        .with("boguspriv", "1")
        .with("filterwin2k", "0")
        .with("localise_queries", "1")
        .with("rebind_protection", "1")
        .with("rebind_localhost", "1")
        .with("local", format!("/{domain}/"))
        .with("domain", domain)
        .with("expandhosts", "1")
        .with("nonegcache", "0")
        .with("cachesize", "1000")
        .with("authoritative", "1")
        .with("readethers", "1")
        .with("leasefile", "/tmp/dhcp.leases")
        .with("resolvfile", "/tmp/resolv.conf.d/resolv.conf.auto")
        .with("nonwildcard", "1")
        .with("localservice", "1")
        .with("ednspacket_max", "1232")
}

fn ignored(interface: &str) -> Section {
    Section::named("dhcp", interface)
        .with("interface", interface)
        .with("ignore", "1")
}

fn pool(net: &Net<'_>) -> Section {
    let network = net.network;
    let section = Section::named("dhcp", net.id()).with("interface", net.id());
    let dhcp = network
        .ipv4
        .as_ref()
        .and_then(|ipv4| Some((ipv4.address, ipv4.dhcp.as_ref()?)));
    let section = match dhcp {
        Some((address, dhcp)) => {
            let base = u32::from(address.network());
            let start = u32::from(dhcp.start).saturating_sub(base);
            let limit = (u32::from(dhcp.end).saturating_sub(u32::from(dhcp.start))) + 1;
            section
                .with("start", start.to_string())
                .with("limit", limit.to_string())
                .with("leasetime", lease_time(dhcp.lease_time_s))
                // The gateway is the site's DHCP server: without `force`, dnsmasq's init
                // refuses to serve a segment where another server answers, such as an AP that
                // still runs its stock one (found by the system harness, D-068).
                .with("force", "1")
                .with("dhcpv4", "server")
                .list("dhcp_option", options(&dhcp.options))
        }
        None => section.with("ignore", "1"),
    };
    if !network.ipv6.is_on() {
        return section;
    }
    // odhcpd: SLAAC's A flag is `ra_slaac`; the M and O flags are `managed-config` (addresses
    // from DHCPv6) and `other-config` (DNS and the rest from DHCPv6).
    let (dhcpv6, slaac, flags): (_, _, &[&str]) = match network.ipv6_clients {
        Ipv6Clients::Slaac => ("disabled", "1", &["none"]),
        Ipv6Clients::SlaacStateless => ("server", "1", &["other-config"]),
        Ipv6Clients::SlaacStateful => ("server", "1", &["managed-config", "other-config"]),
        Ipv6Clients::Stateful => ("server", "0", &["managed-config", "other-config"]),
    };
    section
        .with("dhcpv6", dhcpv6)
        .with("ra", "server")
        .with("ra_slaac", slaac)
        .list("ra_flags", flags.iter().copied())
}

/// A pool's `dhcp_option`s: DNS (6), NTP (42), the domain (15), then the custom ones.
pub(crate) fn options(options: &DhcpOptions) -> Vec<String> {
    let servers = |code: u8, list: &[std::net::Ipv4Addr]| {
        (!list.is_empty()).then(|| {
            let ips: Vec<String> = list.iter().map(ToString::to_string).collect();
            format!("{code},{}", ips.join(","))
        })
    };
    servers(6, &options.dns)
        .into_iter()
        .chain(servers(42, &options.ntp))
        .chain(options.domain.as_ref().map(|d| format!("15,{d}")))
        .chain(options.custom.iter().cloned())
        .collect()
}

/// A network's PXE boot: dnsmasq's `dhcp-boot` for the clients of its pool (tagged by the
/// network's id).
fn boot(net: &Net<'_>) -> Option<Section> {
    let ipv4 = net.network.ipv4.as_ref()?;
    let boot = ipv4.dhcp.as_ref()?.options.boot.as_ref()?;
    Some(
        Section::anonymous("boot")
            .with("filename", boot.filename.as_str())
            .maybe("servername", boot.server_name.as_deref())
            .maybe("serveraddress", boot.server.map(|ip| ip.to_string()))
            .with("networkid", net.id()),
    )
}

/// The controller's address, when it sits in one of the networks the gateway serves.
fn controller(plan: &Plan<'_>) -> Option<IpAddr> {
    let ip = plan.controller?;
    let inside = plan.nets.iter().any(|net| {
        net.network.ipv4.as_ref().is_some_and(|v4| match ip {
            IpAddr::V4(ip) => v4.address.trunc().contains(&ip),
            IpAddr::V6(_) => false,
        })
    });
    inside.then_some(ip)
}

/// A lease time as dnsmasq reads it: `12h`, `30m`, or seconds.
fn lease_time(seconds: u32) -> String {
    if seconds.is_multiple_of(3600) {
        format!("{}h", seconds / 3600)
    } else if seconds.is_multiple_of(60) {
        format!("{}m", seconds / 60)
    } else {
        seconds.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::lease_time;

    #[test]
    fn lease_times() {
        assert_eq!(lease_time(43_200), "12h");
        assert_eq!(lease_time(1_800), "30m");
        assert_eq!(lease_time(90), "90");
    }
}
