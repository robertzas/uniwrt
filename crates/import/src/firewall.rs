//! The firewall policy between networks, from the gateway's fw4 zones and forwardings
//! (features/network.md, Firewall policy): each zone takes its network's id as its name (the
//! WAN's is `wan`), and each cell where the forwardings differ from what the networks'
//! purposes seed becomes an override, so the policy renders as the node has it. Named DNAT
//! redirects from the WAN to a host on a network become port forwards (the one named `DMZ`
//! without ports the DMZ host), and miniupnpd's `upnpd` package UPnP; the redirects the model
//! can't hold stay raw.

use std::{collections::BTreeSet, net::Ipv4Addr};

use uniwrt_core::Ipv4Net;
use uniwrt_model::site::{
    SiteConfig,
    firewall::{DMZ, ForwardProto, PortForward, PortRange, Upnp, ZoneAction, ZonePolicy},
};
use uniwrt_uci::Section;

use crate::{Source, read};

const WAN: &str = "wan";

pub(crate) fn import(site: &mut SiteConfig, gateway: &Source<'_>, renames: &mut read::Renames) {
    let Some(firewall) = gateway.packages.get("firewall") else {
        return;
    };
    let ids: BTreeSet<&str> = site.networks.iter().map(|n| n.id.as_str()).collect();
    for zone in firewall.sections_of("zone") {
        let Some(name) = zone.option("name") else {
            continue;
        };
        let covers: Vec<&str> = read::items(zone, "network")
            .into_iter()
            .map(|i| renames.network(i))
            .collect();
        let target = match covers.as_slice() {
            [id] if ids.contains(id) => Some((*id).to_owned()),
            nets if !nets.is_empty() && nets.iter().all(|n| n.starts_with("wan")) => {
                Some(WAN.to_owned())
            }
            _ => None,
        };
        if let Some(target) = target.filter(|t| t != name) {
            renames.zones.insert(name.to_owned(), target);
        }
    }
    let zone = |name: &str| -> String {
        renames
            .zones
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_owned())
    };
    let allowed: BTreeSet<(String, String)> = firewall
        .sections_of("forwarding")
        .filter(|f| read::flag(f, "enabled") != Some(false))
        .filter_map(|f| Some((zone(f.option("src")?), zone(f.option("dest")?))))
        .collect();
    let mut zones: Vec<(&str, Option<_>)> = site
        .networks
        .iter()
        .map(|n| (n.id.as_str(), Some(n.purpose)))
        .collect();
    zones.push((WAN, None));
    let mut policies = Vec::new();
    for &(from, purpose) in &zones {
        for &(to, _) in &zones {
            if from == to {
                continue;
            }
            let seeded = purpose.is_some_and(|p| uniwrt_render::seeded(p, to == WAN));
            let actual = allowed.contains(&(from.to_owned(), to.to_owned()));
            if seeded != actual {
                policies.push(ZonePolicy {
                    from: from.to_owned(),
                    to: to.to_owned(),
                    action: if actual {
                        ZoneAction::Allow
                    } else {
                        ZoneAction::Block
                    },
                });
            }
        }
    }
    site.firewall.policies = policies;
    let wan = |section: &Section| section.option("src").map(zone).as_deref() == Some(WAN);
    site.firewall.forwards.clear();
    site.firewall.dmz = None;
    for redirect in firewall.sections_of("redirect").filter(|r| wan(r)) {
        if redirect.option("name") == Some(DMZ) {
            if site.firewall.dmz.is_none() {
                site.firewall.dmz = dmz(site, redirect);
            }
        } else if let Some(forward) = forward(site, redirect) {
            site.firewall.forwards.push(forward);
        }
    }
    site.firewall.upnp = upnp(site, gateway, renames);
}

/// A DNAT target, as fw4 defaults a redirect's.
fn dnat(redirect: &Section) -> bool {
    matches!(redirect.option("target"), None | Some("DNAT"))
}

