//! Building the [`RfReport`] from the nodes' facts, their radios' latest scans and the radar
//! history.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use uniwrt_core::{MacAddr, NodeId, Timestamp};
use uniwrt_model::{
    facts::{DfsState, NodeFacts, RadioFacts, WifiIfaceMode},
    rf::Scan,
    wifi::{Band, ChannelWidth},
};

use crate::{
    radar::RadarHit,
    report::{
        BandReport, ChannelReport, ChannelUse, Hearing, MatrixEntry, MatrixSource, Neighbor,
        NeighborKind, Overlap, RadioReport, RfReport,
    },
    span::{Span, weight},
};

/// What the report is built from.
#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    /// Each adopted node's latest facts.
    pub nodes: &'a [(NodeId, &'a NodeFacts)],
    /// The latest scan of each radio.
    pub scans: &'a [Scan],
    /// The radar hits of the history kept.
    pub radar: &'a [RadarHit],
}

/// The site's own BSSs and names.
struct Own {
    bssids: HashMap<MacAddr, (NodeId, String)>,
    names: BTreeSet<String>,
}

impl Own {
    fn new(nodes: &[(NodeId, &NodeFacts)]) -> Self {
        let mut own = Self {
            bssids: HashMap::new(),
            names: BTreeSet::new(),
        };
        for (node, facts) in nodes {
            for radio in &facts.radios {
                for iface in &radio.ifaces {
                    if let Some(bssid) = iface.bssid {
                        own.bssids.insert(bssid, (*node, radio.name.clone()));
                    }
                    own.names
                        .extend(iface.ssid.iter().chain(&iface.mesh_id).cloned());
                }
            }
        }
        own
    }

    fn kind(&self, bss: &uniwrt_model::rf::Bss) -> NeighborKind {
        if self.bssids.contains_key(&bss.bssid) {
            NeighborKind::Own
        } else if bss
            .ssid
            .iter()
            .chain(&bss.mesh_id)
            .any(|name| self.names.contains(name))
        {
            NeighborKind::Lookalike
        } else {
            NeighborKind::Foreign
        }
    }
}

/// Builds the report; `vendor` names a BSSID's maker.
pub fn report(inputs: &Inputs<'_>, vendor: impl Fn(MacAddr) -> Option<String>) -> RfReport {
    let own = Own::new(inputs.nodes);
    let mut bands: BTreeSet<Band> = inputs.scans.iter().map(|s| s.band).collect();
    for (_, facts) in inputs.nodes {
        bands.extend(facts.radios.iter().filter(|r| !r.disabled).map(|r| r.band));
    }
    bands.remove(&Band::G60);
    RfReport {
        bands: bands
            .into_iter()
            .map(|band| band_report(inputs, &own, band, &vendor))
            .collect(),
    }
}

fn band_report(
    inputs: &Inputs<'_>,
    own: &Own,
    band: Band,
    vendor: &impl Fn(MacAddr) -> Option<String>,
) -> BandReport {
    let mut networks = neighbors(inputs.scans, own, band, vendor);
    let matrix = matrix(inputs.nodes, own, band, &networks);
    let mut radios = Vec::new();
    for (node, facts) in inputs.nodes {
        for radio in facts
            .radios
            .iter()
            .filter(|r| r.band == band && !r.disabled)
        {
            radios.push(radio_report(*node, radio, &mut networks));
        }
    }
    radios.sort_by(|a, b| (a.node, &a.radio).cmp(&(b.node, &b.radio)));
    let channels = channels(inputs, band, &networks, &radios);
    networks.sort_by(|a, b| {
        b.impact
            .total_cmp(&a.impact)
            .then(b.bss.signal_dbm.cmp(&a.bss.signal_dbm))
            .then(a.bss.bssid.cmp(&b.bss.bssid))
    });
    BandReport {
        band,
        radios,
        channels,
        networks,
        matrix,
    }
}

/// Every BSS the band's scans heard, as heard best, with who heard it.
fn neighbors(
    scans: &[Scan],
    own: &Own,
    band: Band,
    vendor: &impl Fn(MacAddr) -> Option<String>,
) -> Vec<Neighbor> {
    let mut found: BTreeMap<MacAddr, Neighbor> = BTreeMap::new();
    for scan in scans.iter().filter(|s| s.band == band) {
        for bss in &scan.bsss {
            let hearing = Hearing {
                node: scan.node,
                radio: scan.radio.clone(),
                signal_dbm: bss.signal_dbm,
                at: scan.at,
            };
            let neighbor = found.entry(bss.bssid).or_insert_with(|| {
                let owner = own.bssids.get(&bss.bssid);
                Neighbor {
                    bss: bss.clone(),
                    kind: own.kind(bss),
                    node: owner.map(|(node, _)| *node),
                    radio: owner.map(|(_, radio)| radio.clone()),
                    vendor: vendor(bss.bssid),
                    heard_by: Vec::new(),
                    impact: 0.0,
                }
            });
            if bss.signal_dbm > neighbor.bss.signal_dbm {
                neighbor.bss = bss.clone();
            }
            neighbor.heard_by.push(hearing);
        }
    }
    let mut found: Vec<Neighbor> = found.into_values().collect();
    for neighbor in &mut found {
        neighbor
            .heard_by
            .sort_by(|a, b| b.signal_dbm.cmp(&a.signal_dbm).then(a.node.cmp(&b.node)));
    }
    found
}

