//! The adoption port on the gateway (features/adoption.md, Path C; D-105): the port leaves the
//! LAN for a bridge of its own, `br-adopt`, where the gateway holds a static address in
//! factory-default OpenWrt's 192.168.1.0/24 with IPv6 off (a factory device's router
//! advertisements stay out). Its interface, `uw_adopt`, gets no DHCP and a firewall zone that
//! accepts nothing from the segment and forwards nothing: the gateway reaches the device's SSH
//! server, and fw4 lets the replies back.

use uniwrt_model::{
    facts::{NodeFacts, SwitchLayout},
    site::{SiteConfig, adoption::AdoptionPort, gateway::BackupLink},
};
use uniwrt_uci::Section;

use crate::{Issue, ext::SectionExt, plan::Plan};

/// The segment's interface, and its firewall zone.
pub const ADOPT_INTERFACE: &str = "uw_adopt";
/// The segment's bridge.
pub const ADOPT_BRIDGE: &str = "br-adopt";

const AT: &str = "adoption_port/port";

/// The site's adoption port, where the gateway can take it: a port of its own that isn't the
/// WAN's, the backup line's nor one of a swconfig switch (whose ports aren't devices of their own), and an uplink
/// outside 192.168.1.0/24 (the segment's route would take the uplink's place).
pub(crate) fn plan<'a>(
    site: &'a SiteConfig,
    facts: &NodeFacts,
    switch: Option<&SwitchLayout>,
    errors: &mut Vec<Issue>,
    warnings: &mut Vec<Issue>,
) -> Option<&'a AdoptionPort> {
    let adoption = site.adoption_port.as_ref()?;
    let port = adoption.port.as_str();
    let layout = &facts.board.layout;
    if layout.wan.as_deref() == Some(port) {
        errors.push(Issue::new(
            AT,
            "The adoption port can't be the WAN port: pick a LAN port",
        ));
        return None;
    }
    if let Some(BackupLink::Port { port: backup, .. }) = site.wan.failover.as_ref().map(|f| &f.link)
        && backup == port
    {
        errors.push(Issue::new(
            AT,
            format!("{port} is the backup line's port: pick another"),
        ));
        return None;
    }
    if switch.is_some() && layout.lan.iter().any(|lan| lan == port) {
        errors.push(Issue::new(
            AT,
            format!(
                "{port} is a port of the gateway's swconfig switch, which can't be taken out \
                 of the LAN on its own"
            ),
        ));
        return None;
    }
    let upstream = facts
        .interfaces
        .iter()
        .filter(|i| matches!(i.name.as_str(), "wan" | "wanb"))
        .flat_map(|i| i.ipv4.iter().map(uniwrt_core::Ipv4Net::trunc))
        .find(|up| {
            up.contains(&AdoptionPort::SUBNET.network())
                || AdoptionPort::SUBNET.contains(&up.network())
        });
    if let Some(upstream) = upstream {
        errors.push(Issue::new(
            AT,
            format!(
                "The gateway's uplink holds {upstream} now (the modem's network), which \
                 overlaps the adoption port's 192.168.1.0/24: the gateway would lose its way \
                 to the internet while a device is plugged in. Move the modem's network, or \
                 turn the adoption port off"
            ),
        ));
        return None;
    }
    if !facts.devices.is_empty() && !facts.devices.iter().any(|device| device.name == port) {
        warnings.push(Issue::new(
            AT,
            format!("The gateway has no port {port} now"),
        ));
    }
    Some(adoption)
}

/// The `network` sections: the bridge and its interface.
pub(crate) fn network(plan: &Plan<'_>) -> Vec<Section> {
    let Some(adoption) = plan.adoption else {
        return Vec::new();
    };
    vec![
        Section::anonymous("device")
            .with("name", ADOPT_BRIDGE)
            .with("type", "bridge")
            .with("ports", [adoption.port.as_str()])
            .with("ipv6", "0"),
        Section::named("interface", ADOPT_INTERFACE)
            .with("device", ADOPT_BRIDGE)
            .with("proto", "static")
            .list("ipaddr", [adoption.interface().to_string()]),
    ]
}

/// The `dhcp` section: no DHCP or router advertisements on the segment, where the factory
/// device serves its own.
pub(crate) fn dhcp(plan: &Plan<'_>) -> Option<Section> {
    plan.adoption.map(|_| {
        Section::named("dhcp", ADOPT_INTERFACE)
            .with("interface", ADOPT_INTERFACE)
            .with("ignore", "1")
    })
}

/// The `firewall` zone: nothing in, nothing forwarded.
pub(crate) fn firewall(plan: &Plan<'_>) -> Option<Section> {
    plan.adoption.map(|_| {
        Section::anonymous("zone")
            .with("name", ADOPT_INTERFACE)
            .with("network", [ADOPT_INTERFACE])
            .with("input", "REJECT")
            .with("output", "ACCEPT")
            .with("forward", "REJECT")
    })
}
