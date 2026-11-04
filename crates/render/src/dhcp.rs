//! The `dhcp` package (features/network.md, DHCP & local DNS): on the gateway dnsmasq serves
//! each network's pool and reservations and the local DNS records, and odhcpd advertises IPv6
//! (SLAAC, with stateless DHCPv6); on APs every network is ignored, as their DHCP services are
//! off.

use uniwrt_model::{
    node::NodeRole,
    site::{gateway::DnsRecord, network::Ipv6Mode},
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
                .with("dhcpv4", "server")
        }
        None => section.with("ignore", "1"),
    };
    if network.ipv6 == Ipv6Mode::Off {
        return section;
    }
    section
        .with("dhcpv6", "server")
        .with("ra", "server")
        .with("ra_slaac", "1")
        .list("ra_flags", ["other-config"])
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
