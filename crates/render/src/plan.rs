//! What every package's renderer shares, worked out once per node: the VLAN each network has
//! on the bridge, the bridge's ports, the WAN's device, the mesh radio, and the issues found.

use std::collections::{BTreeMap, BTreeSet};

use crate::{Issue, Secrets, Target};
use uniwrt_model::{
    facts::{NodeFacts, RadioFacts, Software, SwitchKind, SwitchLayout, WifiFeature},
    node::NodeRole,
    site::{SiteConfig, gateway::WanProto, network::Network, node::NodeConfig, wireless::Channel},
    wifi::Band,
};

/// The bridge every network joins.
pub const BRIDGE: &str = "br-lan";
/// batman-adv's mesh device.
pub const BAT: &str = "bat0";

pub(crate) struct Plan<'a> {
    pub site: &'a SiteConfig,
    pub secrets: &'a dyn Secrets,
    pub config: &'a NodeConfig,
    pub role: NodeRole,
    pub facts: &'a NodeFacts,
    /// The bridge filters VLANs: some network is tagged.
    pub vlan_aware: bool,
    /// The networks, in document order, with their VLAN on the bridge.
    pub nets: Vec<Net<'a>>,
    pub ports: Vec<Port>,
    /// The swconfig switch, when the node drives one.
    pub switch: Option<&'a SwitchLayout>,
    /// The gateway's uplink.
    pub wan: Option<Wan>,
    /// The PPPoE password from the vault.
    pub wan_password: Option<String>,
    pub mesh: Option<Mesh<'a>>,
    /// The network that carries an AP's own address: the management network, else the
    /// default one.
    pub mgmt: &'a str,
    pub errors: Vec<Issue>,
    pub warnings: Vec<Issue>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Net<'a> {
    pub network: &'a Network,
    /// Its VLAN ID, or for the untagged network the bridge's internal one.
    pub vid: u16,
}

impl Net<'_> {
    pub(crate) fn id(&self) -> &str {
        &self.network.id
    }

    pub(crate) fn tagged(&self) -> bool {
        self.network.vlan.is_some()
    }
}

/// A port of the bridge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Port {
    /// Carries the untagged network untagged and the others tagged: Ethernet ports, `bat0`.
    Trunk(String),
    /// Carries one VLAN untagged: a swconfig CPU port's `<cpu>.<vid>` device.
    Access { device: String, vid: u16 },
}

impl Port {
    pub(crate) fn device(&self) -> &str {
        match self {
            Self::Trunk(device) | Self::Access { device, .. } => device,
        }
    }
}

/// The gateway's uplink device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Wan {
    /// The device `interface wan` uses, such as `wan`, `wan.201` or `eth0.2`.
    pub device: String,
    /// An 802.1Q device to declare: its parent port and VLAN.
    pub vlan_device: Option<(String, u16)>,
    /// On a swconfig switch: the WAN's VLAN and whether its ports tag it.
    pub switch_vlan: Option<(u16, bool)>,
}

/// The node's part in the mesh.
pub(crate) struct Mesh<'a> {
    pub radio: &'a RadioFacts,
    /// The radio carries only the mesh (a tri-band node's second radio of the band).
    pub dedicated: bool,
    pub channel: u16,
    pub key: String,
}

