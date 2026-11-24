//! The gateway's network services (D-132): the upstream DNS's layers (D-146) from dnsmasq's
//! `noresolv`, `strictorder` and `server`s (https-dns-proxy's local instances and stubby's,
//! plain servers, the WAN's), whether the
//! gateway serves NTP, and the DDNS names ddns-scripts keeps. What the model can't hold stays
//! as it is: an upstream with conditional or odd servers stays the ISP's (its options raw), and
//! a DDNS section of another service, source or id stays a raw section. DNS filtering (D-133)
//! is on where the main dnsmasq reads uniwrt's conf directory, its bypassed clients those of
//! uniwrt's redirects, its statistics where that dnsmasq logs queries; its lists live on the controller, so they are the defaults.

use std::net::IpAddr;

use uniwrt_core::MacAddr;
use uniwrt_model::site::{
    SecretRef, SiteConfig,
    blocking::{Blocking, CONF_DIR},
    services::{
        DOT_PORT, Ddns, DdnsProvider, DdnsSource, DohResolver, DotResolver, EncryptedResolver,
        MAX_SERVERS, Upstream,
    },
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
    site.services.blocking = blocking(gateway);
}

/// DNS filtering as uniwrt renders it: the main dnsmasq's conf directory, and the clients its
/// redirects send to the unfiltered instance.
fn blocking(gateway: &Source<'_>) -> Blocking {
    let packages = gateway.packages;
    let confdir = format!("{CONF_DIR},*.conf");
    let main = read::first(packages, "dhcp", "dnsmasq");
    let enabled = main.is_some_and(|dnsmasq| dnsmasq.option("confdir") == Some(confdir.as_str()));
    // Query statistics are on where filtering is and the main instance logs its queries.
    let stats = !enabled || main.is_some_and(|dnsmasq| dnsmasq.option("logqueries") == Some("1"));
    let mut bypass: Vec<MacAddr> = Vec::new();
    if enabled {
        let redirects = read::sections(packages, "firewall", "redirect").filter(|s| {
            s.option("name")
                .is_some_and(|name| name.starts_with(uniwrt_render::DNS_BYPASS_PREFIX))
        });
        for mac in redirects.flat_map(|s| read::items(s, "src_mac")) {
            if let Ok(mac) = mac.parse::<MacAddr>()
                && !bypass.contains(&mac)
            {
                bypass.push(mac);
            }
        }
    }
    Blocking {
        enabled,
        bypass,
        stats,
        ..Blocking::default()
    }
}

/// Where dnsmasq forwards (D-146): the layers, where its `server`s are the model's in the
/// model's order (DoH's and stubby's local servers, then plain servers) and, with the WAN's
/// servers asked too, `strictorder` keeps that order; anything else stays the ISP's.
fn upstream(dnsmasq: &Section, gateway: &Source<'_>, ntp: &[String]) -> Upstream {
    let isp = read::flag(dnsmasq, "noresolv") != Some(true);
    let ordered = read::flag(dnsmasq, "strictorder") == Some(true);
    let servers: Vec<&str> = read::items(dnsmasq, "server")
        .into_iter()
        .filter(|server| !pinned(server, ntp))
        .collect();
    if servers.is_empty() || (isp && !ordered) {
        return Upstream::default();
    }
    layers(&servers, gateway, isp).unwrap_or_default()
}

/// A server render pins an NTP server's name to: `/<name>/<ip>`.
fn pinned(server: &str, ntp: &[String]) -> bool {
    let Some((name, ip)) = server.strip_prefix('/').and_then(|s| s.split_once('/')) else {
        return false;
    };
    ntp.iter().any(|n| n == name) && ip.parse::<IpAddr>().is_ok()
}

/// The layers of `servers`: local proxies first (each DoH instance by its port, stubby by its
/// own), then plain servers.
fn layers(servers: &[&str], gateway: &Source<'_>, isp: bool) -> Option<Upstream> {
    let instances: Vec<&Section> =
        read::sections(gateway.packages, "https-dns-proxy", "https-dns-proxy").collect();
    let mut encrypted = Vec::new();
    let mut plain = Vec::new();
    let mut dot = false;
    for server in servers {
        if let Ok(ip) = server.parse::<IpAddr>() {
            plain.push(ip);
            continue;
        }
        // A proxy after a plain server is an order the model can't hold.
        if !plain.is_empty() {
            return None;
        }
        let port = server.strip_prefix("127.0.0.1#")?;
        if let Some(resolvers) = stubby(gateway, port) {
            if dot {
                return None;
            }
            dot = true;
            encrypted.extend(resolvers.into_iter().map(EncryptedResolver::Dot));
            continue;
        }
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
        encrypted.push(EncryptedResolver::Doh(DohResolver {
            url: instance.option("resolver_url")?.to_owned(),
            bootstrap: bootstrap?,
        }));
    }
    if encrypted.len() > MAX_SERVERS || plain.len() > MAX_SERVERS {
        return None;
    }
    Some(Upstream {
        strict: !encrypted.is_empty() && plain.is_empty() && !isp,
        encrypted,
        servers: plain,
        isp,
    })
}

/// stubby's resolvers, where it listens on `127.0.0.1@<port>` (its default 5453): resolver
/// sections in a row with the same name, port and pins are one resolver with their addresses.
fn stubby(gateway: &Source<'_>, port: &str) -> Option<Vec<DotResolver>> {
    let global = read::first(gateway.packages, "stubby", "stubby")?;
    let listens = match global.get("listen_address") {
        Some(listen) => listen
            .items()
            .iter()
            .any(|a| *a == format!("127.0.0.1@{port}")),
        None => port == "5453",
    };
    if !listens {
        return None;
    }
    let mut resolvers: Vec<DotResolver> = Vec::new();
    for section in read::sections(gateway.packages, "stubby", "resolver") {
        let address: IpAddr = section.option("address")?.parse().ok()?;
        let name = section.option("tls_auth_name")?.to_ascii_lowercase();
        let port = section
            .option("tls_port")
            .map_or(Some(DOT_PORT), |p| p.parse().ok())?;
        let spki: Option<Vec<String>> = section
            .get("spki")
            .map_or(&[][..], |v| v.items())
            .iter()
            .map(|pin| pin.strip_prefix("sha256/").map(str::to_owned))
            .collect();
        let spki = spki?;
        match resolvers.last_mut() {
            Some(last) if last.name == name && last.port == port && last.spki == spki => {
                last.addresses.push(address);
            }
            _ => resolvers.push(DotResolver {
                name,
                addresses: vec![address],
                port,
                spki,
            }),
        }
    }
    (!resolvers.is_empty()).then_some(resolvers)
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
