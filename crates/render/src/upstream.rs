//! Where the gateway's dnsmasq forwards what it can't answer (features/network.md, Upstream DNS;
//! D-132, D-146): the layers in order, the encrypted resolvers' local servers (each DoH
//! resolver's https-dns-proxy instance, and stubby where the first DoT resolver stands), then
//! the site's plain servers, then the WAN's (its resolv file; `noresolv` without it). With more
//! than one layer dnsmasq asks them in order (`strictorder`), so a later layer only hears a
//! query the earlier ones failed; within one layer it keeps choosing the fastest server, which
//! routes around a dead one. The NTP servers' names resolve through a plain server
//! ([`ntp_pin`]), since TLS needs the right time first.

use std::net::IpAddr;

use uniwrt_model::site::services::{EncryptedResolver, Upstream};
use uniwrt_uci::Section;

use crate::{doh, dot, ext::SectionExt as _, plan::Plan};

/// `dnsmasq` with the site's upstream.
pub(crate) fn apply(plan: &Plan<'_>, dnsmasq: Section) -> Section {
    let upstream = &plan.site.services.upstream;
    let ntp = plan
        .site
        .site
        .ntp_servers
        .iter()
        .filter(|name| name.parse::<IpAddr>().is_err());
    let pinned = ntp_pin(upstream)
        .into_iter()
        .flat_map(|ip| ntp.clone().map(move |name| format!("/{name}/{ip}")));
    dnsmasq
        .flag("noresolv", !upstream.uses_isp())
        .flag("strictorder", upstream.layers() > 1)
        .list("server", pinned.chain(servers(upstream)))
}

/// The servers dnsmasq forwards to, in order, before the WAN's.
pub(crate) fn servers(upstream: &Upstream) -> Vec<String> {
    let mut servers = Vec::new();
    let mut doh = 0;
    let mut dot = false;
    for resolver in &upstream.encrypted {
        match resolver {
            EncryptedResolver::Doh(_) => {
                servers.push(doh::server(doh));
                doh += 1;
            }
            EncryptedResolver::Dot(_) if !dot => {
                servers.push(dot::server());
                dot = true;
            }
            EncryptedResolver::Dot(_) => {}
        }
    }
    servers.extend(upstream.plain().iter().map(ToString::to_string));
    servers
}

/// The plain server the NTP servers' names resolve through while encrypted resolvers come
/// first: DoH's first bootstrap server (IPv4 first); else, while strict, the first DoT address
/// (IPv4 first; the public DoT services answer plain DNS on port 53 too). Without DoH and not
/// strict there is none: a TLS failure moves the query to the plain layers.
pub(crate) fn ntp_pin(upstream: &Upstream) -> Option<IpAddr> {
    let doh: Vec<_> = upstream.doh().collect();
    doh::bootstrap(&doh).or_else(|| {
        upstream
            .is_strict()
            .then(|| first_ipv4(upstream.dot().flat_map(|r| &r.addresses)))
            .flatten()
    })
}

/// The first IPv4 address, else the first.
pub(crate) fn first_ipv4<'a>(mut ips: impl Iterator<Item = &'a IpAddr>) -> Option<IpAddr> {
    let first = ips.next()?;
    if first.is_ipv4() {
        return Some(*first);
    }
    Some(*ips.find(|ip| ip.is_ipv4()).unwrap_or(first))
}