impl<'a> Plan<'a> {
    pub(crate) fn new(
        site: &'a SiteConfig,
        secrets: &'a dyn Secrets,
        config: &'a NodeConfig,
        target: Target<'a>,
    ) -> Result<Self, Vec<Issue>> {
        let facts = target.facts;
        let layout = &facts.board.layout;
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let tagged: BTreeSet<u16> = site.networks.iter().filter_map(|n| n.vlan).collect();
        let free = |taken: &BTreeSet<u16>| (1..4095).find(|vid| !taken.contains(vid));
        let untagged_vid = free(&tagged).unwrap_or(1);
        let nets: Vec<Net<'a>> = site
            .networks
            .iter()
            .map(|network| Net {
                network,
                vid: network.vlan.unwrap_or(untagged_vid),
            })
            .collect();
        let switch = match facts.capabilities.switch {
            Some(SwitchKind::Swconfig) => match &layout.switch {
                Some(switch) if switch.lan_cpu.is_some() => Some(switch),
                _ => {
                    errors.push(Issue::new(
                        "node",
                        "The node drives a swconfig switch, but its board.json describes none",
                    ));
                    None
                }
            },
            _ => None,
        };
        let mesh = mesh(site, secrets, config, facts, &mut errors, &mut warnings);
        // A port a raw interface takes as its device (a wired batman-adv link, a VLAN of its
        // own) has another job: bridging it too would take it from that interface.
        let claimed: BTreeSet<&str> = config
            .raw
            .iter()
            .filter(|r| r.package == "network" && r.kind == "interface" && !r.merge)
            .filter_map(|r| match r.options.get("device") {
                Some(uniwrt_model::site::node::RawValue::Option(device)) => Some(device.as_str()),
                _ => None,
            })
            .collect();
        let ports = ports(target.role, facts, switch, &nets, mesh.is_some(), &claimed);
        if ports.is_empty() && mesh.is_none() {
            errors.push(Issue::new(
                "node",
                "The node's board.json names no LAN port",
            ));
        }
        let wan = if target.role == NodeRole::Gateway {
            wan(site, facts, switch, &nets, &mut errors)
        } else {
            None
        };
        if site.wan.pcp.is_some_and(|pcp| pcp > 0)
            && wan.as_ref().is_some_and(|w| w.vlan_device.is_none())
        {
            warnings.push(Issue::new(
                "wan/pcp",
                "The gateway's switch tags the ISP's VLAN, so the priority isn't set",
            ));
        }
        let wan_password = match &site.wan.proto {
            WanProto::Pppoe { password, .. } if target.role == NodeRole::Gateway => {
                let value = password
                    .as_ref()
                    .and_then(|p| secrets.get(p))
                    .map(str::to_owned);
                if value.is_none() {
                    errors.push(Issue::new(
                        "wan/password",
                        "The PPPoE password isn't set or isn't in the vault",
                    ));
                }
                value
            }
            _ => None,
        };
        let mgmt = crate::management_network(site).map_or("lan", |n| n.id.as_str());
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(Self {
            site,
            secrets,
            config,
            role: target.role,
            facts,
            vlan_aware: !tagged.is_empty(),
            nets,
            ports,
            switch,
            wan,
            wan_password,
            mesh,
            mgmt,
            errors,
            warnings,
        })
    }

    /// The device a network's interface sits on.
    pub(crate) fn device(&self, net: &Net<'_>) -> String {
        if self.vlan_aware {
            format!("{BRIDGE}.{}", net.vid)
        } else {
            BRIDGE.to_owned()
        }
    }

    pub(crate) fn net(&self, id: &str) -> Option<&Net<'a>> {
        self.nets.iter().find(|net| net.id() == id)
    }

    /// The vault's value of a secret, or an error at `path`.
    pub(crate) fn secret(
        &mut self,
        path: &str,
        secret: Option<&uniwrt_model::site::SecretRef>,
    ) -> Option<String> {
        let value = secret.and_then(|s| self.secrets.get(s)).map(str::to_owned);
        if value.is_none() {
            self.errors.push(Issue::new(
                path,
                "The secret isn't set or isn't in the vault",
            ));
        }
        value
    }
}

