//! The `https-dns-proxy` package (features/network.md, DHCP & local DNS; D-132): DNS over
//! HTTPS on the gateway, one local instance per resolver on `127.0.0.1`, from port
//! [`FIRST_PORT`] up, which the gateway's dnsmasq forwards to ([`servers`]). The proxy leaves
//! dnsmasq and the firewall alone (`dnsmasq_config_update '-'`, `force_dns '0'`): render owns
//! both. With DoH off on a gateway that holds the package, it renders its settings without
//! instances, the service off.

use std::net::IpAddr;

use uniwrt_model::site::services::{DohResolver, Upstream};
use uniwrt_uci::{Package, Section};

use crate::{Requirement, plan::Plan};

/// The package DoH needs, with its init script.
pub(crate) const REQUIRES: Requirement = Requirement {
    package: "https-dns-proxy",
    service: Some("https-dns-proxy"),
};

/// The first instance's port; the next ones follow.
pub(crate) const FIRST_PORT: u16 = 5053;

/// The resolvers, when the site's upstream is DoH.
pub(crate) fn resolvers<'a>(plan: &Plan<'a>) -> Option<&'a [DohResolver]> {
    match &plan.site.services.upstream {
        Upstream::Doh { resolvers } => Some(resolvers),
        _ => None,
    }
}

/// The local servers dnsmasq forwards to: `127.0.0.1#<port>` for each resolver.
pub(crate) fn servers(resolvers: &[DohResolver]) -> impl Iterator<Item = String> {
    (FIRST_PORT..)
        .take(resolvers.len())
        .map(|port| format!("127.0.0.1#{port}"))
}

/// The plain server the gateway resolves its NTP servers' names through, since DoH's TLS needs
/// the right time first: the first IPv4 bootstrap server, else the first.
pub(crate) fn bootstrap(resolvers: &[DohResolver]) -> Option<IpAddr> {
    let first = resolvers.iter().flat_map(|r| &r.bootstrap);
    first
        .clone()
        .find(|ip| ip.is_ipv4())
        .or_else(|| first.clone().next())
        .copied()
}

/// The package, or `None` where there is nothing to own: not the gateway, or DoH off and no
/// package on the node.
pub(crate) fn render(plan: &Plan<'_>, held: bool) -> Option<Package> {
    plan.wan.as_ref()?;
    let resolvers = resolvers(plan);
    if resolvers.is_none() && !held {
        return None;
    }
    let mut sections = vec![
        Section::named("main", "config")
            .with("dnsmasq_config_update", "-")
            .with("force_dns", "0")
            .with("canary_domains_icloud", "0")
            .with("canary_domains_mozilla", "0")
            .with("heartbeat_domain", "-")
            .with("procd_trigger_wan6", "0")
            .with("user", "nobody")
            .with("group", "nogroup")
            .with("listen_addr", "127.0.0.1"),
    ];
    for (resolver, port) in resolvers.unwrap_or_default().iter().zip(FIRST_PORT..) {
        let bootstrap: Vec<String> = resolver.bootstrap.iter().map(ToString::to_string).collect();
        let mut instance = Section::anonymous("https-dns-proxy");
        if !bootstrap.is_empty() {
            instance = instance.with("bootstrap_dns", bootstrap.join(","));
        }
        sections.push(
            instance
                .with("resolver_url", resolver.url.as_str())
                .with("listen_addr", "127.0.0.1")
                .with("listen_port", port.to_string()),
        );
    }
    let mut package = Package::new("https-dns-proxy");
    package.sections = sections;
    Some(package)
}
