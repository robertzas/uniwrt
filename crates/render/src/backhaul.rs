//! The wired backhaul (features/mesh.md, Wired backhaul; D-094): batman-adv over a cable
//! between two nodes, on the site's backhaul VLAN (`<port>.<vid>`, MTU 1532). An apply stages
//! it make-before-break: first an overlay, the port still a trunk, so the cable carries the
//! LAN whichever end comes over first; then the port carries batman-adv's VLAN alone and
//! leaves the bridge. On the gateway, the backhauls' interfaces form a firewall zone of their
//! own that accepts only what the agent's MTU probe needs (`ICMPv6` echo and neighbour
//! discovery); APs run no firewall.

use std::collections::BTreeSet;

use uniwrt_model::{
    facts::{DeviceKind, NodeFacts, Software, SwitchKind},
    node::NodeRole,
    site::{SiteConfig, node::NodeConfig},
};
use uniwrt_uci::{Package, Section, Value};

use crate::{
    Issue, Target,
    ext::SectionExt,
    plan::{self, BAT, Net, Plan},
    ports,
};

/// A cable to another node that batman-adv runs over, as the apply stages it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backhaul {
    /// The node's Ethernet port at its end.
    pub port: String,
    pub stage: BackhaulStage,
}

/// How far a cable has come over to batman-adv.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackhaulStage {
    /// batman-adv on the backhaul VLAN beside the networks: the port stays a trunk.
    Overlay,
    /// The port carries the backhaul VLAN alone, out of the bridge.
    Exclusive,
}

/// The name of a backhaul's interface starts with this, followed by its port.
pub const BACKHAUL_PREFIX: &str = "uw_bh_";

/// The gateway's firewall zone of the backhauls' interfaces.
pub(crate) const BACKHAUL_ZONE: &str = "uw_bh";

/// batman-adv's header needs 32 bytes above the 1500 the bridge carries.
pub(crate) const MESH_MTU: &str = "1532";

/// The interface of the backhaul on `port`.
pub fn backhaul_interface(port: &str) -> String {
    let port = port.replace(|c: char| !c.is_ascii_alphanumeric(), "_");
    format!("{BACKHAUL_PREFIX}{port}")
}

/// What [`plan`] checks a backhaul against.
pub(crate) struct Node<'a, 'n> {
    pub role: NodeRole,
    pub facts: &'a NodeFacts,
    pub config: &'a NodeConfig,
    pub swconfig: bool,
    pub claimed: &'a BTreeSet<&'a str>,
    pub nets: &'a [Net<'n>],
}

/// The backhauls asked for that the node can carry; each one it can't is an error.
pub(crate) fn plan<'a>(
    site: &SiteConfig,
    asked: &'a [Backhaul],
    node: &Node<'_, '_>,
    errors: &mut Vec<Issue>,
) -> Vec<&'a Backhaul> {
    if asked.is_empty() {
        return Vec::new();
    }
    if !site.mesh.enabled || !site.mesh.wired {
        errors.push(Issue::new(
            "mesh/wired",
            "A wired backhaul was asked for while the mesh's wired links are off",
        ));
        return Vec::new();
    }
    // The mesh's own issue says what is missing.
    if !node
        .facts
        .capabilities
        .software
        .contains(&Software::BatmanAdv)
    {
        return Vec::new();
    }
    let mut taken = BTreeSet::new();
    let mut planned = Vec::new();
    for backhaul in asked {
        if !taken.insert(backhaul.port.as_str()) {
            continue;
        }
        match node.refusal(&backhaul.port) {
            Some(problem) => errors.push(Issue::new("node", problem)),
            None => planned.push(backhaul),
        }
    }
    planned
}

impl Node<'_, '_> {
    /// Why `port` can't carry a wired backhaul, if it can't.
    fn refusal(&self, port: &str) -> Option<String> {
        let layout = &self.facts.board.layout;
        let known =
            layout.lan.iter().any(|lan| lan == port)
                || layout.wan.as_deref() == Some(port)
                || self.facts.devices.iter().any(|device| {
                    device.name == port && matches!(device.kind, DeviceKind::Ethernet)
                });
        let profile = ports::profile(self.config, port);
        if self.swconfig {
            Some(format!(
                "{port} is a port of a swconfig switch, which can't carry the wired backhaul yet"
            ))
        } else if !known {
            Some(format!(
                "The node has no port {port} for the wired backhaul"
            ))
        } else if self.role == NodeRole::Gateway && layout.wan.as_deref() == Some(port) {
            Some(format!("{port} is the gateway's WAN port"))
        } else if self.claimed.contains(port) {
            Some(format!("{port} already has another job"))
        } else if profile.is_some_and(|p| !p.backhaul) {
            Some(format!("{port} is kept out of the wired backhaul"))
        } else if profile
            .is_some_and(|p| ports::carried(Some(p), self.nets) != ports::carried(None, self.nets))
        {
            Some(format!(
                "{port} isn't a trunk, so it can't carry the wired backhaul"
            ))
        } else {
            None
        }
    }
}

