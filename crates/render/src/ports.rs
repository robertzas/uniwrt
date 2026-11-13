//! The bridge's ports and what each carries (features/network.md, Networks): an Ethernet port
//! is a trunk (the untagged network untagged, every network with a VLAN tagged) unless the
//! node's port profile says otherwise; a port that carries nothing leaves the bridge. A
//! swconfig switch's user ports take their profiles in its `switch_vlan`s instead
//! ([`switch`](crate::switch)), its CPU port's `<cpu>.<vid>` devices joining the bridge as
//! access ports.

use std::collections::BTreeSet;

use uniwrt_model::{
    facts::{NodeFacts, SwitchLayout},
    node::NodeRole,
    site::node::{NodeConfig, PortConfig, TaggedNetworks},
};

use crate::{
    Issue,
    plan::{BAT, Net},
};

/// A port of the bridge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Port {
    pub device: String,
    /// The VLAN it carries untagged, as its PVID.
    pub native: Option<u16>,
    /// The VLANs it carries tagged.
    pub tagged: Vec<u16>,
}

impl Port {
    pub(crate) fn device(&self) -> &str {
        &self.device
    }
}

/// What a port carries: its native VLAN and its tagged ones.
pub(crate) type Carried = (Option<u16>, Vec<u16>);

/// What a port with `profile` (none: a trunk) carries.
pub(crate) fn carried(profile: Option<&PortConfig>, nets: &[Net<'_>]) -> Carried {
    let vid = |id: &str| nets.iter().find(|net| net.id() == id).map(|net| net.vid);
    let Some(profile) = profile else {
        let native = nets.iter().find(|net| !net.tagged()).map(|net| net.vid);
        let tagged = nets.iter().filter(|net| net.tagged()).map(|net| net.vid);
        return (native, tagged.collect());
    };
    let native = profile.native.as_deref().and_then(vid);
    let tagged = nets
        .iter()
        .filter(|net| net.tagged() && Some(net.vid) != native)
        .filter(|net| match &profile.tagged {
            TaggedNetworks::All => true,
            TaggedNetworks::None => false,
            TaggedNetworks::Only { networks } => networks.iter().any(|id| id == net.id()),
        })
        .map(|net| net.vid);
    (native, tagged.collect())
}

/// The node's profile of `port`.
pub(crate) fn profile<'a>(config: &'a NodeConfig, port: &str) -> Option<&'a PortConfig> {
    config.ports.iter().find(|profile| profile.port == port)
}

/// The bridge's ports: the Ethernet ports (on an AP the WAN port too, as dumb APs bridge every
/// port) but those `claimed` by a raw interface or the backup line, each as its profile says,
/// a swconfig CPU port's device per VLAN, and `bat0`.
pub(crate) fn ports(
    plan: (NodeRole, &NodeFacts, &NodeConfig),
    switch: Option<&SwitchLayout>,
    nets: &[Net<'_>],
    mesh: bool,
    claimed: &BTreeSet<&str>,
) -> Vec<Port> {
    let (role, facts, config) = plan;
    let mut ports: Vec<Port> = Vec::new();
    for name in ethernet(role, facts, switch) {
        if claimed.contains(name.as_str()) || ports.iter().any(|p| p.device == *name) {
            continue;
        }
        let (native, tagged) = carried(profile(config, name), nets);
        if native.is_some() || !tagged.is_empty() {
            ports.push(Port {
                device: name.clone(),
                native,
                tagged,
            });
        }
    }
    if let Some(cpu) = switch.and_then(|s| s.lan_cpu.as_ref()) {
        ports.extend(nets.iter().map(|net| Port {
            device: format!("{}.{}", cpu.device, net.vid),
            native: Some(net.vid),
            tagged: Vec::new(),
        }));
    }
    if mesh {
        let (native, tagged) = carried(None, nets);
        ports.push(Port {
            device: BAT.to_owned(),
            native,
            tagged,
        });
    }
    ports
}

/// The node's Ethernet ports that are devices of their own: its LAN ports (on an AP its WAN
/// port too) but a swconfig CPU port's devices.
fn ethernet<'a>(
    role: NodeRole,
    facts: &'a NodeFacts,
    switch: Option<&SwitchLayout>,
) -> Vec<&'a String> {
    let layout = &facts.board.layout;
    let on_switch = |name: &str| {
        switch.is_some_and(|switch| {
            switch.cpu_ports().iter().any(|cpu| {
                name == cpu.device
                    || name
                        .strip_prefix(cpu.device.as_str())
                        .and_then(|rest| rest.strip_prefix('.'))
                        .is_some_and(|vid| vid.bytes().all(|b| b.is_ascii_digit()))
            })
        })
    };
    let mut names: Vec<&String> = layout.lan.iter().collect();
    if role != NodeRole::Gateway {
        names.extend(&layout.wan);
    }
    names.retain(|name| !on_switch(name));
    names
}

/// The switch's user ports a profile may name: the LAN's (on an AP the WAN's too).
pub(crate) fn switch_ports(role: NodeRole, switch: &SwitchLayout) -> Vec<u8> {
    let mut user = switch.lan_ports.clone();
    if role != NodeRole::Gateway {
        user.extend(&switch.wan_ports);
    }
    user
}

/// Warnings about the node's profiles: ports it lacks or that aren't the bridge's, and on an
/// AP a linked port that drops the network carrying the node's own address (`mgmt`).
pub(crate) fn check(
    plan: (NodeRole, &NodeFacts, &NodeConfig),
    switch: Option<&SwitchLayout>,
    claimed: &BTreeSet<&str>,
    mgmt: Option<&Net<'_>>,
    nets: &[Net<'_>],
) -> Vec<Issue> {
    let (role, facts, config) = plan;
    let ethernet = ethernet(role, facts, switch);
    let user: Vec<String> = switch
        .map(|s| {
            switch_ports(role, s)
                .into_iter()
                .map(|n| PortConfig::switch_port(&s.name, n))
                .collect()
        })
        .unwrap_or_default();
    let mut warnings = Vec::new();
    for profile in &config.ports {
        let at = format!("nodes/{}/ports/{}", config.node, profile.port);
        let port = profile.port.as_str();
        if claimed.contains(port) {
            warnings.push(Issue::new(
                at,
                format!(
                    "Port {port} is taken by the backup line, the adoption port or a raw interface: \
                     its profile is left out"
                ),
            ));
            continue;
        }
        if !ethernet.iter().any(|name| *name == port) && !user.iter().any(|name| name == port) {
            let why = if facts.board.layout.wan.as_deref() == Some(port) {
                "is the WAN port"
            } else {
                "isn't one of the node's LAN ports"
            };
            warnings.push(Issue::new(
                at,
                format!("Port {port} {why}: its profile is left out"),
            ));
            continue;
        }
        let Some(mgmt) = mgmt.filter(|_| role != NodeRole::Gateway) else {
            continue;
        };
        let (native, tagged) = carried(Some(profile), nets);
        let linked = facts
            .devices
            .iter()
            .any(|device| device.name == port && device.carrier);
        if linked && native != Some(mgmt.vid) && !tagged.contains(&mgmt.vid) {
            warnings.push(Issue::new(
                at,
                format!(
                    "Port {port} is linked but won't carry {}, the node's own network: if it's \
                     the node's uplink, the node loses the controller and the apply rolls back",
                    mgmt.network.name
                ),
            ));
        }
    }
    warnings
}
