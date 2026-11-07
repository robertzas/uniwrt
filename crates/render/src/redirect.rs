//! The gateway's port forwards and DMZ host as fw4 `redirect`s (features/network.md, Firewall
//! policy): DNAT from the WAN to the zone of the network whose subnet holds the host. fw4
//! accepts what it forwards, and reflects it to the networks (NAT reflection) unless a forward
//! says otherwise. The DMZ comes last, so the forwards' ports go where they say; it is written
//! as LuCI writes one, so a brownfield DMZ imports as it is.

use std::net::Ipv4Addr;

use uniwrt_model::site::firewall::{DMZ, PortForward};
use uniwrt_uci::Section;

use crate::{ext::SectionExt, plan::Plan};

pub(crate) fn sections(plan: &Plan<'_>, wan: &str) -> Vec<Section> {
    let firewall = &plan.site.firewall;
    let mut sections: Vec<Section> = firewall
        .forwards
        .iter()
        .filter_map(|forward| Some(forward_section(forward, zone_of(plan, forward.to)?, wan)))
        .collect();
    if let Some(host) = firewall.dmz
        && let Some(zone) = zone_of(plan, host)
    {
        sections.push(
            Section::anonymous("redirect")
                .with("name", DMZ)
                .with("target", "DNAT")
                .with("src", wan)
                .with("proto", "all")
                .with("dest", zone)
                .with("dest_ip", host.to_string()),
        );
    }
    sections
}

fn forward_section(forward: &PortForward, zone: &str, wan: &str) -> Section {
    let to_ports = forward.to_ports().unwrap_or(forward.ports);
    let section = Section::anonymous("redirect")
        .with("name", forward.name.as_str())
        .with("target", "DNAT")
        .with("src", wan)
        .maybe("src_ip", forward.source.map(|net| net.to_string()))
        .with("src_dport", forward.ports.to_string())
        .with("dest", zone)
        .with("dest_ip", forward.to.to_string())
        .with("dest_port", to_ports.to_string())
        .list("proto", forward.proto.names().iter().copied());
    let section = if forward.reflection {
        section
    } else {
        section.with("reflection", "0")
    };
    if forward.enabled {
        section
    } else {
        section.with("enabled", "0")
    }
}

/// The zone of the network whose subnet holds `host`; validation sees that one does.
fn zone_of<'p>(plan: &'p Plan<'_>, host: Ipv4Addr) -> Option<&'p str> {
    plan.nets
        .iter()
        .find(|net| {
            net.network
                .ipv4
                .as_ref()
                .is_some_and(|ipv4| ipv4.address.trunc().contains(&host))
        })
        .map(crate::plan::Net::id)
}
