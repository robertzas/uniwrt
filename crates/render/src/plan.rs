//! What every package's renderer shares, worked out once per node: the VLAN each network has
//! on the bridge, the bridge's ports, the WAN's device, the mesh radio and the wired backhaul,
//! and the issues found.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    Issue, Secrets, Target,
    backhaul::{self, Backhaul},
    ports::{self, Port},
};
use uniwrt_model::{
    facts::{NodeFacts, RadioFacts, Software, SwitchKind, SwitchLayout, WifiFeature},
    node::NodeRole,
    site::{
        SiteConfig,
        adoption::AdoptionPort,
        gateway::{BackupLink, Failover, WanProto},
        network::Network,
        node::NodeConfig,
        wireless::Channel,
    },
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
    /// The gateway's backup line, with failover on.
    pub backup: Option<crate::failover::Backup>,
    /// The gateway's adoption port.
    pub adoption: Option<&'a AdoptionPort>,
    pub mesh: Option<Mesh<'a>>,
    /// The cables batman-adv runs over.
    pub backhaul: Vec<&'a Backhaul>,
    /// The node runs `bat0`: for its mesh radio, its backhaul or both.
    pub bat: bool,
    /// The controller's address as the gateway sees it ([`Target::controller`]).
    pub controller: Option<std::net::IpAddr>,
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
        let nets = nets(site);
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
        let backup = match &site.wan.failover {
            Some(failover) if target.role == NodeRole::Gateway => {
                backup(failover, facts, switch, &mut errors, &mut warnings)
            }
            _ => None,
        };
        let adoption = if target.role == NodeRole::Gateway {
            crate::adoption::plan(site, facts, switch, &mut errors, &mut warnings)
        } else {
            None
        };
        let mut claimed = claimed(site, config, backup.is_some());
        claimed.extend(adoption.map(|a| a.port.as_str()));
        let at = backhaul::Node {
            role: target.role,
            facts,
            config,
            swconfig: switch.is_some(),
            claimed: &claimed,
            nets: &nets,
        };
        let backhaul = backhaul::plan(site, target.backhaul, &at, &mut errors);
        claimed.extend(backhaul::exclusive(&backhaul));
        let bat = mesh.is_some() || !backhaul.is_empty();
        // `bat0` joins the bridge once a radio or a cable carries the LAN over it: while every
        // cable is still an overlay, the trunk carries it, and bridging `bat0` beside it would
        // join the two LANs twice, so each end's batman-adv would learn the other's hosts as
        // its own and keep them after the cable leaves the bridge (D-098).
        let bridged = mesh.is_some() || backhaul::exclusive(&backhaul).next().is_some();
        let node = (target.role, facts, config);
        let ports = ports::ports(node, switch, &nets, bridged, &claimed);
        let mgmt = crate::management_network(site).map_or("lan", |n| n.id.as_str());
        warnings.extend(ports::check(
            node,
            switch,
            &claimed,
            nets.iter().find(|net| net.id() == mgmt),
            &nets,
        ));
        if ports.is_empty() && !bat {
            errors.push(Issue::new(
                "node",
                "The node's board.json names no LAN port",
            ));
        }
        let (wan, wan_password) = if target.role == NodeRole::Gateway {
            let at = (site, secrets, facts, switch);
            uplink(at, &nets, &mut errors, &mut warnings)
        } else {
            (None, None)
        };
        if target.role == NodeRole::Gateway {
            errors.extend(upstream_overlaps(site, facts));
        }
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
            backup,
            adoption,
            mesh,
            backhaul,
            bat,
            controller: target.controller,
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

/// The PPPoE password from the vault, or an error.
fn pppoe_password(
    password: Option<&uniwrt_model::site::SecretRef>,
    secrets: &dyn Secrets,
    errors: &mut Vec<Issue>,
) -> Option<String> {
    let value = password.and_then(|p| secrets.get(p)).map(str::to_owned);
    if value.is_none() {
        errors.push(Issue::new(
            "wan/password",
            "The PPPoE password isn't set or isn't in the vault",
        ));
    }
    value
}

/// The networks whose subnet overlaps the one the gateway's uplinks hold now (`wan`, `wanb`),
/// such as an ISP modem's 192.168.0.0/24: the gateway couldn't route between them.
fn upstream_overlaps(site: &SiteConfig, facts: &NodeFacts) -> Vec<Issue> {
    let upstream: Vec<_> = facts
        .interfaces
        .iter()
        .filter(|i| matches!(i.name.as_str(), "wan" | "wanb"))
        .flat_map(|i| i.ipv4.iter().map(uniwrt_core::Ipv4Net::trunc))
        .collect();
    site.networks
        .iter()
        .filter_map(|network| {
            let subnet = network.ipv4.as_ref()?.address.trunc();
            let wan = upstream
                .iter()
                .find(|up| up.contains(&subnet.network()) || subnet.contains(&up.network()))?;
            Some(Issue::new(
                format!("networks/{}/ipv4/address", network.id),
                format!(
                    "Its subnet {subnet} overlaps {wan}, which the gateway's uplink holds now \
                     (the modem's network): choose another, or the gateway can't route"
                ),
            ))
        })
        .collect()
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

/// The gateway's uplink device, and the PPPoE password it logs in with.
fn uplink(
    at: (&SiteConfig, &dyn Secrets, &NodeFacts, Option<&SwitchLayout>),
    nets: &[Net<'_>],
    errors: &mut Vec<Issue>,
    warnings: &mut Vec<Issue>,
) -> (Option<Wan>, Option<String>) {
    let (site, secrets, facts, switch) = at;
    let wan = wan(site, facts, switch, nets, errors);
    if site.wan.pcp.is_some_and(|pcp| pcp > 0)
        && wan.as_ref().is_some_and(|w| w.vlan_device.is_none())
    {
        warnings.push(Issue::new(
            "wan/pcp",
            "The gateway's switch tags the ISP's VLAN, so the priority isn't set",
        ));
    }
    let password = match &site.wan.proto {
        WanProto::Pppoe { password, .. } => pppoe_password(password.as_ref(), secrets, errors),
        _ => None,
    };
    (wan, password)
}

/// The ports with another job than the bridge's: the device of a raw interface (a wired
/// batman-adv link, a VLAN of its own), and the backup line's. Bridging them too would take
/// them from that job.
/// The networks, in document order, with their VLAN on the bridge: the untagged one takes the
/// lowest VLAN ID no network is tagged with.
pub(crate) fn nets(site: &SiteConfig) -> Vec<Net<'_>> {
    let tagged: BTreeSet<u16> = site.networks.iter().filter_map(|n| n.vlan).collect();
    let untagged_vid = (1..4095).find(|vid| !tagged.contains(vid)).unwrap_or(1);
    site.networks
        .iter()
        .map(|network| Net {
            network,
            vid: network.vlan.unwrap_or(untagged_vid),
        })
        .collect()
}

/// The ports a raw interface of the node's or the gateway's backup line (`backup`) takes.
pub(crate) fn claimed<'a>(
    site: &'a SiteConfig,
    config: &'a NodeConfig,
    backup: bool,
) -> BTreeSet<&'a str> {
    let mut claimed: BTreeSet<&str> = config
        .raw
        .iter()
        .filter(|r| r.package == "network" && r.kind == "interface" && !r.merge)
        .filter_map(|r| match r.options.get("device") {
            Some(uniwrt_model::site::node::RawValue::Option(device)) => Some(device.as_str()),
            _ => None,
        })
        .collect();
    if let Some(BackupLink::Port { port, .. }) = site
        .wan
        .failover
        .as_ref()
        .map(|f| &f.link)
        .filter(|_| backup)
    {
        claimed.insert(port.as_str());
    }
    claimed
}