/// Which of `ports` the node can carry a wired backhaul on, as [`render`](crate::render) checks
/// them: none while the mesh or its wired links are off, or the node lacks batman-adv. The
/// apply asks only for these, so that a cable it finds never refuses a node.
pub fn backhaul_ports<'p>(
    site: &SiteConfig,
    target: Target<'_>,
    ports: impl IntoIterator<Item = &'p str>,
) -> BTreeSet<&'p str> {
    let facts = target.facts;
    if !site.mesh.enabled
        || !site.mesh.wired
        || !facts.capabilities.software.contains(&Software::BatmanAdv)
    {
        return BTreeSet::new();
    }
    let default = NodeConfig::new(target.node);
    let config = site
        .nodes
        .iter()
        .find(|config| config.node == target.node)
        .unwrap_or(&default);
    let nets = plan::nets(site);
    // A backup line's port counts as taken whether or not it renders.
    let claimed = plan::claimed(site, config, target.role == NodeRole::Gateway);
    let node = Node {
        role: target.role,
        facts,
        config,
        swconfig: facts.capabilities.switch == Some(SwitchKind::Swconfig),
        claimed: &claimed,
        nets: &nets,
    };
    ports
        .into_iter()
        .filter(|port| node.refusal(port).is_none())
        .collect()
}

/// The backhauls a node's `network` holds, at the stage each has come to: a port still in the
/// bridge is an overlay. The VLAN they run on may differ from the site's now.
pub fn held_backhauls(network: &Package) -> Vec<Backhaul> {
    let bridged: BTreeSet<&str> = network
        .sections_of("device")
        .filter(|section| section.option("name") == Some(plan::BRIDGE))
        .filter_map(|section| section.get("ports").map(Value::items))
        .flatten()
        .map(String::as_str)
        .collect();
    network
        .sections_of("interface")
        .filter(|section| {
            section
                .name
                .as_deref()
                .is_some_and(|name| name.starts_with(BACKHAUL_PREFIX))
                && section.option("proto") == Some("batadv_hardif")
        })
        .filter_map(|section| {
            let (port, vid) = section.option("device")?.rsplit_once('.')?;
            vid.parse::<u16>().ok()?;
            Some(Backhaul {
                port: port.to_owned(),
                stage: if bridged.contains(port) {
                    BackhaulStage::Overlay
                } else {
                    BackhaulStage::Exclusive
                },
            })
        })
        .collect()
}

/// The ports the backhaul takes out of the bridge.
pub(crate) fn exclusive<'a>(planned: &[&'a Backhaul]) -> impl Iterator<Item = &'a str> {
    planned
        .iter()
        .filter(|b| b.stage == BackhaulStage::Exclusive)
        .map(|b| b.port.as_str())
}

/// Each backhaul's port and VLAN device at batman-adv's MTU, and its hard interface.
pub(crate) fn sections(plan: &Plan<'_>) -> Vec<Section> {
    let vid = plan.site.mesh.wired_vlan;
    plan.backhaul
        .iter()
        .flat_map(|backhaul| {
            let port = backhaul.port.as_str();
            let vlan = format!("{port}.{vid}");
            [
                Section::anonymous("device")
                    .with("name", port)
                    .with("mtu", MESH_MTU),
                Section::anonymous("device")
                    .with("type", "8021q")
                    .with("ifname", port)
                    .with("vid", vid.to_string())
                    .with("name", vlan.as_str())
                    .with("mtu", MESH_MTU),
                Section::named("interface", backhaul_interface(port))
                    .with("proto", "batadv_hardif")
                    .with("master", BAT)
                    .with("device", vlan.as_str()),
            ]
        })
        .collect()
}

/// The gateway's zone of the backhauls, and the rule that lets the agent's MTU probe through:
/// its multicast echo requests, the peers' replies (which conntrack doesn't tie to a request
/// sent to a group) and the neighbour discovery the replies need. batman-adv's own frames
/// aren't IP, so the firewall never sees them.
pub(crate) fn firewall(plan: &Plan<'_>) -> Vec<Section> {
    if plan.backhaul.is_empty() {
        return Vec::new();
    }
    let interfaces: Vec<String> = plan
        .backhaul
        .iter()
        .map(|backhaul| backhaul_interface(&backhaul.port))
        .collect();
    vec![
        Section::anonymous("zone")
            .with("name", BACKHAUL_ZONE)
            .list("network", interfaces)
            .with("input", "REJECT")
            .with("output", "ACCEPT")
            .with("forward", "REJECT"),
        Section::anonymous("rule")
            .with("name", "Allow-uw_bh-MTU-probe")
            .with("src", BACKHAUL_ZONE)
            .with("proto", "icmp")
            .with(
                "icmp_type",
                [
                    "echo-request",
                    "echo-reply",
                    "neighbour-solicitation",
                    "neighbour-advertisement",
                ],
            )
            .with("family", "ipv6")
            .with("target", "ACCEPT"),
    ]
}
