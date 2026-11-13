//! A simulated site for the planner's tests: nodes and foreign access points placed on a floor,
//! signals from a log-distance path loss, and the planner's inputs derived from them.

use std::collections::BTreeSet;

use uniwrt_core::NodeId;
use uniwrt_model::{
    facts::{Channel, DfsState},
    rf::DfsPolicy,
    site::wireless::TxPower,
    wifi::{Band, ChannelWidth},
};

use super::{ChannelStat, Current, Heard, Link, Plan, PlanInputs, PlanRadio, blocks, plan as run};

/// What scans still hear.
const HEARING_DBM: f32 = -95.0;

#[derive(Debug, Clone)]
pub(super) struct Node {
    pub id: NodeId,
    pub at: (f32, f32),
    pub radios: Vec<PlanRadio>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Foreign {
    pub at: (f32, f32),
    pub band: Band,
    pub primary: u16,
    pub width: ChannelWidth,
    pub tx_dbm: f32,
}

#[derive(Debug, Clone, Default)]
pub(super) struct World {
    pub nodes: Vec<Node>,
    pub foreign: Vec<Foreign>,
    pub stats: Vec<ChannelStat>,
    pub dfs: DfsPolicy,
}

/// The signal at `d` meters from a transmitter at `tx` dBm.
fn signal(band: Band, tx: f32, d: f32) -> f32 {
    let at_1m = match band {
        Band::G2 => 40.0,
        Band::G5 => 47.0,
        Band::G6 | Band::G60 => 48.0,
    };
    tx - at_1m - 30.0 * d.max(1.0).log10()
}

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

#[allow(clippy::cast_possible_truncation)]
fn dbm(value: f32) -> i16 {
    value.round() as i16
}

/// The center of the standard block of `width` holding `primary`.
pub(super) fn center(band: Band, primary: u16, width: ChannelWidth) -> u32 {
    if width == ChannelWidth::Mhz20 {
        return band.frequency(primary);
    }
    let block = blocks::blocks(band, width)
        .into_iter()
        .find(|b| b.channels.contains(&primary))
        .expect("a standard block");
    let span = block.span(band);
    u32::midpoint(span.low, span.high)
}

/// The US's channels: 2.4 GHz 1–11; 5 GHz 36–48, DFS 52–64 and 100–144, then 149–165.
pub(super) fn channels(band: Band, g2_max: u16) -> Vec<Channel> {
    let numbers: Vec<u16> = match band {
        Band::G2 => (1..=g2_max).collect(),
        Band::G5 => (36..=64)
            .step_by(4)
            .chain((100..=144).step_by(4))
            .chain((149..=165).step_by(4))
            .collect(),
        Band::G6 => (1..=233).step_by(4).collect(),
        Band::G60 => Vec::new(),
    };
    numbers
        .into_iter()
        .map(|number| {
            let dfs = band == Band::G5 && (52..=144).contains(&number);
            Channel {
                number,
                frequency_mhz: band.frequency(number),
                dfs,
                dfs_state: dfs.then_some(DfsState::Usable),
                no_ir: dfs,
                max_tx_power_dbm: Some(if band == Band::G2 { 20 } else { 23 }),
            }
        })
        .collect()
}

/// A radio running `primary` at `width` and `tx` dBm, its power the planner's to choose.
pub(super) fn radio(
    node: i64,
    band: Band,
    primary: u16,
    width: ChannelWidth,
    tx: i16,
) -> PlanRadio {
    let widths: BTreeSet<ChannelWidth> = match band {
        Band::G2 => [ChannelWidth::Mhz20, ChannelWidth::Mhz40].into(),
        Band::G5 => [20, 40, 80, 160].map(mhz).into(),
        Band::G6 | Band::G60 => [20, 40, 80, 160, 320].map(mhz).into(),
    };
    PlanRadio {
        node: NodeId::new(node),
        radio: match band {
            Band::G2 => "radio0",
            _ => "radio1",
        }
        .to_owned(),
        band,
        channels: channels(band, 11),
        widths,
        current: Some(Current {
            channel: primary,
            width,
            center_frequency_mhz: center(band, primary, width),
            tx_power_dbm: Some(tx),
        }),
        pin: None,
        pin_width: None,
        tx_power: TxPower::Auto,
        observed: false,
        mesh: false,
        clients: 0,
    }
}

pub(super) fn mhz(width: u16) -> ChannelWidth {
    ChannelWidth::try_from(width).expect("a width")
}

impl World {
    pub(super) fn node(&mut self, id: i64, at: (f32, f32), radios: Vec<PlanRadio>) -> &mut Self {
        self.nodes.push(Node {
            id: NodeId::new(id),
            at,
            radios,
        });
        self
    }

    pub(super) fn radios(&self) -> Vec<PlanRadio> {
        self.nodes.iter().flat_map(|n| n.radios.clone()).collect()
    }

    pub(super) fn links(&self) -> Vec<Link> {
        let mut out = Vec::new();
        for from in &self.nodes {
            for to in self.nodes.iter().filter(|n| n.id != from.id) {
                for theirs in &to.radios {
                    let Some(tx) = theirs.current.and_then(|c| c.tx_power_dbm) else {
                        continue;
                    };
                    let Some(ours) = from.radios.iter().find(|r| r.band == theirs.band) else {
                        continue;
                    };
                    let s = signal(theirs.band, f32::from(tx), distance(from.at, to.at));
                    if s >= HEARING_DBM {
                        out.push(Link {
                            band: theirs.band,
                            from: from.id,
                            to: to.id,
                            signal_dbm: dbm(s),
                            mesh: ours.mesh && theirs.mesh && s >= -80.0,
                        });
                    }
                }
            }
        }
        out
    }

    pub(super) fn heard(&self) -> Vec<Heard> {
        let mut out = Vec::new();
        for node in &self.nodes {
            for f in &self.foreign {
                let s = signal(f.band, f.tx_dbm, distance(node.at, f.at));
                if s >= HEARING_DBM && node.radios.iter().any(|r| r.band == f.band) {
                    out.push(Heard {
                        node: node.id,
                        band: f.band,
                        primary: f.primary,
                        center_frequency_mhz: center(f.band, f.primary, f.width),
                        width: f.width,
                        signal_dbm: dbm(s),
                    });
                }
            }
        }
        out
    }

    pub(super) fn plan(&self) -> Plan {
        self.plan_of(&self.radios())
    }

    /// Plans `radios` in this world's air (the links follow each radio's current power).
    pub(super) fn plan_of(&self, radios: &[PlanRadio]) -> Plan {
        let mut world = self.clone();
        for node in &mut world.nodes {
            node.radios = radios
                .iter()
                .filter(|r| r.node == node.id)
                .cloned()
                .collect();
        }
        let (links, heard) = (world.links(), world.heard());
        run(&PlanInputs {
            radios,
            links: &links,
            heard: &heard,
            channels: &self.stats,
            dfs: self.dfs,
        })
    }

    /// The radios as the plan sets them.
    pub(super) fn applied(&self, plan: &Plan) -> Vec<PlanRadio> {
        let mut radios = self.radios();
        for r in &mut radios {
            let p = plan
                .bands
                .iter()
                .flat_map(|b| &b.radios)
                .find(|p| p.node == r.node && p.radio == r.radio)
                .expect("planned");
            r.current = Some(Current {
                channel: p.planned.channel,
                width: p.planned.width,
                center_frequency_mhz: p.planned.center_frequency_mhz,
                tx_power_dbm: p
                    .planned
                    .tx_power_dbm
                    .or(r.current.and_then(|c| c.tx_power_dbm)),
            });
        }
        radios
    }
}