/// How the nodes hear each other: live mesh links first, else the strongest of each other's
/// BSSs the latest scans heard.
fn matrix(
    nodes: &[(NodeId, &NodeFacts)],
    own: &Own,
    band: Band,
    networks: &[Neighbor],
) -> Vec<MatrixEntry> {
    let mut entries: BTreeMap<(NodeId, NodeId), MatrixEntry> = BTreeMap::new();
    for neighbor in networks.iter().filter(|n| n.kind == NeighborKind::Own) {
        let Some(to) = neighbor.node else { continue };
        for hearing in neighbor.heard_by.iter().filter(|h| h.node != to) {
            let entry = MatrixEntry {
                from: hearing.node,
                to,
                signal_dbm: hearing.signal_dbm,
                source: MatrixSource::Scan,
                at: hearing.at,
            };
            entries
                .entry((hearing.node, to))
                .and_modify(|e| {
                    if entry.signal_dbm > e.signal_dbm {
                        *e = entry.clone();
                    }
                })
                .or_insert(entry);
        }
    }
    let mut mesh: BTreeMap<(NodeId, NodeId), MatrixEntry> = BTreeMap::new();
    for (from, facts) in nodes {
        let ifaces: BTreeSet<&str> = facts
            .radios
            .iter()
            .filter(|r| r.band == band)
            .flat_map(|r| &r.ifaces)
            .filter(|i| i.mode == WifiIfaceMode::Mesh)
            .filter_map(|i| i.name.as_deref())
            .collect();
        for station in facts
            .stations
            .iter()
            .filter(|s| ifaces.contains(s.iface.as_str()))
        {
            let (Some((to, _)), Some(signal_dbm)) =
                (own.bssids.get(&station.mac), station.signal_dbm)
            else {
                continue;
            };
            let entry = MatrixEntry {
                from: *from,
                to: *to,
                signal_dbm,
                source: MatrixSource::Mesh,
                at: facts.collected_at,
            };
            mesh.entry((*from, *to))
                .and_modify(|e| {
                    if signal_dbm > e.signal_dbm {
                        *e = entry.clone();
                    }
                })
                .or_insert(entry);
        }
    }
    entries.extend(mesh);
    entries.into_values().collect()
}

fn radio_span(radio: &RadioFacts) -> Option<Span> {
    let width = radio.width.unwrap_or(ChannelWidth::Mhz20);
    let center = radio.center_frequency_mhz.or(radio.frequency_mhz)?;
    Some(Span::new(center, width))
}

fn bss_span(bss: &uniwrt_model::rf::Bss) -> Span {
    Span::new(bss.center_frequency_mhz, bss.width)
}

/// A radio and the networks its node hears on its channel; adds their impact to the networks.
fn radio_report(node: NodeId, radio: &RadioFacts, networks: &mut [Neighbor]) -> RadioReport {
    let mut on_channel = Vec::new();
    if let (Some(span), true) = (radio_span(radio), radio.up) {
        for neighbor in networks.iter_mut().filter(|n| n.kind != NeighborKind::Own) {
            let Some(signal_dbm) = neighbor
                .heard_by
                .iter()
                .filter(|h| h.node == node)
                .map(|h| h.signal_dbm)
                .max()
            else {
                continue;
            };
            let overlap = span.overlap(bss_span(&neighbor.bss));
            if overlap <= 0.0 {
                continue;
            }
            let impact = weight(signal_dbm) * overlap;
            neighbor.impact = round(neighbor.impact + impact);
            on_channel.push(Overlap {
                bssid: neighbor.bss.bssid,
                co_channel: Some(neighbor.bss.frequency_mhz) == radio.frequency_mhz,
                overlap: round(overlap),
                signal_dbm,
                impact: round(impact),
            });
        }
    }
    on_channel.sort_by(|a, b| b.impact.total_cmp(&a.impact).then(a.bssid.cmp(&b.bssid)));
    RadioReport {
        node,
        radio: radio.name.clone(),
        up: radio.up,
        channel: radio.channel,
        frequency_mhz: radio.frequency_mhz,
        center_frequency_mhz: radio.center_frequency_mhz,
        width: radio.width,
        tx_power_dbm: radio.tx_power_dbm,
        busy_percent: radio.channel_busy_percent,
        noise_dbm: radio.noise_dbm,
        interference: round(on_channel.iter().map(|o| o.impact).sum()),
        on_channel,
    }
}

/// What the phys and the surveys say of a channel: whether it may be used, its DFS state, how
/// busy and noisy it was.
#[derive(Default, Clone, Copy)]
struct Channel {
    allowed: bool,
    dfs: bool,
    dfs_state: Option<DfsState>,
    busy_percent: Option<f32>,
    noise_dbm: Option<i16>,
}