/// The backup line on the gateway: a port that isn't the WAN's nor one of a swconfig switch
/// (whose ports aren't devices of their own), or a USB tether.
fn backup(
    failover: &Failover,
    facts: &NodeFacts,
    switch: Option<&SwitchLayout>,
    errors: &mut Vec<Issue>,
    warnings: &mut Vec<Issue>,
) -> Option<crate::failover::Backup> {
    let layout = &facts.board.layout;
    if let BackupLink::Port { port, .. } = &failover.link {
        if layout.wan.as_deref() == Some(port.as_str()) {
            errors.push(Issue::new(
                "wan/failover/link",
                "The backup needs a port of its own, not the WAN's",
            ));
            return None;
        }
        if switch.is_some() && layout.lan.iter().any(|lan| lan == port) {
            errors.push(Issue::new(
                "wan/failover/link",
                format!("{port} is a port of the gateway's swconfig switch, which can't be taken out of the LAN on its own"),
            ));
            return None;
        }
        if !facts.devices.is_empty() && !facts.devices.iter().any(|device| &device.name == port) {
            warnings.push(Issue::new(
                "wan/failover/link",
                format!("The gateway has no port {port} now"),
            ));
        }
    }
    Some(crate::failover::Backup::new(failover))
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

/// The node's radios on the mesh's band that aren't turned off: the mesh takes one of them.
pub(crate) fn mesh_radios<'a>(
    site: &SiteConfig,
    config: &NodeConfig,
    facts: &'a NodeFacts,
) -> Vec<&'a RadioFacts> {
    let band = site.mesh.band.unwrap_or(Band::G5);
    let disabled: BTreeMap<&str, bool> = config
        .radios
        .iter()
        .map(|r| (r.radio.as_str(), r.disabled))
        .collect();
    facts
        .radios
        .iter()
        .filter(|r| r.band == band && !disabled.get(r.name.as_str()).copied().unwrap_or(false))
        .collect()
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
    let band = mesh.band.unwrap_or(Band::G5);
    let candidates = mesh_radios(site, config, facts);
    // 802.11s matters only where a radio would carry the mesh: a node without one meshes over
    // its cables alone (features/mesh.md, Wired backhaul).
    let caps = &facts.capabilities;
    let mut missing = Vec::new();
    if !caps.software.contains(&Software::BatmanAdv) {
        missing.push("batman-adv (kmod-batman-adv)");
    }
    if !candidates.is_empty() && !caps.wifi.contains(&WifiFeature::Mesh) {
        missing.push("802.11s with SAE (the full wpad)");
    }
    if !missing.is_empty() {
        errors.push(Issue::new(
            "mesh",
            format!("The node lacks {} for the mesh", missing.join(" and ")),
        ));
        return None;
    }
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
