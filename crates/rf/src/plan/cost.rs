//! The objective (features/wifi.md, RF planner: Objective) over one band: what each radio
//! suffers from the site's other radios and foreign networks, DFS, moving, and the width it gives
//! up. Every term is 0 or more, so a band's cost is too, and `improvement` is a share.
//!
//! The search runs over *units*: a radio, or the band's mesh radios together (one channel and
//! width). A unit's choice costs its radios' own terms (`unary`) plus, against each other unit's
//! choice, what each side's radios suffer from the other's (`pair`).

use std::collections::{BTreeMap, BTreeSet};

use uniwrt_model::{
    facts::Channel,
    rf::DfsPolicy,
    wifi::{Band, ChannelWidth},
};

use super::{
    blocks::{self, WEATHER_MHZ},
    input::{Current, PlanInputs, PlanRadio},
    output::Breakdown,
};
use crate::span::{Span, weight};

/// A foreign network a radio's node hears: its span, primary channel and weight.
type Heard = Vec<(Span, u16, f32)>;

/// How much one of the site's own radios on the same node disturbs another on the same band:
/// far more than any neighbor.
const SAME_NODE: f32 = 5.0;
/// A channel busy all the time counts as half a loud co-channel network.
const BUSY: f32 = 0.5;
/// Any DFS channel: radar may move the radio, with a CAC on each restart.
const DFS: f32 = 0.05;
/// Each radar hit in the history, on any channel of the block; capped.
const RADAR_HIT: f32 = 0.25;
const RADAR_CAP: f32 = 2.0;
/// Moving onto a DFS channel: a minute's silence for the CAC (ten on weather channels).
const CAC: f32 = 0.1;
const WEATHER_CAC: f32 = 0.5;
/// The mesh's DFS risk counts twice: radar drops the backhaul.
const MESH_DFS: f32 = 2.0;
/// Moving at all (a CSA or a restart, the clients' hiccup).
const CHANGE: f32 = 0.1;
/// Each halving of the width below the widest the radio may use.
const WIDTH: f32 = 0.3;
/// Clients beyond this don't add to a radio's load.
const LOAD_CLIENTS: u16 = 20;
/// The primary channel of a block stays as it is unless another is this much quieter.
const PRIMARY_SLACK: f32 = 0.05;

/// A channel a unit may take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Choice {
    pub primary: u16,
    pub width: ChannelWidth,
    pub span: Span,
}

