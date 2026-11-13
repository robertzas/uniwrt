//! The planner's inputs from what the controller knows (M6.1 a2, D-111): the nodes' facts and
//! modes, the RF report, the radios' latest scans, the site's radio policy and overrides, and
//! the DFS policy.
//!
//! - **Radios:** each enabled 2.4, 5 and 6 GHz radio (60 GHz isn't planned), with its phy's
//!   channels and widths, what it runs, whether it carries the mesh (an 802.11s interface) and
//!   its clients (stations that aren't mesh peers).
//! - **Pins** (D-111): a radio override's channel, else the band's when fixed; a radio override's
//!   width only (the band's width can't say "auto", and widths are free up to the hardware,
//!   D-110); a radio override's power, else the band's. On a node whose other radio of the band
//!   carries the mesh, the band's channel doesn't pin the radio that doesn't (as render keeps it
//!   off the mesh's channel). A radio the controller may not change (its node in observe mode,
//!   or its Wi-Fi not managed) is pinned to what it runs and `observed`; one that is down is
//!   left out (it neither transmits nor can be changed).
//! - **Links:** the report's inter-node matrix, live mesh links marked.
//! - **Heard:** the report's networks that aren't the site's own (lookalikes take air too), once
//!   per node that heard them, as it heard them loudest.
//! - **Channels:** each 20 MHz channel of the report with its radar hits, and how busy the latest
//!   scans' surveys found it (the busiest; not a radio's live share, which counts its own
//!   traffic).

use std::collections::{BTreeMap, BTreeSet};

use uniwrt_core::{MacAddr, NodeId};
use uniwrt_model::{
    facts::{NodeFacts, RadioFacts, WifiIfaceMode},
    node::NodeMode,
    rf::{DfsPolicy, Scan},
    site::{
        SiteConfig,
        node::{ManagedPackage, NodeConfig},
        wireless::{Channel, TxPower},
    },
    wifi::Band,
};

use super::input::{ChannelStat, Current, Heard, Link, PlanInputs, PlanRadio};
use crate::report::{MatrixSource, NeighborKind, RfReport};

/// What the inputs are gathered from.
#[derive(Debug, Clone, Copy)]
pub struct Sources<'a> {
    /// Each adopted node's latest facts and mode.
    pub nodes: &'a [(NodeId, &'a NodeFacts, NodeMode)],
    pub report: &'a RfReport,
    /// Each radio's latest scan.
    pub scans: &'a [Scan],
    pub site: &'a SiteConfig,
    pub dfs: DfsPolicy,
}

/// The inputs, owned; [`Gathered::inputs`] lends them to [`super::plan`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Gathered {
    pub radios: Vec<PlanRadio>,
    pub links: Vec<Link>,
    pub heard: Vec<Heard>,
    pub channels: Vec<ChannelStat>,
    pub dfs: DfsPolicy,
}

impl Gathered {
    pub fn inputs(&self) -> PlanInputs<'_> {
        PlanInputs {
            radios: &self.radios,
            links: &self.links,
            heard: &self.heard,
            channels: &self.channels,
            dfs: self.dfs,
        }
    }
}

/// Gathers the planner's inputs.
pub fn gather(sources: &Sources<'_>) -> Gathered {
    let mut radios = Vec::new();
    for (node, facts, mode) in sources.nodes {
        let config = sources.site.nodes.iter().find(|c| c.node == *node);
        let managed = *mode == NodeMode::Managed
            && config.is_some_and(|c| c.managed.contains(&ManagedPackage::Wireless));
        radios.extend(
            facts
                .radios
                .iter()
                .filter_map(|radio| plan_radio(sources.site, *node, facts, config, managed, radio)),
        );
    }
    let mut links = Vec::new();
    let mut heard = Vec::new();
    let mut channels = Vec::new();
    for band in &sources.report.bands {
        links.extend(band.matrix.iter().map(|m| Link {
            band: band.band,
            from: m.from,
            to: m.to,
            signal_dbm: m.signal_dbm,
            mesh: m.source == MatrixSource::Mesh,
        }));
        for network in band.networks.iter().filter(|n| n.kind != NeighborKind::Own) {
            let mut loudest: BTreeMap<NodeId, i16> = BTreeMap::new();
            for hearing in &network.heard_by {
                let signal = loudest.entry(hearing.node).or_insert(hearing.signal_dbm);
                *signal = (*signal).max(hearing.signal_dbm);
            }
            heard.extend(loudest.into_iter().map(|(node, signal_dbm)| Heard {
                node,
                band: band.band,
                primary: network.bss.channel,
                center_frequency_mhz: network.bss.center_frequency_mhz,
                width: network.bss.width,
                signal_dbm,
            }));
        }
        let busy = surveyed(sources.scans, band.band);
        channels.extend(band.channels.iter().map(|c| ChannelStat {
            band: band.band,
            channel: c.channel,
            busy_percent: busy.get(&c.frequency_mhz).copied(),
            radar_hits: c.radar_hits,
        }));
    }
    Gathered {
        radios,
        links,
        heard,
        channels,
        dfs: sources.dfs,
    }
}

