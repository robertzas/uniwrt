//! The `network` package: loopback, the ULA, the bridge every network joins and its VLANs, the
//! swconfig switch, one interface per network, the WAN and the mesh.

use uniwrt_model::{
    node::NodeRole,
    site::network::{Ipv4Config, Ipv6Mode},
};
use uniwrt_uci::{Package, Section};

use crate::{
    ext::SectionExt,
    failover, mesh,
    plan::{BRIDGE, Net, Plan},
    ports::Port,
    switch, wan,
};

pub(crate) fn render(plan: &Plan<'_>) -> Package {
    let mut sections = vec![
        Section::named("interface", "loopback")
            .with("device", "lo")
            .with("proto", "static")
            .with("ipaddr", ["127.0.0.1/8"]),
        Section::named("globals", "globals").maybe(
            "ula_prefix",
            plan.site.addressing.ula.map(|ula| ula.to_string()),
        ),
    ];
    sections.push(bridge(plan));
    if plan.vlan_aware {
        sections.extend(plan.nets.iter().map(|net| bridge_vlan(plan, net)));
    }
    sections.extend(wan::devices(plan));
    sections.extend(switch::sections(plan));
    sections.extend(plan.nets.iter().map(|net| interface(plan, net)));
    sections.extend(wan::interfaces(plan));
    sections.extend(failover::network(plan));
    sections.extend(mesh::interfaces(plan));
    let mut package = Package::new("network");
    package.sections = sections;
    package
}

fn bridge(plan: &Plan<'_>) -> Section {
    let snooping = plan.nets.iter().any(|net| net.network.igmp_snooping);
    Section::anonymous("device")
        .with("name", BRIDGE)
        .with("type", "bridge")
        .list("ports", plan.ports.iter().map(Port::device))
        .flag("igmp_snooping", snooping)
        .maybe(
            "macaddr",
            plan.facts.board.layout.lan_mac.map(|mac| mac.to_string()),
        )
}

/// A network's VLAN on the bridge: the ports whose native VLAN it is carry it untagged (as
/// their PVID), the ones that tag it tagged; trunks carry the untagged network untagged and the
/// others tagged.
fn bridge_vlan(plan: &Plan<'_>, net: &Net<'_>) -> Section {
    let ports = plan.ports.iter().filter_map(|port: &Port| {
        if port.native == Some(net.vid) {
            Some(format!("{}:u*", port.device))
        } else if port.tagged.contains(&net.vid) {
            Some(format!("{}:t", port.device))
        } else {
            None
        }
    });
    Section::anonymous("bridge-vlan")
        .with("device", BRIDGE)
        .with("vlan", net.vid.to_string())
        .list("ports", ports)
}

fn interface(plan: &Plan<'_>, net: &Net<'_>) -> Section {
    let section = Section::named("interface", net.id()).with("device", plan.device(net));
    if plan.role != NodeRole::Gateway {
        // APs bridge; they hold an address only on the management network.
        return if net.id() == plan.mgmt {
            section.with("proto", "dhcp")
        } else {
            section.with("proto", "none")
        };
    }
    let network = net.network;
    let ipv6 = network.ipv6 != Ipv6Mode::Off;
    if network.ipv4.is_none() && !ipv6 {
        return section.with("proto", "none");
    }
    let classes: &[&str] = match network.ipv6 {
        Ipv6Mode::Ula => &["local"],
        Ipv6Mode::Delegated => &[wan::WAN6],
        Ipv6Mode::Off | Ipv6Mode::Both => &[],
    };
    section
        .with("proto", "static")
        .list(
            "ipaddr",
            network
                .ipv4
                .as_ref()
                .map(|Ipv4Config { address, .. }| address.to_string()),
        )
        .maybe("ip6assign", ipv6.then_some("64"))
        // A /64 per network, numbered after its VLAN (features/network.md, IPv6).
        .maybe(
            "ip6hint",
            (ipv6 && net.tagged()).then(|| format!("{:x}", net.vid)),
        )
        .list("ip6class", classes.iter().copied())
}