impl Choice {
    pub(crate) fn of(current: &Current) -> Self {
        Self {
            primary: current.channel,
            width: current.width,
            span: Span::new(current.center_frequency_mhz, current.width),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Unit {
    /// Indexes into the band's radios.
    pub radios: Vec<usize>,
    pub choices: Vec<Choice>,
    pub widest: ChannelWidth,
    pub pin: Option<(usize, u16)>,
    /// Its radios have no channel to choose and keep the current one.
    pub stuck: bool,
}

/// One band's radios and what they hear, ready to cost.
pub(crate) struct Model<'a> {
    pub radios: Vec<&'a PlanRadio>,
    pub units: Vec<Unit>,
    /// The unit of each radio.
    pub unit_of: Vec<usize>,
    load: Vec<f32>,
    /// `disturb[i][j]`: how much radio `j` disturbs radio `i` per share of overlap, at the
    /// planned powers (0 within a unit).
    disturb: Vec<Vec<f32>>,
    /// The same at the current powers, for the cost as it runs.
    disturb_now: Vec<Vec<f32>>,
    /// The foreign networks each radio's node hears.
    heard: Vec<Heard>,
    busy: BTreeMap<u16, f32>,
    radar: BTreeMap<u16, u32>,
}

impl<'a> Model<'a> {
    /// `radios` are the band's, in plan order, with each one's planned power shift in dB;
    /// `g2` is the 2.4 GHz channel set planned with.
    pub(crate) fn new(
        inputs: &PlanInputs<'_>,
        band: Band,
        radios: Vec<(&'a PlanRadio, i16)>,
        g2: Option<&[u16]>,
    ) -> Self {
        let load = radios
            .iter()
            .map(|(r, _)| 1.0 + f32::from(r.clients.min(LOAD_CLIENTS)) / 10.0)
            .collect::<Vec<_>>();
        let heard = radios
            .iter()
            .map(|(r, _)| {
                inputs
                    .heard
                    .iter()
                    .filter(|h| h.band == band && h.node == r.node)
                    .map(|h| {
                        (
                            Span::new(h.center_frequency_mhz, h.width),
                            h.primary,
                            weight(h.signal_dbm),
                        )
                    })
                    .collect()
            })
            .collect::<Vec<_>>();
        let (units, unit_of) = units(band, &radios, &heard, inputs.dfs, g2);
        let rssi = |i: usize, j: usize| {
            inputs
                .links
                .iter()
                .find(|l| l.band == band && l.from == radios[i].0.node && l.to == radios[j].0.node)
                .map(|l| l.signal_dbm)
        };
        let matrix = |shifted: bool| {
            (0..radios.len())
                .map(|i| {
                    (0..radios.len())
                        .map(|j| {
                            if i == j || unit_of[i] == unit_of[j] {
                                0.0
                            } else if radios[i].0.node == radios[j].0.node {
                                SAME_NODE
                            } else {
                                rssi(i, j).map_or(0.0, |s| {
                                    let s = if shifted { s + radios[j].1 } else { s };
                                    load[i] * weight(s)
                                })
                            }
                        })
                        .collect()
                })
                .collect()
        };
        let stats = inputs.channels.iter().filter(|c| c.band == band);
        Self {
            disturb: matrix(true),
            disturb_now: matrix(false),
            radios: radios.into_iter().map(|(r, _)| r).collect(),
            units,
            unit_of,
            load,
            heard,
            busy: stats
                .clone()
                .filter_map(|c| Some((c.channel, c.busy_percent? / 100.0)))
                .collect(),
            radar: stats.map(|c| (c.channel, c.radar_hits)).collect(),
        }
    }

    /// Radio `i`'s cost on `choice` with the others on `spans` (`None`: down), at the planned
    /// powers or the current ones.
    pub(crate) fn radio(
        &self,
        i: usize,
        choice: Choice,
        spans: &[Option<Span>],
        planned: bool,
    ) -> Breakdown {
        let disturb = if planned {
            &self.disturb
        } else {
            &self.disturb_now
        };
        let own = spans
            .iter()
            .enumerate()
            .filter_map(|(j, s)| Some(disturb[i][j] * choice.span.overlap((*s)?)))
            .sum();
        let mut b = self.unary(i, choice, planned);
        b.own = own;
        b.total += own;
        b
    }

    /// Radio `i`'s terms that don't depend on the others; `planned` counts moving.
    fn unary(&self, i: usize, choice: Choice, planned: bool) -> Breakdown {
        let radio = self.radios[i];
        let subs = sub_channels(choice.span, &radio.channels);
        let foreign: f32 = self.heard[i]
            .iter()
            .map(|(span, _, w)| w * choice.span.overlap(*span))
            .sum();
        #[allow(clippy::cast_precision_loss)]
        let busy = if subs.is_empty() {
            0.0
        } else {
            subs.iter()
                .map(|c| self.busy.get(&c.number).copied().unwrap_or(0.0))
                .sum::<f32>()
                / subs.len() as f32
        };
        let external = self.load[i] * (foreign + BUSY * busy);
        let current = radio.current.map(|c| Choice::of(&c));
        let mut dfs = 0.0;
        if subs.iter().any(|c| c.dfs) {
            #[allow(clippy::cast_precision_loss)]
            let hits = subs
                .iter()
                .map(|c| self.radar.get(&c.number).copied().unwrap_or(0))
                .sum::<u32>() as f32;
            dfs = DFS + (RADAR_HIT * hits).min(RADAR_CAP);
            let held = current.map_or_else(Vec::new, |c| sub_channels(c.span, &radio.channels));
            let new: Vec<_> = subs.iter().filter(|c| c.dfs && !held.contains(c)).collect();
            if planned && !new.is_empty() {
                dfs += if new.iter().any(|c| WEATHER_MHZ.contains(&c.frequency_mhz)) {
                    WEATHER_CAC
                } else {
                    CAC
                };
            }
            if radio.mesh {
                dfs *= MESH_DFS;
            }
        }
        let change = match current {
            Some(c) if planned && (c.primary, c.width) != (choice.primary, choice.width) => CHANGE,
            _ => 0.0,
        };
        let halvings = (self.units[self.unit_of[i]].widest.mhz() / choice.width.mhz().max(1))
            .max(1)
            .ilog2();
        #[allow(clippy::cast_precision_loss)]
        let width = WIDTH * self.load[i] * halvings as f32;
        Breakdown {
            own: 0.0,
            external,
            dfs,
            change,
            width,
            total: external + dfs + change + width,
        }
    }

    /// The search's tables: each unit's cost per choice, and `pair[u][v]`, how much unit `v`'s
    /// radios disturb unit `u`'s per share of overlap.
    pub(crate) fn tables(&self) -> (Vec<Vec<f32>>, Vec<Vec<f32>>) {
        let unary = self
            .units
            .iter()
            .map(|u| {
                u.choices
                    .iter()
                    .map(|c| {
                        u.radios
                            .iter()
                            .map(|i| self.unary(*i, *c, true).total)
                            .sum()
                    })
                    .collect()
            })
            .collect();
        let n = self.units.len();
        let mut pair = vec![vec![0.0; n]; n];
        for i in 0..self.radios.len() {
            for j in 0..self.radios.len() {
                pair[self.unit_of[i]][self.unit_of[j]] += self.disturb[i][j];
            }
        }
        (unary, pair)
    }
}

/// The radio's channels whose 20 MHz lie inside `span`.
pub(crate) fn sub_channels(span: Span, channels: &[Channel]) -> Vec<Channel> {
    channels
        .iter()
        .filter(|c| c.frequency_mhz > span.low && c.frequency_mhz < span.high)
        .copied()
        .collect()
}

/// Groups the band's mesh radios into one unit, and lists each unit's choices.
fn units(
    band: Band,
    radios: &[(&PlanRadio, i16)],
    heard: &[Heard],
    dfs: DfsPolicy,
    g2: Option<&[u16]>,
) -> (Vec<Unit>, Vec<usize>) {
    let mut members: Vec<Vec<usize>> = Vec::new();
    let mesh: Vec<usize> = (0..radios.len()).filter(|i| radios[*i].0.mesh).collect();
    if !mesh.is_empty() {
        members.push(mesh);
    }
    members.extend(
        (0..radios.len())
            .filter(|i| !radios[*i].0.mesh)
            .map(|i| vec![i]),
    );
    members.sort_by_key(|m| m[0]);
    let mut unit_of = vec![0; radios.len()];
    let units = members
        .into_iter()
        .enumerate()
        .map(|(u, radios_of)| {
            for i in &radios_of {
                unit_of[*i] = u;
            }
            unit(band, radios, heard, radios_of, dfs, g2)
        })
        .collect();
    (units, unit_of)
}

fn unit(
    band: Band,
    radios: &[(&PlanRadio, i16)],
    heard: &[Heard],
    members: Vec<usize>,
    dfs: DfsPolicy,
    g2: Option<&[u16]>,
) -> Unit {
    let of = |i: usize| radios[i].0;
    let pin = members.iter().find_map(|i| of(*i).pin.map(|p| (*i, p)));
    let mut widths: BTreeSet<ChannelWidth> = of(members[0]).widths.clone();
    for i in &members[1..] {
        widths = widths.intersection(&of(*i).widths).copied().collect();
    }
    if band == Band::G2 {
        widths = BTreeSet::from([ChannelWidth::Mhz20]);
    }
    if let Some(w) = members.iter().find_map(|i| of(*i).pin_width) {
        widths = BTreeSet::from([w]);
    }
    let mut choices = Vec::new();
    for width in &widths {
        for block in blocks::blocks(band, *width) {
            if pin.is_some_and(|(_, p)| !block.channels.contains(&p)) {
                continue;
            }
            if !members
                .iter()
                .all(|i| blocks::usable(&block, &of(*i).channels, dfs, pin.is_some(), g2))
            {
                continue;
            }
            let primary = match pin {
                Some((_, p)) => p,
                None => primary(band, radios, heard, &members, &block),
            };
            choices.push(Choice {
                primary,
                width: *width,
                span: block.span(band),
            });
        }
    }
    let mut stuck = false;
    if choices.is_empty() {
        if let Some((_, p)) = pin {
            choices.push(Choice {
                primary: p,
                width: ChannelWidth::Mhz20,
                span: Span::channel(band.frequency(p)),
            });
        } else if let Some(c) = members.iter().find_map(|i| of(*i).current) {
            stuck = true;
            choices.push(Choice::of(&c));
        }
    }
    let widest = choices
        .iter()
        .map(|c| c.width)
        .max()
        .unwrap_or(ChannelWidth::Mhz20);
    Unit {
        radios: members,
        choices,
        widest,
        pin,
        stuck,
    }
}

/// The block's primary channel: where the members' nodes hear the fewest foreign networks'
/// primaries (beacons and contention), the current one unless another is clearly quieter.
fn primary(
    band: Band,
    radios: &[(&PlanRadio, i16)],
    heard: &[Heard],
    members: &[usize],
    block: &blocks::Block,
) -> u16 {
    let score = |channel: u16| -> f32 {
        members
            .iter()
            .flat_map(|i| &heard[*i])
            .filter(|(_, p, _)| *p == channel)
            .map(|(_, _, w)| w)
            .sum()
    };
    let mut best = block.channels[0];
    for c in &block.channels[1..] {
        if score(*c) < score(best) {
            best = *c;
        }
    }
    let span = block.span(band);
    let current = members.iter().find_map(|i| {
        radios[*i].0.current.filter(|c| {
            c.width == block.width
                && block.channels.contains(&c.channel)
                && Span::new(c.center_frequency_mhz, c.width) == span
        })
    });
    match current {
        Some(c) if score(c.channel) <= score(best) + PRIMARY_SLACK => c.channel,
        _ => best,
    }
}
