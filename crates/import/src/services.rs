//! The gateway's network services (D-132): the upstream DNS from dnsmasq's `noresolv` and
//! `server`s (the WAN's, plain servers, or https-dns-proxy's local instances), whether the
//! gateway serves NTP, and the DDNS names ddns-scripts keeps. What the model can't hold stays
//! as it is: an upstream with conditional or odd servers stays the ISP's (its options raw), and
//! a DDNS section of another service, source or id stays a raw section.

use std::net::IpAddr;

use uniwrt_model::site::{
    SecretRef, SiteConfig,
    services::{Ddns, DdnsProvider, DdnsSource, DohResolver, MAX_SERVERS, Upstream},
    validate::is_id,
};
use uniwrt_uci::Section;

use crate::{Source, read};

pub(crate) fn import(
    site: &mut SiteConfig,
    gateway: &Source<'_>,
    seal: &mut dyn FnMut(&str) -> SecretRef,
) {
    let packages = gateway.packages;
    if let Some(ntp) = packages.get("system").and_then(|p| p.section("ntp")) {
        site.services.ntp_server = read::flag(ntp, "enable_server") == Some(true);
    }
    if let Some(dnsmasq) = read::first(packages, "dhcp", "dnsmasq") {
        site.services.upstream = upstream(dnsmasq, gateway, &site.site.ntp_servers);
    }
    site.services.ddns = read::sections(packages, "ddns", "service")
        .filter_map(|section| ddns(section, seal))
        .collect();
}

/// Where dnsmasq forwards: the WAN's servers unless `noresolv` names others the model holds.
fn upstream(dnsmasq: &Section, gateway: &Source<'_>, ntp: &[String]) -> Upstream {
    if read::flag(dnsmasq, "noresolv") != Some(true) {
        return Upstream::Isp;
    }
    let servers = read::items(dnsmasq, "server");
    let plain: Option<Vec<IpAddr>> = servers.iter().map(|s| s.parse().ok()).collect();
    if let Some(servers) = plain.filter(|s| (1..=MAX_SERVERS).contains(&s.len())) {
        return Upstream::Custom { servers };
    }
    doh(&servers, gateway, ntp).unwrap_or_default()
}

/// DoH: every server is a local https-dns-proxy instance, in port order, besides the NTP names
/// render pins to a bootstrap server.
fn doh(servers: &[&str], gateway: &Source<'_>, ntp: &[String]) -> Option<Upstream> {
    let pinned = |server: &str| {
        let Some((name, ip)) = server.strip_prefix('/').and_then(|s| s.split_once('/')) else {
            return false;
        };
        ntp.iter().any(|n| n == name) && ip.parse::<IpAddr>().is_ok()
    };
    let instances: Vec<&Section> =
        read::sections(gateway.packages, "https-dns-proxy", "https-dns-proxy").collect();
    let mut resolvers = Vec::new();
    for server in servers.iter().filter(|s| !pinned(s)) {
        let port = server.strip_prefix("127.0.0.1#")?;
        let instance = instances.iter().enumerate().find_map(|(i, s)| {
            // The init script numbers instances without a port from 5053.
            let default = (5053 + i).to_string();
            (s.option("listen_port").unwrap_or(&default) == port).then_some(*s)
        })?;
        let bootstrap: Option<Vec<IpAddr>> = instance
            .option("bootstrap_dns")
            .unwrap_or_default()
            .split(',')
            .filter(|ip| !ip.trim().is_empty())
            .map(|ip| ip.trim().parse().ok())
            .collect();
        resolvers.push(DohResolver {
            url: instance.option("resolver_url")?.to_owned(),
            bootstrap: bootstrap?,
        });
    }
    (1..=MAX_SERVERS)
        .contains(&resolvers.len())
        .then_some(Upstream::Doh { resolvers })
}

/// A ddns-scripts `service` the model holds: a curated provider or an update URL, read from the
/// WAN or the web.
fn ddns(section: &Section, seal: &mut dyn FnMut(&str) -> SecretRef) -> Option<Ddns> {
    let id = section.name.as_deref().filter(|id| is_id(id))?;
    let hostname = section.option("lookup_host")?.to_ascii_lowercase();
    let domain = section.option("domain").unwrap_or_default();
    let username = section.option("username");
    let named = |name: Option<&str>| name.map(str::to_owned);
    let provider = match (section.option("service_name"), section.option("update_url")) {
        (Some("cloudflare.com-v4"), _) => {
            if username != Some("Bearer") {
                return None;
            }
            let (host, zone) = domain.split_once('@')?;
            let full = if host.is_empty() {
                zone.to_owned()
            } else {
                format!("{host}.{zone}")
            };
            if full != hostname {
                return None;
            }
            DdnsProvider::Cloudflare {
                zone: zone.to_owned(),
            }
        }
        (Some("duckdns.org"), _) => DdnsProvider::Duckdns,
        (Some("desec.io"), _) => DdnsProvider::Desec,
        (Some("no-ip.com"), _) => DdnsProvider::NoIp {
            username: named(username)?,
        },
        (Some("dynu.com"), _) => DdnsProvider::Dynu {
            username: named(username)?,
        },
        (Some("afraid.org-v2-token"), _) => DdnsProvider::Freedns,
        (None, Some(url)) => DdnsProvider::Custom {
            url: url.to_owned(),
            username: named(username),
        },
        _ => return None,
    };
    let ipv6 = read::flag(section, "use_ipv6") == Some(true);
    let source = match section.option("ip_source").unwrap_or("network") {
        "network" => DdnsSource::Wan,
        "web" => DdnsSource::Web,
        _ => return None,
    };
    Some(Ddns {
        id: id.to_owned(),
        enabled: read::flag(section, "enabled") == Some(true),
        hostname,
        provider,
        ipv6,
        source,
        secret: section
            .option("password")
            .filter(|p| !p.is_empty())
            .map(seal),
    })
}