fn channels(
    inputs: &Inputs<'_>,
    band: Band,
    networks: &[Neighbor],
    radios: &[RadioReport],
) -> Vec<ChannelReport> {
    let mut found: BTreeMap<u32, Channel> = BTreeMap::new();
    for (_, facts) in inputs.nodes {
        for phy_band in facts
            .phys
            .iter()
            .flat_map(|p| &p.bands)
            .filter(|b| b.band == band)
        {
            for c in &phy_band.channels {
                let channel = found.entry(c.frequency_mhz).or_default();
                channel.allowed = true;
                channel.dfs |= c.dfs;
                channel.dfs_state = worst(channel.dfs_state, c.dfs_state);
            }
        }
    }
    for scan in inputs.scans.iter().filter(|s| s.band == band) {
        for survey in &scan.channels {
            let channel = found.entry(survey.frequency_mhz).or_default();
            if survey.dwell_ms.is_some() {
                channel.busy_percent = max(channel.busy_percent, survey.busy_percent);
            }
            channel.noise_dbm = min(channel.noise_dbm, survey.noise_dbm);
        }
    }
    for neighbor in networks {
        found.entry(neighbor.bss.frequency_mhz).or_default();
    }
    // A radio's own channel is measured live: newer than any scan.
    let mut live: BTreeMap<u32, (Option<f32>, Option<i16>)> = BTreeMap::new();
    for radio in radios.iter().filter(|r| r.up) {
        if let Some(freq) = radio.frequency_mhz {
            let (busy, noise) = live.entry(freq).or_default();
            *busy = max(*busy, radio.busy_percent);
            *noise = min(*noise, radio.noise_dbm);
        }
    }
    found
        .into_iter()
        .filter_map(|(frequency_mhz, mut channel)| {
            let number = band.channel(frequency_mhz)?;
            if let Some((busy, noise)) = live.get(&frequency_mhz) {
                channel.busy_percent = busy.or(channel.busy_percent);
                channel.noise_dbm = noise.or(channel.noise_dbm);
            }
            Some(channel_report(
                inputs,
                number,
                frequency_mhz,
                channel,
                networks,
                radios,
            ))
        })
        .collect()
}

fn channel_report(
    inputs: &Inputs<'_>,
    number: u16,
    frequency_mhz: u32,
    channel: Channel,
    networks: &[Neighbor],
    radios: &[RadioReport],
) -> ChannelReport {
    let span = Span::channel(frequency_mhz);
    let hits: Vec<Timestamp> = inputs
        .radar
        .iter()
        .filter(|hit| span.overlap(hit.span) > 0.0)
        .map(|hit| hit.at)
        .collect();
    let mut count = 0;
    let mut strongest_dbm = None;
    let mut pressure = 0.0;
    for neighbor in networks.iter().filter(|n| n.kind != NeighborKind::Own) {
        let overlap = span.overlap(bss_span(&neighbor.bss));
        if overlap > 0.0 {
            count += 1;
            strongest_dbm = strongest_dbm.max(Some(neighbor.bss.signal_dbm));
            pressure += weight(neighbor.bss.signal_dbm) * overlap;
        }
    }
    let ours = radios
        .iter()
        .filter(|r| r.up)
        .filter_map(|r| {
            let center = r.center_frequency_mhz.or(r.frequency_mhz)?;
            let radio_span = Span::new(center, r.width.unwrap_or(ChannelWidth::Mhz20));
            (span.overlap(radio_span) > 0.0).then(|| ChannelUse {
                node: r.node,
                radio: r.radio.clone(),
                primary: r.frequency_mhz == Some(frequency_mhz),
            })
        })
        .collect();
    ChannelReport {
        channel: number,
        frequency_mhz,
        allowed: channel.allowed,
        dfs: channel.dfs,
        dfs_state: channel.dfs_state,
        radar_hits: u32::try_from(hits.len()).unwrap_or(u32::MAX),
        last_radar: hits.into_iter().max(),
        networks: count,
        strongest_dbm,
        pressure: round(pressure),
        busy_percent: channel.busy_percent,
        noise_dbm: channel.noise_dbm,
        ours,
    }
}

/// Radar seen beats not yet checked, which beats checked.
fn worst(a: Option<DfsState>, b: Option<DfsState>) -> Option<DfsState> {
    let rank = |s: &Option<DfsState>| match s {
        Some(DfsState::Unavailable) => 3,
        Some(DfsState::Usable) => 2,
        Some(DfsState::Available) => 1,
        None => 0,
    };
    if rank(&b) > rank(&a) { b } else { a }
}

fn max(a: Option<f32>, b: Option<f32>) -> Option<f32> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        _ => a.or(b),
    }
}

fn min(a: Option<i16>, b: Option<i16>) -> Option<i16> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        _ => a.or(b),
    }
}

/// Two decimals, so that reports compare and serialize steadily.
fn round(value: f32) -> f32 {
    (value * 100.0).round() / 100.0
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