/// The bridge's ports: the Ethernet ports (on an AP the WAN port too, as dumb APs bridge
/// every port) but those `claimed` by a raw interface, a swconfig CPU port's device per VLAN,
/// and `bat0`.
fn ports(
    role: NodeRole,
    facts: &NodeFacts,
    switch: Option<&SwitchLayout>,
    nets: &[Net<'_>],
    mesh: bool,
    claimed: &BTreeSet<&str>,
) -> Vec<Port> {
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
    let mut ports: Vec<Port> = Vec::new();
    for name in names {
        if !on_switch(name)
            && !claimed.contains(name.as_str())
            && !ports.iter().any(|p| p.device() == name)
        {
            ports.push(Port::Trunk(name.clone()));
        }
    }
    if let Some(cpu) = switch.and_then(|s| s.lan_cpu.as_ref()) {
        ports.extend(nets.iter().map(|net| Port::Access {
            device: format!("{}.{}", cpu.device, net.vid),
            vid: net.vid,
        }));
    }
    if mesh {
        ports.push(Port::Trunk(BAT.to_owned()));
    }
    ports
}

fn wan(
    site: &SiteConfig,
    facts: &NodeFacts,
    switch: Option<&SwitchLayout>,
    nets: &[Net<'_>],
    errors: &mut Vec<Issue>,
) -> Option<Wan> {
    let Some(port) = facts.board.layout.wan.clone() else {
        errors.push(Issue::new(
            "wan",
            "The gateway has no WAN port: its board.json names none",
        ));
        return None;
    };
    let isp_vlan = site.wan.vlan;
    let switched = switch.and_then(|s| {
        let cpu = s.wan_cpu.as_ref()?;
        (!s.wan_ports.is_empty() && port.starts_with(&format!("{}.", cpu.device))).then_some(cpu)
    });
    if let Some(cpu) = switched {
        let taken: BTreeSet<u16> = nets.iter().map(|n| n.vid).collect();
        let vid = isp_vlan.or_else(|| (2..4095).find(|vid| !taken.contains(vid)))?;
        if taken.contains(&vid) {
            errors.push(Issue::new(
                "wan/vlan",
                format!("The WAN's VLAN {vid} is also a network's VLAN on the gateway's switch"),
            ));
        }
        return Some(Wan {
            device: format!("{}.{vid}", cpu.device),
            vlan_device: None,
            switch_vlan: Some((vid, isp_vlan.is_some())),
        });
    }
    Some(match isp_vlan {
        Some(vid) => Wan {
            device: format!("{port}.{vid}"),
            vlan_device: Some((port, vid)),
            switch_vlan: None,
        },
        None => Wan {
            device: port,
            vlan_device: None,
            switch_vlan: None,
        },
    })
}

/// The channel a mesh takes when the band's policy leaves it to the node: every peer must
/// share one, so it can't be chosen per node.
pub(crate) const fn mesh_default_channel(band: Band) -> u16 {
    match band {
        Band::G2 => 1,
        Band::G5 => 36,
        Band::G6 => 37,
        Band::G60 => 2,
    }
}

fn mesh<'a>(
    site: &SiteConfig,
    secrets: &dyn Secrets,
    config: &NodeConfig,
    facts: &'a NodeFacts,
    errors: &mut Vec<Issue>,
    warnings: &mut Vec<Issue>,
) -> Option<Mesh<'a>> {
    let mesh = &site.mesh;
    if !mesh.enabled {
        return None;
    }
    let caps = &facts.capabilities;
    let mut missing = Vec::new();
    if !caps.software.contains(&Software::BatmanAdv) {
        missing.push("batman-adv (kmod-batman-adv)");
    }
    if !caps.wifi.contains(&WifiFeature::Mesh) {
        missing.push("802.11s with SAE (the full wpad)");
    }
    if !missing.is_empty() {
        errors.push(Issue::new(
            "mesh",
            format!("The node lacks {} for the mesh", missing.join(" and ")),
        ));
        return None;
    }
    let band = mesh.band.unwrap_or(Band::G5);
    let disabled: BTreeMap<&str, bool> = config
        .radios
        .iter()
        .map(|r| (r.radio.as_str(), r.disabled))
        .collect();
    let candidates: Vec<&RadioFacts> = facts
        .radios
        .iter()
        .filter(|r| r.band == band && !disabled.get(r.name.as_str()).copied().unwrap_or(false))
        .collect();
    // The high band's radio of a tri-band node: the one that reaches the highest channel.
    let top = |radio: &RadioFacts| {
        facts
            .phys
            .iter()
            .find(|phy| Some(&phy.name) == radio.phy.as_ref())
            .and_then(|phy| phy.bands.iter().find(|b| b.band == band))
            .and_then(|b| b.channels.iter().map(|c| c.number).max())
            .unwrap_or(0)
    };
    let Some(radio) = candidates
        .iter()
        .copied()
        .max_by_key(|r| (top(r), std::cmp::Reverse(&r.name)))
    else {
        warnings.push(Issue::new(
            "mesh",
            format!(
                "The node has no {} radio for the mesh: it reaches the others over wired links only",
                band_name(band)
            ),
        ));
        return None;
    };
    let fixed = site
        .radio
        .bands
        .iter()
        .find(|p| p.band == band)
        .and_then(|p| match p.channel {
            Channel::Fixed(n) => Some(n),
            Channel::Auto => None,
        });
    let channel = fixed.unwrap_or_else(|| {
        let channel = mesh_default_channel(band);
        warnings.push(Issue::new(
            "mesh",
            format!(
                "The mesh needs one channel on every node: set the {} channel; it uses {channel} until then",
                band_name(band)
            ),
        ));
        channel
    });
    let key = mesh
        .key
        .as_ref()
        .and_then(|k| secrets.get(k))
        .map(str::to_owned);
    let Some(key) = key else {
        errors.push(Issue::new(
            "mesh/key",
            "The mesh key isn't set or isn't in the vault",
        ));
        return None;
    };
    Some(Mesh {
        radio,
        dedicated: candidates.len() > 1,
        channel,
        key,
    })
}

pub(crate) const fn band_name(band: Band) -> &'static str {
    match band {
        Band::G2 => "2.4 GHz",
        Band::G5 => "5 GHz",
        Band::G6 => "6 GHz",
        Band::G60 => "60 GHz",
    }
}