fn plan_radio(
    site: &SiteConfig,
    node: NodeId,
    facts: &NodeFacts,
    config: Option<&NodeConfig>,
    managed: bool,
    radio: &RadioFacts,
) -> Option<PlanRadio> {
    if radio.band == Band::G60 {
        return None;
    }
    let tweak = config.and_then(|c| c.radios.iter().find(|r| r.radio == radio.name));
    if radio.disabled || (managed && tweak.is_some_and(|t| t.disabled)) {
        return None;
    }
    let current = match (
        radio.up,
        radio.channel,
        radio.width,
        radio.center_frequency_mhz,
    ) {
        (true, Some(channel), Some(width), Some(center_frequency_mhz)) => Some(Current {
            channel,
            width,
            center_frequency_mhz,
            tx_power_dbm: radio.tx_power_dbm,
        }),
        _ => None,
    };
    let phy = facts
        .phys
        .iter()
        .find(|p| Some(&p.name) == radio.phy.as_ref())
        .and_then(|p| p.bands.iter().find(|b| b.band == radio.band));
    let mesh = carries_mesh(radio);
    let mut planned = PlanRadio {
        node,
        radio: radio.name.clone(),
        band: radio.band,
        channels: phy.map(|p| p.channels.clone()).unwrap_or_default(),
        widths: phy.map(|p| p.widths.clone()).unwrap_or_default(),
        current,
        pin: None,
        pin_width: None,
        tx_power: TxPower::Auto,
        mesh,
        clients: clients(facts, radio),
        observed: !managed,
    };
    if !managed {
        let current = current?;
        planned.pin = Some(current.channel);
        planned.pin_width = Some(current.width);
        planned.tx_power = current
            .tx_power_dbm
            .and_then(|dbm| u8::try_from(dbm).ok())
            .map_or(TxPower::Auto, TxPower::Dbm);
        return Some(planned);
    }
    let policy = site.radio.bands.iter().find(|p| p.band == radio.band);
    // Render keeps a tri-band node's other radio of the mesh's band off the mesh's channel.
    let beside_mesh = !mesh
        && facts
            .radios
            .iter()
            .any(|r| r.name != radio.name && r.band == radio.band && carries_mesh(r));
    let band_channel = policy.map(|p| p.channel).filter(|_| !beside_mesh);
    planned.pin = match tweak.and_then(|t| t.channel).or(band_channel) {
        Some(Channel::Fixed(n)) => Some(n),
        Some(Channel::Auto) | None => None,
    };
    planned.pin_width = tweak.and_then(|t| t.width);
    planned.tx_power = tweak
        .and_then(|t| t.tx_power)
        .or(policy.map(|p| p.tx_power))
        .unwrap_or(TxPower::Auto);
    Some(planned)
}

fn carries_mesh(radio: &RadioFacts) -> bool {
    radio.ifaces.iter().any(|i| i.mode == WifiIfaceMode::Mesh)
}

/// The stations on the radio's interfaces that aren't mesh peers.
fn clients(facts: &NodeFacts, radio: &RadioFacts) -> u16 {
    let ifaces: BTreeSet<&str> = radio
        .ifaces
        .iter()
        .filter_map(|i| i.name.as_deref())
        .collect();
    let stations: BTreeSet<MacAddr> = facts
        .stations
        .iter()
        .filter(|s| s.peer_link.is_none() && ifaces.contains(s.iface.as_str()))
        .map(|s| s.mac)
        .collect();
    u16::try_from(stations.len()).unwrap_or(u16::MAX)
}

/// The busiest each 20 MHz channel of `band` was in the scans' surveys, by frequency.
fn surveyed(scans: &[Scan], band: Band) -> BTreeMap<u32, f32> {
    let mut busy: BTreeMap<u32, f32> = BTreeMap::new();
    for survey in scans
        .iter()
        .filter(|s| s.band == band)
        .flat_map(|s| &s.channels)
        .filter(|c| c.dwell_ms.is_some())
    {
        if let Some(percent) = survey.busy_percent {
            let at = busy.entry(survey.frequency_mhz).or_insert(percent);
            *at = at.max(percent);
        }
    }
    busy
}

#[cfg(test)]
#[path = "sources_tests.rs"]
mod tests;
