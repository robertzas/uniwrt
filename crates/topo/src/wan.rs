//! The internet and each gateway's links to it: its interfaces with a default route, or named
//! like OpenWrt's WAN (`wan`, `wan6`, `wanb`), one link per device beneath them.

use std::collections::BTreeMap;

use uniwrt_model::{facts::InterfaceFacts, node::NodeRole};

use crate::{
    Medium, TopoKind, TopoNode,
    fleet::{Fleet, INTERNET, device},
    links::{Graph, Link, Sample},
};

pub(crate) fn link(graph: &mut Graph, fleet: &Fleet<'_>) {
    if !fleet
        .members
        .iter()
        .any(|member| member.role == NodeRole::Gateway)
    {
        return;
    }
    graph.vertex(TopoNode {
        id: INTERNET.to_owned(),
        kind: TopoKind::Internet,
        node: None,
        mac: None,
        name: None,
        ips: Vec::new(),
        state: None,
        cpu_percent: None,
        memory_percent: None,
        parent: None,
        uplink: None,
        hops: None,
    });
    for (member, node, facts) in fleet.with_facts() {
        if node.role != NodeRole::Gateway {
            continue;
        }
        let mut wans = BTreeMap::<&str, Vec<&InterfaceFacts>>::new();
        for iface in facts.interfaces.iter().filter(|iface| is_wan(iface)) {
            let name = iface.device.as_deref().unwrap_or(&iface.name);
            wans.entry(name).or_default().push(iface);
        }
        for (name, ifaces) in wans {
            let mut link = Link::new(fleet.vertex(member), INTERNET.to_owned(), Medium::Wan);
            link.ports[0] = Some(name.to_owned());
            link.tag = Some(name.to_owned());
            link.up = ifaces
                .iter()
                .any(|iface| iface.up && has_default_route(iface));
            if let Some(dev) = device(facts, name) {
                link.capacity_mbps = dev.speed_mbps;
                link.sample = Some(Sample::port(facts.collected_at, &dev.counters));
            }
            graph.link(link);
        }
    }
}

fn is_wan(iface: &InterfaceFacts) -> bool {
    has_default_route(iface) || (iface.name.starts_with("wan") && iface.proto != "none")
}

const fn has_default_route(iface: &InterfaceFacts) -> bool {
    iface.ipv4_gateway.is_some() || iface.ipv6_gateway.is_some()
}