/// A host on one of the site's networks, not the gateway.
fn host(site: &SiteConfig, redirect: &Section) -> Option<Ipv4Addr> {
    let ip: Ipv4Addr = redirect.option("dest_ip")?.parse().ok()?;
    site.networks
        .iter()
        .filter_map(|n| n.ipv4.as_ref())
        .any(|ipv4| ipv4.address.trunc().contains(&ip) && ipv4.address.addr() != ip)
        .then_some(ip)
}

fn dmz(site: &SiteConfig, redirect: &Section) -> Option<Ipv4Addr> {
    let all = matches!(read::items(redirect, "proto").as_slice(), ["all" | "any"]);
    let ported = ["src_dport", "dest_port", "src_ip"]
        .iter()
        .any(|o| redirect.get(o).is_some());
    (dnat(redirect) && all && !ported).then(|| host(site, redirect))?
}

/// A named forward the model holds as the node has it, its name unique.
fn forward(site: &SiteConfig, redirect: &Section) -> Option<PortForward> {
    let name = redirect.option("name")?;
    let taken = site
        .firewall
        .forwards
        .iter()
        .any(|f| f.name.eq_ignore_ascii_case(name));
    if !dnat(redirect) || taken || name.trim() != name || name.is_empty() {
        return None;
    }
    let proto = match read::items(redirect, "proto").as_slice() {
        [] | ["tcp", "udp"] | ["udp", "tcp"] | ["tcpudp"] => ForwardProto::TcpUdp,
        ["tcp"] => ForwardProto::Tcp,
        ["udp"] => ForwardProto::Udp,
        _ => return None,
    };
    let ports: PortRange = redirect.option("src_dport")?.parse().ok()?;
    let to_ports: PortRange = match redirect.option("dest_port") {
        Some(p) => p.parse().ok()?,
        None => ports,
    };
    if to_ports.last - to_ports.first != ports.last - ports.first {
        return None;
    }
    let source = match redirect.get("src_ip") {
        None => None,
        Some(_) => match read::items(redirect, "src_ip").as_slice() {
            [one] => Some(
                one.parse::<Ipv4Net>()
                    .or_else(|_| one.parse::<Ipv4Addr>().map(Ipv4Net::from))
                    .ok()?,
            ),
            _ => return None,
        },
    };
    Some(PortForward {
        name: name.to_owned(),
        enabled: read::flag(redirect, "enabled") != Some(false),
        proto,
        ports,
        to: host(site, redirect)?,
        to_port: (to_ports != ports).then_some(to_ports.first),
        source,
        reflection: read::flag(redirect, "reflection") != Some(false),
    })
}

/// miniupnpd's settings: on unless its package says `enabled 0` (as its init script reads it),
/// for the networks it listens on (the default network unless it names them); off without the
/// package.
fn upnp(site: &SiteConfig, gateway: &Source<'_>, renames: &read::Renames) -> Upnp {
    let lan = || {
        site.networks
            .iter()
            .find(|n| n.purpose == uniwrt_model::site::network::Purpose::Default)
            .map(|n| n.id.clone())
    };
    let Some(config) = read::first(gateway.packages, "upnpd", "upnpd") else {
        return Upnp {
            enabled: false,
            networks: lan().into_iter().collect(),
        };
    };
    let named = read::items(config, "internal_iface");
    let networks = if named.is_empty() {
        lan().into_iter().collect()
    } else {
        named
            .into_iter()
            .map(|i| renames.network(i).to_owned())
            .filter(|id| {
                site.networks
                    .iter()
                    .any(|n| &n.id == id && n.ipv4.is_some())
            })
            .fold(Vec::new(), |mut ids, id| {
                if !ids.contains(&id) {
                    ids.push(id);
                }
                ids
            })
    };
    Upnp {
        enabled: read::flag(config, "enabled") != Some(false) && !networks.is_empty(),
        networks,
    }
}
