//! The `mdns_repeater` package (features/network.md, Other services; D-136): the gateway's
//! `mdns-repeater` reflects mDNS (IPv4) between the networks that have it on and an address on
//! the gateway, at most [`MDNS_NETWORKS`] (validation refuses more). It runs only for two such
//! networks or more; with fewer, a gateway that holds the package keeps its section without
//! interfaces, the service off. The untrusted zones among them may reach it ([`firewall`]).

use uniwrt_model::site::network::{MDNS_NETWORKS, Purpose};
use uniwrt_uci::{Package, Section};

use crate::{
    Requirement,
    ext::SectionExt,
    plan::{Net, Plan},
};

/// The repeater with its init script.
pub(crate) const REQUIRES: Requirement = Requirement {
    package: "mdns-repeater",
    service: Some("mdns-repeater"),
};

/// The networks it reflects between, when they are two or more.
pub(crate) fn networks<'p, 'a>(plan: &'p Plan<'a>) -> Vec<&'p Net<'a>> {
    let nets: Vec<&Net<'a>> = plan
        .nets
        .iter()
        .filter(|net| net.network.mdns && net.network.ipv4.is_some())
        .take(MDNS_NETWORKS)
        .collect();
    if nets.len() < 2 { Vec::new() } else { nets }
}

/// The package, or `None` where there is nothing to own: not the gateway, or no reflection
/// and no package on the node.
pub(crate) fn render(plan: &Plan<'_>, held: bool) -> Option<Package> {
    plan.wan.as_ref()?;
    let nets = networks(plan);
    if nets.is_empty() && !held {
        return None;
    }
    let mut package = Package::new("mdns_repeater");
    package.sections = vec![
        Section::named("mdns_repeater", "main")
            .list("interface", nets.iter().map(|net| plan.device(net))),
    ];
    Some(package)
}

/// The rules that let the untrusted networks it serves reach it.
pub(crate) fn firewall(plan: &Plan<'_>) -> Vec<Section> {
    networks(plan)
        .into_iter()
        .filter(|net| !matches!(net.network.purpose, Purpose::Default | Purpose::Mgmt))
        .map(|net| {
            Section::anonymous("rule")
                .with("name", format!("Allow-mDNS-{}", net.id()))
                .with("src", net.id())
                .with("proto", "udp")
                .with("dest_ip", "224.0.0.251")
                .with("dest_port", "5353")
                .with("family", "ipv4")
                .with("target", "ACCEPT")
        })
        .collect()
}
