//! The `interfaces` set: `network.interface dump`. An interface's IPv6 addresses include the
//! ones netifd assigned it from delegated prefixes (`ipv6-prefix-assignment`); its gateways are
//! the next hops of its default routes.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use serde::Deserialize;
use uniwrt_core::{Ipv4Net, Ipv6Net};
use uniwrt_model::facts::InterfaceFacts;

#[derive(Deserialize)]
pub(super) struct Raw {
    dump: Dump,
}

#[derive(Deserialize)]
struct Dump {
    interface: Vec<Iface>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Iface {
    interface: String,
    #[serde(default)]
    proto: String,
    #[serde(default)]
    up: bool,
    #[serde(default, rename = "l3_device")]
    l3_device: Option<String>,
    #[serde(default)]
    device: Option<String>,
    #[serde(default)]
    uptime: Option<u64>,
    #[serde(default)]
    ipv4_address: Vec<Addr>,
    #[serde(default)]
    ipv6_address: Vec<Addr>,
    #[serde(default)]
    ipv6_prefix: Vec<Addr>,
    #[serde(default)]
    ipv6_prefix_assignment: Vec<Assignment>,
    #[serde(default)]
    route: Vec<Route>,
    #[serde(default)]
    dns_server: Vec<String>,
    #[serde(default)]
    errors: Vec<IfaceError>,
}

#[derive(Deserialize)]
struct Addr {
    address: String,
    mask: u8,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Assignment {
    #[serde(default)]
    local_address: Option<Addr>,
}

#[derive(Deserialize)]
struct Route {
    target: String,
    mask: u8,
    #[serde(default)]
    nexthop: Option<String>,
}

#[derive(Deserialize)]
struct IfaceError {
    code: String,
}

pub(super) fn parse(raw: Raw) -> Vec<InterfaceFacts> {
    raw.dump.interface.into_iter().map(interface).collect()
}

fn interface(iface: Iface) -> InterfaceFacts {
    let ipv6 = iface
        .ipv6_address
        .iter()
        .chain(
            iface
                .ipv6_prefix_assignment
                .iter()
                .filter_map(|a| a.local_address.as_ref()),
        )
        .filter_map(v6)
        .fold(Vec::new(), |mut all, net| {
            if !all.contains(&net) {
                all.push(net);
            }
            all
        });
    InterfaceFacts {
        name: iface.interface,
        proto: iface.proto,
        up: iface.up,
        device: iface.l3_device.or(iface.device),
        uptime_s: iface.uptime,
        ipv4: iface.ipv4_address.iter().filter_map(v4).collect(),
        ipv6,
        delegated_prefixes: iface.ipv6_prefix.iter().filter_map(v6).collect(),
        ipv4_gateway: gateway::<Ipv4Addr>(&iface.route),
        ipv6_gateway: gateway::<Ipv6Addr>(&iface.route),
        dns_servers: iface
            .dns_server
            .iter()
            .filter_map(|server| server.parse::<IpAddr>().ok())
            .collect(),
        errors: iface.errors.into_iter().map(|err| err.code).collect(),
    }
}

fn v4(addr: &Addr) -> Option<Ipv4Net> {
    Ipv4Net::new(addr.address.parse().ok()?, addr.mask).ok()
}

fn v6(addr: &Addr) -> Option<Ipv6Net> {
    Ipv6Net::new(addr.address.parse().ok()?, addr.mask).ok()
}

/// The next hop of the first default route of this address family.
fn gateway<A>(routes: &[Route]) -> Option<A>
where
    A: std::str::FromStr + Into<IpAddr> + Copy,
{
    routes.iter().find_map(|route| {
        let target: A = route.target.parse().ok()?;
        if route.mask != 0 || !target.into().is_unspecified() {
            return None;
        }
        route.nexthop.as_ref()?.parse().ok()
    })
}
